use std::path::Path;

use wee_events::{AggregateId, AggregateType};

use crate::Error;

use super::{
    LocalPartitionLayout, LocalPartitionStrategy, NamedPartition, PartitionName,
    PartitionNamingStrategy, PartitionRead, PartitionStrategy, SqldNamespacedPartitionStrategy,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TypeStrategy;

pub type TypePartition = NamedPartition<AggregateType>;

impl PartitionStrategy for TypeStrategy {
    type Partition = TypePartition;

    fn partition_for_aggregate(
        &self,
        aggregate_id: &AggregateId,
    ) -> Result<Self::Partition, Error> {
        let aggregate_type = aggregate_id.aggregate_type().clone();
        Ok(TypePartition::new(
            aggregate_type.as_str().to_string(),
            aggregate_type,
        ))
    }

    fn read_plan(&self, partition: &Self::Partition) -> PartitionRead {
        PartitionRead::ScanType(partition.key().clone())
    }

    fn read_plan_by_type(
        &self,
        partition: &Self::Partition,
        aggregate_type: &AggregateType,
    ) -> PartitionRead {
        if partition.key() == aggregate_type {
            PartitionRead::ScanType(aggregate_type.clone())
        } else {
            PartitionRead::Skip
        }
    }
}

impl PartitionNamingStrategy for TypeStrategy {
    fn partition_name<'a>(&self, partition: &'a Self::Partition) -> PartitionName<'a> {
        PartitionName::Named(partition.name())
    }

    fn partition_from_name(&self, name: &str) -> Result<Self::Partition, Error> {
        let aggregate_type = AggregateType::new(name);
        Ok(TypePartition::new(
            aggregate_type.as_str().to_string(),
            aggregate_type,
        ))
    }

    async fn partition_from_target_name(
        &self,
        name: &str,
        target: &crate::event_store::DatabaseTarget,
    ) -> Result<Option<Self::Partition>, Error> {
        let conn = crate::database::open_event_store_connection(target).await?;
        let mut rows = conn
            .query(
                "SELECT DISTINCT aggregate_type FROM events ORDER BY aggregate_type",
                (),
            )
            .await?;

        let Some(row) = rows.next().await? else {
            return Ok(None);
        };

        let aggregate_type: String = row.get(0)?;
        if rows.next().await?.is_some() {
            return Err(Error::Configuration(format!(
                "partition '{name}' contains multiple aggregate types"
            )));
        }

        let aggregate_type = AggregateType::new(aggregate_type);
        Ok(Some(TypePartition::new(
            aggregate_type.as_str().to_string(),
            aggregate_type,
        )))
    }
}

impl LocalPartitionStrategy for TypeStrategy {
    fn initialize_root(&self, root: &Path) -> Result<(), Error> {
        std::fs::create_dir_all(root)?;
        Ok(())
    }

    fn local_partition_layout(&self) -> LocalPartitionLayout {
        LocalPartitionLayout::NamedDatabases
    }
}

impl SqldNamespacedPartitionStrategy for TypeStrategy {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store::DatabaseTarget;

    #[test]
    fn partition_name_matches_aggregate_type() {
        let partition = TypeStrategy
            .partition_for_aggregate(&AggregateId::new("a/b:c", "123"))
            .expect("routing should succeed");

        assert_eq!(partition.name(), "a/b:c");
        assert_eq!(partition.key(), &AggregateType::new("a/b:c"));
    }

    #[test]
    fn partition_from_name_restores_type_partition() {
        let partition = TypeStrategy
            .partition_from_name("a/b:c")
            .expect("partition restore should succeed");

        assert_eq!(
            partition,
            TypePartition::new("a/b:c", AggregateType::new("a/b:c"))
        );
    }

    #[tokio::test]
    async fn partition_from_target_name_reads_exact_type_from_store() {
        let temp_dir = tempfile::tempdir().expect("tempdir should succeed");
        let path = temp_dir.path().join("type.db");
        let target = DatabaseTarget::Local(path);
        let conn = crate::database::open_event_store_connection(&target)
            .await
            .expect("connection should open");

        conn.execute(
            "INSERT INTO events (
                 event_id, aggregate_type, aggregate_key, event_type, revision,
                 causation_id, correlation_id, encoding, data
             ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6, ?7)",
            libsql::params![
                "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "campaign/run",
                "123",
                "created",
                "01ARZ3NDEKTSV4RRFFQ69G5FAW",
                "json",
                Vec::<u8>::new(),
            ],
        )
        .await
        .expect("insert should succeed");

        let partition = TypeStrategy
            .partition_from_target_name("campaign-run", &target)
            .await
            .expect("discovery should succeed")
            .expect("non-empty partition should be discovered");

        assert_eq!(
            partition,
            TypePartition::new("campaign/run", AggregateType::new("campaign/run"))
        );
    }
}
