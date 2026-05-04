use crate::Error;

use super::types::DatabaseTarget;
use libsql::Connection;
use std::future::Future;

/// Maps logical partitions to concrete database targets.
///
/// This is a static extension point; catalogs are composed into concrete store
/// types rather than used behind trait objects.
pub trait PartitionCatalog<P>: Send + Sync {
    fn ensure_target_for_partition(
        &self,
        partition: &P,
    ) -> impl Future<Output = Result<DatabaseTarget, Error>> + Send;

    fn target_for_existing_partition(
        &self,
        partition: &P,
    ) -> impl Future<Output = Result<Option<DatabaseTarget>, Error>> + Send;

    fn partitions(&self) -> impl Future<Output = Result<Vec<P>, Error>> + Send;

    fn prepare_connection_for_partition(
        &self,
        _partition: &P,
        _conn: &Connection,
    ) -> impl Future<Output = Result<(), Error>> + Send {
        async { Ok(()) }
    }
}
