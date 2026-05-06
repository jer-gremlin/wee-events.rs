use std::collections::{BTreeSet, HashMap};
use std::marker::PhantomData;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::future::try_join_all;
use libsql::{Connection, TransactionBehavior};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::sleep;
use ulid::{Generator, Ulid};
use wee_events::{
    Aggregate, AggregateId, AggregateType, ChangeSet, CorrelationId, EventData, EventId,
    EventMetadata, EventStore as EventStoreApi, PublishOptions, RawEvent, RecordedEvent,
    RetryDiagnostics, Revision,
};

use crate::{database, Error};

use super::backends::{
    BackendBinding, InMemoryTargetResolver, LocalPartitionCatalog, NamedTargetCatalog,
    SingleTargetCatalog,
};
use super::strategies::{
    GlobalStrategy, LocalPartitionStrategy, PartitionNamingStrategy, PartitionRead,
    PartitionStrategy, SingleTargetPartitionStrategy, SqldNamespacedPartitionStrategy,
};
use super::types::{
    PartitionCatalog, SqldDefaultProvisioner, SqldNamespacedProvisioner, TursoProvisioner,
};

type SharedConnection = Arc<AsyncMutex<Connection>>;
const MAX_AUTO_RETRIES: usize = 5;
const BASE_RETRY_DELAY_MS: u64 = 2;
const MAX_RETRY_DELAY_MS: u64 = 64;

/// SQLite-compatible event store backed by libSQL.
///
/// # Connection lifecycle
///
/// Connections are opened lazily per partition and cached for the lifetime of
/// the store. For strategies that produce many partitions (e.g.,
/// [`AggregateStrategy`] — one per aggregate), the connection map grows
/// monotonically. Long-running processes with many distinct aggregates should
/// consider a bounded strategy like [`HashedStrategy`] or [`TypeStrategy`].
pub struct EventStore<S = GlobalStrategy, C = LocalPartitionCatalog<GlobalStrategy>>
where
    S: PartitionStrategy,
    C: PartitionCatalog<S::Partition>,
{
    strategy: S,
    catalog: C,
    connections: AsyncMutex<HashMap<S::Partition, SharedConnection>>,
    known_partitions: AsyncMutex<BTreeSet<S::Partition>>,
    generator: Mutex<Generator>,
}

pub type LocalStore<S> = EventStore<S, LocalPartitionCatalog<S>>;
pub type InMemoryStore<S> =
    EventStore<S, SingleTargetCatalog<<S as PartitionStrategy>::Partition, InMemoryTargetResolver>>;
pub type SingleRemoteStore<S, R> =
    EventStore<S, SingleTargetCatalog<<S as PartitionStrategy>::Partition, R>>;
pub type NamedRemoteStore<S, R> = EventStore<S, NamedTargetCatalog<S, R>>;
pub type RemoteStore<S, C> = EventStore<S, C>;

impl EventStore<GlobalStrategy, LocalPartitionCatalog<GlobalStrategy>> {
    pub fn builder() -> EventStoreBuilder<MissingBackend, MissingStrategy<()>> {
        EventStoreBuilder::new()
    }
}

impl<S> EventStore<S, LocalPartitionCatalog<S>>
where
    S: LocalPartitionStrategy,
{
    /// Opens a local store with the requested partitioning strategy.
    ///
    /// For `GlobalStrategy`, `path` is the database file.
    /// For sharded strategies, `path` is the root directory.
    pub async fn open_with_strategy(path: impl AsRef<Path>, strategy: S) -> Result<Self, Error> {
        let store = EventStore::from_catalog(
            strategy.clone(),
            LocalPartitionCatalog::new(path.as_ref().to_path_buf(), strategy)?,
        );

        for partition in store.strategy.bootstrap_partitions() {
            let _ = store.ensure_partition_open(&partition).await?;
        }

        Ok(store)
    }
}

impl<S, C> EventStore<S, C>
where
    S: PartitionStrategy,
    C: PartitionCatalog<S::Partition>,
{
    /// Builds a store from a custom partition catalog.
    pub fn from_catalog(strategy: S, catalog: C) -> Self {
        Self {
            strategy,
            catalog,
            connections: AsyncMutex::new(HashMap::new()),
            known_partitions: AsyncMutex::new(BTreeSet::new()),
            generator: Mutex::new(Generator::new()),
        }
    }

    /// Returns all distinct aggregate IDs in the store.
    pub async fn enumerate_aggregates(&self) -> Result<Vec<AggregateId>, Error> {
        let ids = try_join_all(
            self.all_known_partitions()
                .await?
                .into_iter()
                .map(|partition| {
                    let plan = self.strategy.read_plan(&partition);
                    self.enumerate_partition(partition, plan)
                }),
        )
        .await?
        .into_iter()
        .flatten()
        .collect();

        Ok(Self::sorted_unique_ids(ids))
    }

    /// Returns all distinct aggregate IDs of a given type.
    pub async fn enumerate_aggregates_by_type(
        &self,
        aggregate_type: &AggregateType,
    ) -> Result<Vec<AggregateId>, Error> {
        let ids = try_join_all(
            self.all_known_partitions()
                .await?
                .into_iter()
                .map(|partition| {
                    let plan = self.strategy.read_plan_by_type(&partition, aggregate_type);
                    self.enumerate_partition(partition, plan)
                }),
        )
        .await?
        .into_iter()
        .flatten()
        .collect();

        Ok(Self::sorted_unique_ids(ids))
    }

    async fn enumerate_partition(
        &self,
        partition: S::Partition,
        plan: PartitionRead,
    ) -> Result<Vec<AggregateId>, Error> {
        match plan {
            PartitionRead::ScanAll => {
                let Some(conn) = self.open_partition_if_exists(&partition).await? else {
                    return Ok(Vec::new());
                };
                let conn = conn.lock().await;
                Self::enumerate_all_from_connection(&conn).await
            }
            PartitionRead::ScanType(aggregate_type) => {
                let Some(conn) = self.open_partition_if_exists(&partition).await? else {
                    return Ok(Vec::new());
                };
                let conn = conn.lock().await;
                Self::enumerate_by_type_from_connection(&conn, &aggregate_type).await
            }
            PartitionRead::Direct(aggregate_id) => Ok(vec![aggregate_id]),
            PartitionRead::Skip => Ok(Vec::new()),
        }
    }

    async fn ensure_partition_open(
        &self,
        partition: &S::Partition,
    ) -> Result<SharedConnection, Error> {
        self.remember_partition(partition).await;
        if let Some(conn) = self.connections.lock().await.get(partition).cloned() {
            return Ok(conn);
        }

        let target = self.catalog.ensure_target_for_partition(partition).await?;
        let conn = self.open_target(&target).await?;
        self.catalog
            .prepare_connection_for_partition(partition, &conn)
            .await?;
        let new_conn = Arc::new(AsyncMutex::new(conn));

        let mut connections = self.connections.lock().await;
        Ok(connections
            .entry(partition.clone())
            .or_insert_with(|| Arc::clone(&new_conn))
            .clone())
    }

    async fn open_partition_if_exists(
        &self,
        partition: &S::Partition,
    ) -> Result<Option<SharedConnection>, Error> {
        self.remember_partition(partition).await;
        if let Some(conn) = self.connections.lock().await.get(partition).cloned() {
            return Ok(Some(conn));
        }

        let Some(target) = self
            .catalog
            .target_for_existing_partition(partition)
            .await?
        else {
            return Ok(None);
        };

        let conn = self.open_target(&target).await?;
        self.catalog
            .prepare_connection_for_partition(partition, &conn)
            .await?;
        let new_conn = Arc::new(AsyncMutex::new(conn));
        let mut connections = self.connections.lock().await;
        Ok(Some(
            connections
                .entry(partition.clone())
                .or_insert_with(|| Arc::clone(&new_conn))
                .clone(),
        ))
    }

    async fn remember_partition(&self, partition: &S::Partition) {
        let mut known = self.known_partitions.lock().await;
        known.insert(partition.clone());
    }

    async fn all_known_partitions(&self) -> Result<Vec<S::Partition>, Error> {
        let mut partitions: BTreeSet<S::Partition> =
            self.catalog.partitions().await?.into_iter().collect();
        {
            let known = self.known_partitions.lock().await;
            partitions.extend(known.iter().cloned());
        }
        Ok(partitions.into_iter().collect())
    }

    async fn open_target(
        &self,
        target: &super::types::DatabaseTarget,
    ) -> Result<Connection, Error> {
        database::open_event_store_connection(target).await
    }

    fn sorted_unique_ids(mut ids: Vec<AggregateId>) -> Vec<AggregateId> {
        ids.sort_by(|left, right| {
            left.aggregate_type()
                .as_str()
                .cmp(right.aggregate_type().as_str())
                .then_with(|| left.aggregate_key().cmp(right.aggregate_key()))
        });
        ids.dedup();
        ids
    }

    async fn enumerate_all_from_connection(conn: &Connection) -> Result<Vec<AggregateId>, Error> {
        let mut rows = conn
            .query(
                "SELECT DISTINCT aggregate_type, aggregate_key FROM events",
                (),
            )
            .await?;

        let mut ids = Vec::new();
        while let Some(row) = rows.next().await? {
            let aggregate_type: String = row.get(0)?;
            let aggregate_key: String = row.get(1)?;
            ids.push(AggregateId::new(aggregate_type, aggregate_key));
        }

        Ok(ids)
    }

    async fn enumerate_by_type_from_connection(
        conn: &Connection,
        aggregate_type: &AggregateType,
    ) -> Result<Vec<AggregateId>, Error> {
        let mut rows = conn
            .query(
                "SELECT DISTINCT aggregate_key FROM events WHERE aggregate_type = ?1",
                [aggregate_type.as_str()],
            )
            .await?;

        let mut ids = Vec::new();
        while let Some(row) = rows.next().await? {
            let aggregate_key: String = row.get(0)?;
            ids.push(AggregateId::new(aggregate_type.as_str(), aggregate_key));
        }

        Ok(ids)
    }

    fn generate_ulid(&self) -> Result<String, Error> {
        self.generator
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?
            .generate()
            .map(|u| u.to_string())
            .map_err(|e| Error::Internal(e.to_string()))
    }

    async fn load_from_connection(conn: &Connection, id: &AggregateId) -> Result<Aggregate, Error> {
        let mut rows = conn
            .query(
                "SELECT event_id, event_type, revision, causation_id, correlation_id, encoding, data
                 FROM events
                 WHERE aggregate_type = ?1 AND aggregate_key = ?2
                 ORDER BY revision",
                (id.aggregate_type().as_str(), id.aggregate_key()),
            )
            .await?;

        let mut events = Vec::new();
        while let Some(row) = rows.next().await? {
            let event_id: String = row.get(0)?;
            let event_type: String = row.get(1)?;
            let revision: String = row.get(2)?;
            let causation_id: Option<String> = row.get(3)?;
            let correlation_id: Option<String> = row.get(4)?;
            let encoding: String = row.get(5)?;
            let data: Vec<u8> = row.get(6)?;

            events.push(RecordedEvent {
                event_id: EventId::new(event_id),
                event_type: wee_events::EventType::new(event_type),
                revision: Revision::new(revision),
                metadata: EventMetadata {
                    causation_id: causation_id.map(EventId::new),
                    correlation_id: correlation_id.map(CorrelationId::new),
                },
                data: EventData::raw(encoding, data),
            });
        }

        Ok(Aggregate::from_events(id.clone(), events))
    }

    async fn load_aggregate(&self, id: &AggregateId) -> Result<Aggregate, Error> {
        let partition = self.strategy.partition_for_aggregate(id)?;
        self.remember_partition(&partition).await;
        let Some(conn) = self.open_partition_if_exists(&partition).await? else {
            return Ok(Aggregate::empty(id.clone()));
        };
        let conn = conn.lock().await;
        Self::load_from_connection(&conn, id).await
    }

    async fn current_revision(
        conn: &Connection,
        aggregate_id: &AggregateId,
    ) -> Result<Option<String>, Error> {
        let mut rows = conn
            .query(
                "SELECT MAX(revision) FROM events
                 WHERE aggregate_type = ?1 AND aggregate_key = ?2",
                (
                    aggregate_id.aggregate_type().as_str(),
                    aggregate_id.aggregate_key(),
                ),
            )
            .await?;

        let Some(row) = rows.next().await? else {
            return Ok(None);
        };

        row.get(0).map_err(Into::into)
    }

    async fn publish_with_connection(
        &self,
        conn: &Connection,
        aggregate_id: &AggregateId,
        options: PublishOptions,
        events: Vec<RawEvent>,
    ) -> Result<ChangeSet, Error> {
        if events.is_empty() {
            let aggregate = Self::load_from_connection(conn, aggregate_id).await?;
            return Ok(ChangeSet {
                aggregate_id: aggregate_id.clone(),
                revision: aggregate.revision().clone(),
                events: Vec::new(),
            });
        }

        let can_auto_retry = options.expected_revision.is_none();
        let max_attempts = if can_auto_retry { MAX_AUTO_RETRIES } else { 1 };

        let mut last_conflict = None;
        for attempt in 0..max_attempts {
            match self
                .try_publish_once(conn, aggregate_id, &options, &events)
                .await
            {
                Ok(changeset) => return Ok(changeset),
                Err(PublishAttemptError::Conflict(conflict)) if can_auto_retry => {
                    last_conflict = Some(RetryDiagnostics {
                        last_attempted_revision: conflict.attempted_revision.clone(),
                        observed_max_revision: conflict.actual.clone(),
                        possible_clock_skew_ms: possible_clock_skew_ms(
                            &conflict.attempted_revision,
                            &conflict.actual,
                        ),
                    });
                    if attempt + 1 < max_attempts {
                        sleep(retry_delay(attempt)).await;
                    }
                }
                Err(PublishAttemptError::Conflict(conflict)) => {
                    return Err(Error::WeeEvents(wee_events::Error::RevisionConflict {
                        expected: conflict.expected,
                        actual: conflict.actual,
                    }));
                }
                Err(PublishAttemptError::Store(error)) => return Err(error),
            }
        }

        Err(Error::WeeEvents(wee_events::Error::RetryExhausted {
            attempts: max_attempts,
            diagnostics: last_conflict.expect("retry loop always records the last conflict"),
        }))
    }

    async fn publish_to_aggregate(
        &self,
        aggregate_id: &AggregateId,
        options: PublishOptions,
        events: Vec<RawEvent>,
    ) -> Result<ChangeSet, Error> {
        let partition = self.strategy.partition_for_aggregate(aggregate_id)?;
        self.remember_partition(&partition).await;
        let conn = self.ensure_partition_open(&partition).await?;
        let conn = conn.lock().await;
        self.publish_with_connection(&conn, aggregate_id, options, events)
            .await
    }

    async fn try_publish_once(
        &self,
        conn: &Connection,
        aggregate_id: &AggregateId,
        options: &PublishOptions,
        events: &[RawEvent],
    ) -> Result<ChangeSet, PublishAttemptError> {
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(Error::from)?;
        let metadata = EventMetadata {
            causation_id: options.causation_id.clone(),
            correlation_id: options.correlation_id.clone(),
        };

        let mut recorded = Vec::with_capacity(events.len());
        for (index, raw) in events.iter().enumerate() {
            let event_id = EventId::new(self.generate_ulid()?);
            let revision = self.generate_ulid()?;
            let changes = execute_publish_statement(
                &tx,
                PublishRow {
                    index,
                    aggregate_id,
                    options,
                    raw,
                    metadata: &metadata,
                    event_id: &event_id,
                    revision: &revision,
                },
            )
            .await?;

            if changes == 0 {
                let attempted_revision = Revision::new(revision.clone());
                let actual = match Self::current_revision(&tx, aggregate_id).await? {
                    Some(value) => Revision::new(value),
                    None => Revision::zero(),
                };
                let expected = options
                    .expected_revision
                    .clone()
                    .unwrap_or_else(|| attempted_revision.clone());

                return Err(PublishAttemptError::Conflict(PublishConflict {
                    expected,
                    actual,
                    attempted_revision,
                }));
            }

            recorded.push(RecordedEvent {
                event_id,
                event_type: raw.event_type.clone(),
                revision: Revision::new(revision),
                metadata: metadata.clone(),
                data: raw.data.clone(),
            });
        }

        tx.commit().await.map_err(Error::from)?;

        Ok(ChangeSet {
            aggregate_id: aggregate_id.clone(),
            revision: recorded
                .last()
                .expect("recorded is non-empty because events was non-empty")
                .revision
                .clone(),
            events: recorded,
        })
    }
}

#[derive(Debug, Clone)]
struct PublishConflict {
    expected: Revision,
    actual: Revision,
    attempted_revision: Revision,
}

#[derive(Debug)]
enum PublishAttemptError {
    Conflict(PublishConflict),
    Store(Error),
}

impl From<Error> for PublishAttemptError {
    fn from(error: Error) -> Self {
        Self::Store(error)
    }
}

fn retry_delay(attempt: usize) -> Duration {
    let exponent = attempt.min(5) as u32;
    let backoff_ms = (BASE_RETRY_DELAY_MS << exponent).min(MAX_RETRY_DELAY_MS);
    let jitter_ms = if backoff_ms == 0 {
        0
    } else {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as u64
            % (backoff_ms + 1)
    };

    Duration::from_millis(backoff_ms + jitter_ms)
}

fn possible_clock_skew_ms(attempted: &Revision, actual: &Revision) -> Option<u64> {
    let attempted_ulid = Ulid::from_string(attempted.as_str()).ok()?;
    let actual_ulid = Ulid::from_string(actual.as_str()).ok()?;
    actual_ulid
        .timestamp_ms()
        .checked_sub(attempted_ulid.timestamp_ms())
        .filter(|delta| *delta > 0)
}

pub struct MissingBackend;
pub struct InMemoryBackend;
pub struct LocalBackend {
    pub(super) path: PathBuf,
}
pub struct SqldDefaultBackend<R> {
    pub(super) provisioner: R,
}
pub struct SqldNamespacedBackend<R> {
    pub(super) provisioner: R,
}
pub struct TursoBackend<R> {
    pub(super) provisioner: R,
}

pub struct MissingStrategy<P>(PhantomData<P>);
pub struct WithStrategy<S>(S);

pub struct EventStoreBuilder<B, T> {
    backend: B,
    strategy: T,
}

impl EventStoreBuilder<MissingBackend, MissingStrategy<()>> {
    fn new() -> Self {
        Self {
            backend: MissingBackend,
            strategy: MissingStrategy(PhantomData),
        }
    }

    pub fn local(
        self,
        path: impl AsRef<Path>,
    ) -> EventStoreBuilder<LocalBackend, MissingStrategy<()>> {
        EventStoreBuilder {
            backend: LocalBackend {
                path: path.as_ref().to_path_buf(),
            },
            strategy: self.strategy,
        }
    }

    /// Configures an ephemeral single-database in-memory backend.
    pub fn in_memory(self) -> EventStoreBuilder<InMemoryBackend, MissingStrategy<()>> {
        EventStoreBuilder {
            backend: InMemoryBackend,
            strategy: self.strategy,
        }
    }

    pub fn sqld_default<R>(
        self,
        provisioner: R,
    ) -> EventStoreBuilder<SqldDefaultBackend<R>, MissingStrategy<()>>
    where
        R: SqldDefaultProvisioner,
    {
        EventStoreBuilder {
            backend: SqldDefaultBackend { provisioner },
            strategy: MissingStrategy(PhantomData),
        }
    }

    /// Configures a namespaced sqld backend.
    ///
    /// Strategies used with this backend must provide stable partition names.
    /// The provisioner is responsible for mapping those names to sqld
    /// namespaces.
    pub fn sqld_namespaced<R>(
        self,
        provisioner: R,
    ) -> EventStoreBuilder<SqldNamespacedBackend<R>, MissingStrategy<()>>
    where
        R: SqldNamespacedProvisioner,
    {
        EventStoreBuilder {
            backend: SqldNamespacedBackend { provisioner },
            strategy: MissingStrategy(PhantomData),
        }
    }

    pub fn turso<R>(self, provisioner: R) -> EventStoreBuilder<TursoBackend<R>, MissingStrategy<()>>
    where
        R: TursoProvisioner,
    {
        EventStoreBuilder {
            backend: TursoBackend { provisioner },
            strategy: MissingStrategy(PhantomData),
        }
    }

    pub fn strategy<S>(self, strategy: S) -> EventStoreBuilder<MissingBackend, WithStrategy<S>>
    where
        S: PartitionStrategy,
    {
        EventStoreBuilder {
            backend: self.backend,
            strategy: WithStrategy(strategy),
        }
    }
}

impl EventStoreBuilder<LocalBackend, MissingStrategy<()>> {
    pub fn strategy<S>(self, strategy: S) -> EventStoreBuilder<LocalBackend, WithStrategy<S>>
    where
        S: LocalPartitionStrategy,
    {
        EventStoreBuilder {
            backend: self.backend,
            strategy: WithStrategy(strategy),
        }
    }
}

impl EventStoreBuilder<InMemoryBackend, MissingStrategy<()>> {
    pub fn strategy<S>(self, strategy: S) -> EventStoreBuilder<InMemoryBackend, WithStrategy<S>>
    where
        S: SingleTargetPartitionStrategy,
    {
        EventStoreBuilder {
            backend: self.backend,
            strategy: WithStrategy(strategy),
        }
    }
}

impl<R> EventStoreBuilder<SqldDefaultBackend<R>, MissingStrategy<()>>
where
    R: SqldDefaultProvisioner,
{
    pub fn strategy<S>(
        self,
        strategy: S,
    ) -> EventStoreBuilder<SqldDefaultBackend<R>, WithStrategy<S>>
    where
        S: SingleTargetPartitionStrategy,
    {
        EventStoreBuilder {
            backend: self.backend,
            strategy: WithStrategy(strategy),
        }
    }
}

impl<R> EventStoreBuilder<SqldNamespacedBackend<R>, MissingStrategy<()>>
where
    R: SqldNamespacedProvisioner,
{
    pub fn strategy<S>(
        self,
        strategy: S,
    ) -> EventStoreBuilder<SqldNamespacedBackend<R>, WithStrategy<S>>
    where
        S: SqldNamespacedPartitionStrategy + PartitionNamingStrategy,
    {
        EventStoreBuilder {
            backend: self.backend,
            strategy: WithStrategy(strategy),
        }
    }
}

impl<R> EventStoreBuilder<TursoBackend<R>, MissingStrategy<()>>
where
    R: TursoProvisioner,
{
    pub fn strategy<S>(self, strategy: S) -> EventStoreBuilder<TursoBackend<R>, WithStrategy<S>>
    where
        S: PartitionNamingStrategy,
    {
        EventStoreBuilder {
            backend: self.backend,
            strategy: WithStrategy(strategy),
        }
    }
}

impl<S> EventStoreBuilder<MissingBackend, WithStrategy<S>>
where
    S: SingleTargetPartitionStrategy,
{
    pub fn in_memory(self) -> EventStoreBuilder<InMemoryBackend, WithStrategy<S>> {
        EventStoreBuilder {
            backend: InMemoryBackend,
            strategy: self.strategy,
        }
    }
}

impl<S> EventStoreBuilder<MissingBackend, WithStrategy<S>>
where
    S: LocalPartitionStrategy,
{
    pub fn local(self, path: impl AsRef<Path>) -> EventStoreBuilder<LocalBackend, WithStrategy<S>> {
        EventStoreBuilder {
            backend: LocalBackend {
                path: path.as_ref().to_path_buf(),
            },
            strategy: self.strategy,
        }
    }
}

impl<S> EventStoreBuilder<MissingBackend, WithStrategy<S>>
where
    S: SingleTargetPartitionStrategy,
{
    pub fn sqld_default(
        self,
        provisioner: impl SqldDefaultProvisioner,
    ) -> EventStoreBuilder<SqldDefaultBackend<impl SqldDefaultProvisioner>, WithStrategy<S>> {
        EventStoreBuilder {
            backend: SqldDefaultBackend { provisioner },
            strategy: self.strategy,
        }
    }
}

impl<S> EventStoreBuilder<MissingBackend, WithStrategy<S>>
where
    S: PartitionNamingStrategy,
{
    pub fn turso(
        self,
        provisioner: impl TursoProvisioner,
    ) -> EventStoreBuilder<TursoBackend<impl TursoProvisioner>, WithStrategy<S>> {
        EventStoreBuilder {
            backend: TursoBackend { provisioner },
            strategy: self.strategy,
        }
    }
}

impl<S> EventStoreBuilder<MissingBackend, WithStrategy<S>>
where
    S: SqldNamespacedPartitionStrategy + PartitionNamingStrategy,
{
    pub fn sqld_namespaced(
        self,
        provisioner: impl SqldNamespacedProvisioner,
    ) -> EventStoreBuilder<SqldNamespacedBackend<impl SqldNamespacedProvisioner>, WithStrategy<S>>
    {
        EventStoreBuilder {
            backend: SqldNamespacedBackend { provisioner },
            strategy: self.strategy,
        }
    }
}

impl<B, S> EventStoreBuilder<B, WithStrategy<S>>
where
    S: PartitionStrategy,
    B: BackendBinding<S>,
{
    pub async fn open(self) -> Result<EventStore<S, B::Catalog>, Error> {
        let store = EventStore::from_catalog(
            self.strategy.0.clone(),
            self.backend.into_catalog(&self.strategy.0)?,
        );

        for partition in store.strategy.bootstrap_partitions() {
            let _ = store.ensure_partition_open(&partition).await?;
        }

        Ok(store)
    }
}

impl<S, C> EventStoreApi for EventStore<S, C>
where
    S: PartitionStrategy,
    C: PartitionCatalog<S::Partition>,
{
    type Error = Error;

    async fn load(&self, id: &AggregateId) -> Result<Aggregate, Self::Error> {
        self.load_aggregate(id).await
    }

    async fn publish(
        &self,
        aggregate_id: &AggregateId,
        options: PublishOptions,
        events: Vec<RawEvent>,
    ) -> Result<ChangeSet, Self::Error> {
        self.publish_to_aggregate(aggregate_id, options, events)
            .await
    }
}

/// Shared INSERT prefix for all publish variants. The WHERE clause varies
/// by concurrency mode; only the suffix changes.
const INSERT_PREFIX: &str =
    "INSERT INTO events (event_id, aggregate_type, aggregate_key, event_type, revision,
                         causation_id, correlation_id, encoding, data)
     SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9";

/// WHERE clause for the common case: new revision must exceed current max.
const ADVANCE_WHERE: &str = " WHERE ?5 > COALESCE(
         (SELECT MAX(revision) FROM events
          WHERE aggregate_type = ?2 AND aggregate_key = ?3),
         '00000000000000000000000000'
     )";

/// WHERE clause for initial publish: aggregate must have no events.
const INITIAL_WHERE: &str = " WHERE NOT EXISTS (
         SELECT 1 FROM events
         WHERE aggregate_type = ?2 AND aggregate_key = ?3
     )";

/// Extra AND clause for exact revision match (appended after ADVANCE_WHERE).
const EXACT_SUFFIX: &str = " AND (SELECT MAX(revision) FROM events
          WHERE aggregate_type = ?2 AND aggregate_key = ?3) = ?10";

struct PublishRow<'a> {
    index: usize,
    aggregate_id: &'a AggregateId,
    options: &'a PublishOptions,
    raw: &'a RawEvent,
    metadata: &'a EventMetadata,
    event_id: &'a EventId,
    revision: &'a str,
}

/// Determines the correct concurrency mode and executes the publish INSERT.
///
/// Three modes, all sharing `INSERT_PREFIX`:
/// - **initial**: first event for this aggregate (`expected_revision` is zero)
/// - **exact**: caller expects a specific prior revision (`expected_revision` is non-zero)
/// - **advance**: no expected revision — just ensure monotonic ordering
async fn execute_publish_statement(
    tx: &libsql::Transaction,
    row: PublishRow<'_>,
) -> Result<u64, Error> {
    let causation = row.metadata.causation_id.as_ref().map(|id| id.as_str());
    let correlation = row.metadata.correlation_id.as_ref().map(|id| id.as_str());

    match (row.index, &row.options.expected_revision) {
        (0, Some(expected)) if expected.is_zero() => {
            let sql = format!("{INSERT_PREFIX}{INITIAL_WHERE}");
            tx.execute(
                &sql,
                libsql::params![
                    row.event_id.as_str(),
                    row.aggregate_id.aggregate_type().as_str(),
                    row.aggregate_id.aggregate_key(),
                    row.raw.event_type.as_str(),
                    row.revision,
                    causation,
                    correlation,
                    row.raw.data.encoding.as_str(),
                    row.raw.data.data.clone(),
                ],
            )
            .await
            .map_err(Into::into)
        }
        (0, Some(expected)) => {
            let sql = format!("{INSERT_PREFIX}{ADVANCE_WHERE}{EXACT_SUFFIX}");
            tx.execute(
                &sql,
                libsql::params![
                    row.event_id.as_str(),
                    row.aggregate_id.aggregate_type().as_str(),
                    row.aggregate_id.aggregate_key(),
                    row.raw.event_type.as_str(),
                    row.revision,
                    causation,
                    correlation,
                    row.raw.data.encoding.as_str(),
                    row.raw.data.data.clone(),
                    expected.as_str(),
                ],
            )
            .await
            .map_err(Into::into)
        }
        _ => {
            let sql = format!("{INSERT_PREFIX}{ADVANCE_WHERE}");
            tx.execute(
                &sql,
                libsql::params![
                    row.event_id.as_str(),
                    row.aggregate_id.aggregate_type().as_str(),
                    row.aggregate_id.aggregate_key(),
                    row.raw.event_type.as_str(),
                    row.revision,
                    causation,
                    correlation,
                    row.raw.data.encoding.as_str(),
                    row.raw.data.data.clone(),
                ],
            )
            .await
            .map_err(Into::into)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::possible_clock_skew_ms;
    use ulid::Ulid;
    use wee_events::Revision;

    #[test]
    fn possible_clock_skew_reports_positive_timestamp_gap() {
        let attempted = Revision::new(Ulid::from_parts(1_000, 7).to_string());
        let actual = Revision::new(Ulid::from_parts(1_017, 3).to_string());

        assert_eq!(possible_clock_skew_ms(&attempted, &actual), Some(17));
    }

    #[test]
    fn possible_clock_skew_ignores_non_positive_gaps() {
        let attempted = Revision::new(Ulid::from_parts(1_000, 7).to_string());
        let same_ms = Revision::new(Ulid::from_parts(1_000, 99).to_string());
        let earlier = Revision::new(Ulid::from_parts(999, 3).to_string());

        assert_eq!(possible_clock_skew_ms(&attempted, &same_ms), None);
        assert_eq!(possible_clock_skew_ms(&attempted, &earlier), None);
    }
}
