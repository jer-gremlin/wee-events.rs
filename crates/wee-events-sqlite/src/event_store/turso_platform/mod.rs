mod api;
mod sanitize;

#[cfg(feature = "turso")]
pub use api::TursoHttpClient;

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use crate::Error;
use api::{ApiError, TursoPlatformApi};
use sanitize::sanitize_database_name;

use super::strategies::PartitionName;
use super::types::{DatabaseTarget, NamedTargetProvisioner, TursoProvisioner};

/// Configuration for connecting to the Turso Platform API.
///
/// Use [`from_env`](Self::from_env) to load from standard environment variables,
/// or construct directly with named fields.
///
/// # Warning
///
/// Changing `prefix` after databases have been created will make existing
/// partitions invisible to [`TursoPlatformProvisioner`]. The provisioner
/// identifies its databases by prefix — orphaned databases must be cleaned
/// up manually via the Turso Platform API.
#[derive(Debug, Clone)]
pub struct TursoPlatformConfig {
    /// Turso organization slug.
    pub org: String,
    /// Database group name. Determines the regions databases are placed in.
    pub group: String,
    /// Prefix for database names. Each partition becomes `{prefix}-{sanitized_name}`.
    pub prefix: String,
    /// Platform API token for managing databases.
    pub api_token: String,
    /// Group-level auth token for connecting to databases. Create with
    /// `turso group tokens create <group>`.
    pub group_token: String,
    /// Override the Platform API base URL. Defaults to `https://api.turso.tech`.
    pub base_url: Option<String>,
}

impl TursoPlatformConfig {
    const DEFAULT_BASE_URL: &str = "https://api.turso.tech";

    /// Load configuration from environment variables.
    ///
    /// | Variable | Field |
    /// |----------|-------|
    /// | `TURSO_ORG` | `org` |
    /// | `TURSO_GROUP` | `group` |
    /// | `TURSO_DB_PREFIX` | `prefix` |
    /// | `TURSO_API_TOKEN` | `api_token` |
    /// | `TURSO_GROUP_TOKEN` | `group_token` |
    /// | `TURSO_API_BASE_URL` | `base_url` (optional) |
    pub fn from_env() -> Result<Self, Error> {
        let read = |name: &str| -> Result<String, Error> {
            std::env::var(name)
                .map_err(|_| Error::Configuration(format!("missing environment variable: {name}")))
        };

        Ok(Self {
            org: read("TURSO_ORG")?,
            group: read("TURSO_GROUP")?,
            prefix: read("TURSO_DB_PREFIX")?,
            api_token: read("TURSO_API_TOKEN")?,
            group_token: read("TURSO_GROUP_TOKEN")?,
            base_url: std::env::var("TURSO_API_BASE_URL").ok(),
        })
    }
}

/// Production provisioner that creates per-partition Turso databases via the
/// Platform API.
///
/// Each partition name is sanitized into a valid Turso database name
/// (`{prefix}-{sanitized}`), created on demand, and connected to using the
/// group-level auth token.
///
/// # Examples
///
/// ```text
/// let config = TursoPlatformConfig {
///     org: "my-org".into(),
///     group: "default".into(),
///     prefix: "myapp".into(),
///     api_token: "turso-api-token".into(),
///     group_token: "group-auth-token".into(),
///     base_url: None,
/// };
/// let provisioner = TursoPlatformProvisioner::new(config);
///
/// let store = SqliteEventStore::builder()
///     .turso(provisioner)
///     .strategy(TypeStrategy)
///     .open()
///     .await?;
/// ```
pub struct TursoPlatformProvisioner<A = api::TursoHttpClient> {
    api: A,
    group: String,
    prefix: String,
    group_token: String,
    cache: Mutex<HashMap<String, DatabaseTarget>>,
    known_names: Mutex<HashSet<String>>,
}

#[cfg(feature = "turso")]
impl TursoPlatformProvisioner {
    /// Create a new provisioner from configuration.
    pub fn new(config: TursoPlatformConfig) -> Self {
        let base_url = config
            .base_url
            .unwrap_or_else(|| TursoPlatformConfig::DEFAULT_BASE_URL.to_string());
        let client = api::TursoHttpClient::new(base_url, config.api_token, config.org);
        Self::with_api(client, config.group, config.prefix, config.group_token)
    }
}

impl<A: TursoPlatformApi> TursoPlatformProvisioner<A> {
    fn with_api(api: A, group: String, prefix: String, group_token: String) -> Self {
        Self {
            api,
            group,
            prefix,
            group_token,
            cache: Mutex::new(HashMap::new()),
            known_names: Mutex::new(HashSet::new()),
        }
    }

    fn db_name_for(&self, name: PartitionName<'_>) -> String {
        match name {
            PartitionName::Default => sanitize_database_name("", &self.prefix),
            PartitionName::Named(n) => sanitize_database_name(n, &self.prefix),
        }
    }

    fn record_name(&self, name: PartitionName<'_>) {
        if let PartitionName::Named(n) = name {
            self.known_names.lock().unwrap().insert(n.to_string());
        }
    }

    fn make_target(&self, hostname: &str) -> DatabaseTarget {
        DatabaseTarget::Turso {
            url: format!("libsql://{hostname}"),
            auth_token: self.group_token.clone(),
        }
    }
}

impl<A: TursoPlatformApi> NamedTargetProvisioner for TursoPlatformProvisioner<A> {
    async fn ensure_target_for_name(
        &self,
        name: PartitionName<'_>,
    ) -> Result<DatabaseTarget, Error> {
        let db_name = self.db_name_for(name);

        // Check cache
        if let Some(target) = self.cache.lock().unwrap().get(&db_name) {
            return Ok(target.clone());
        }

        // Create database (handle AlreadyExists by fetching)
        let info = match self.api.create_database(&db_name, &self.group).await {
            Ok(info) => info,
            Err(ApiError::AlreadyExists) => self
                .api
                .get_database(&db_name)
                .await
                .map_err(|e| Error::Internal(e.to_string()))?
                .ok_or_else(|| {
                    Error::Internal(format!(
                        "database '{db_name}' reported as existing but not found"
                    ))
                })?,
            Err(ApiError::AuthFailure(msg)) => {
                return Err(Error::Configuration(format!(
                    "Turso API auth failure: {msg}"
                )));
            }
            Err(e) => return Err(Error::Internal(e.to_string())),
        };

        let target = self.make_target(&info.hostname);
        self.cache
            .lock()
            .unwrap()
            .insert(db_name, target.clone());
        self.record_name(name);
        Ok(target)
    }

    async fn target_for_existing_name(
        &self,
        name: PartitionName<'_>,
    ) -> Result<Option<DatabaseTarget>, Error> {
        let db_name = self.db_name_for(name);

        // Check cache
        if let Some(target) = self.cache.lock().unwrap().get(&db_name) {
            return Ok(Some(target.clone()));
        }

        // Query API
        let Some(info) = self
            .api
            .get_database(&db_name)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?
        else {
            return Ok(None);
        };

        let target = self.make_target(&info.hostname);
        self.cache
            .lock()
            .unwrap()
            .insert(db_name, target.clone());
        self.record_name(name);
        Ok(Some(target))
    }

    async fn names(&self) -> Result<Vec<String>, Error> {
        // Prefer in-memory cache of original names
        let known = self.known_names.lock().unwrap();
        if !known.is_empty() {
            return Ok(known.iter().cloned().collect());
        }
        drop(known);

        // Fallback: list from API, strip prefix (best-effort, lossy)
        let databases = self
            .api
            .list_databases(&self.group)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?;

        let prefix_dash = format!("{}-", self.prefix);
        Ok(databases
            .into_iter()
            .filter_map(|db| db.name.strip_prefix(&prefix_dash).map(String::from))
            .collect())
    }
}

impl<A: TursoPlatformApi> TursoProvisioner for TursoPlatformProvisioner<A> {}

#[cfg(test)]
mod tests {
    use super::*;
    use api::fake::FakeTursoPlatformApi;
    use std::sync::atomic::Ordering;

    fn test_provisioner() -> (
        std::sync::Arc<FakeTursoPlatformApi>,
        TursoPlatformProvisioner<std::sync::Arc<FakeTursoPlatformApi>>,
    ) {
        let api = std::sync::Arc::new(FakeTursoPlatformApi::new());
        let provisioner = TursoPlatformProvisioner::with_api(
            api.clone(),
            "default".to_string(),
            "myapp".to_string(),
            "group-tok-123".to_string(),
        );
        (api, provisioner)
    }

    #[tokio::test]
    async fn ensure_creates_database_and_returns_turso_target() {
        let (_api, provisioner) = test_provisioner();
        let target = provisioner
            .ensure_target_for_name(PartitionName::Named("orders"))
            .await
            .unwrap();

        assert_eq!(
            target,
            DatabaseTarget::Turso {
                url: "libsql://myapp-orders-testorg.turso.io".to_string(),
                auth_token: "group-tok-123".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn ensure_default_partition_uses_prefix_only() {
        let (_api, provisioner) = test_provisioner();
        let target = provisioner
            .ensure_target_for_name(PartitionName::Default)
            .await
            .unwrap();

        assert_eq!(
            target,
            DatabaseTarget::Turso {
                url: "libsql://myapp-testorg.turso.io".to_string(),
                auth_token: "group-tok-123".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn ensure_returns_cached_on_second_call() {
        let (api, provisioner) = test_provisioner();
        provisioner
            .ensure_target_for_name(PartitionName::Named("orders"))
            .await
            .unwrap();
        provisioner
            .ensure_target_for_name(PartitionName::Named("orders"))
            .await
            .unwrap();

        assert_eq!(api.create_calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn ensure_handles_already_exists_by_fetching() {
        let (api, provisioner) = test_provisioner();
        // Pre-create so next ensure hits AlreadyExists
        api.create_database("myapp-orders", "default")
            .await
            .unwrap();

        let target = provisioner
            .ensure_target_for_name(PartitionName::Named("orders"))
            .await
            .unwrap();

        assert_eq!(api.create_calls.load(Ordering::Relaxed), 2); // our call + pre-create
        assert_eq!(api.get_calls.load(Ordering::Relaxed), 1);
        assert!(matches!(target, DatabaseTarget::Turso { .. }));
    }

    #[tokio::test]
    async fn ensure_sanitizes_partition_name() {
        let (api, provisioner) = test_provisioner();
        provisioner
            .ensure_target_for_name(PartitionName::Named("Tenant:ACME"))
            .await
            .unwrap();

        // The fake stores the sanitized name
        assert!(api.get_database("myapp-tenant-acme").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn ensure_records_original_name() {
        let (_api, provisioner) = test_provisioner();
        provisioner
            .ensure_target_for_name(PartitionName::Named("orders"))
            .await
            .unwrap();

        let names = provisioner.names().await.unwrap();
        assert_eq!(names, vec!["orders".to_string()]);
    }

    #[tokio::test]
    async fn existing_returns_cached() {
        let (api, provisioner) = test_provisioner();
        provisioner
            .ensure_target_for_name(PartitionName::Named("orders"))
            .await
            .unwrap();
        api.get_calls.store(0, Ordering::Relaxed);

        let target = provisioner
            .target_for_existing_name(PartitionName::Named("orders"))
            .await
            .unwrap();

        assert!(target.is_some());
        assert_eq!(api.get_calls.load(Ordering::Relaxed), 0); // no API call
    }

    #[tokio::test]
    async fn existing_falls_back_to_api() {
        let (api, provisioner) = test_provisioner();
        // Pre-create directly in API (not through provisioner)
        api.create_database("myapp-orders", "default")
            .await
            .unwrap();

        let target = provisioner
            .target_for_existing_name(PartitionName::Named("orders"))
            .await
            .unwrap();

        assert!(target.is_some());
        assert_eq!(api.get_calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn existing_returns_none_for_unknown() {
        let (_api, provisioner) = test_provisioner();
        let target = provisioner
            .target_for_existing_name(PartitionName::Named("missing"))
            .await
            .unwrap();
        assert!(target.is_none());
    }

    #[tokio::test]
    async fn names_returns_known_names_from_cache() {
        let (_api, provisioner) = test_provisioner();
        provisioner
            .ensure_target_for_name(PartitionName::Named("orders"))
            .await
            .unwrap();
        provisioner
            .ensure_target_for_name(PartitionName::Named("users"))
            .await
            .unwrap();

        let mut names = provisioner.names().await.unwrap();
        names.sort();
        assert_eq!(names, vec!["orders", "users"]);
    }

    #[tokio::test]
    async fn names_falls_back_to_api_when_cache_empty() {
        let (api, provisioner) = test_provisioner();
        // Create directly in API (bypassing provisioner cache)
        api.create_database("myapp-orders", "default")
            .await
            .unwrap();
        api.create_database("other-db", "default").await.unwrap();

        let names = provisioner.names().await.unwrap();
        // Only "myapp-orders" matches the prefix; "other-db" is filtered out
        assert_eq!(names, vec!["orders".to_string()]);
    }

    #[test]
    fn from_env_returns_error_on_missing_var() {
        // Clear any existing vars
        std::env::remove_var("TURSO_ORG");
        let result = TursoPlatformConfig::from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("TURSO_ORG"));
    }
}
