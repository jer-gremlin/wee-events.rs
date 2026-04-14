use std::path::Path;

use wee_events::{AggregateId, AggregateType};

use crate::Error;

use super::{
    LocalPartitionLayout, LocalPartitionStrategy, NamedPartition, PartitionName,
    PartitionNamingStrategy, PartitionRead, PartitionStrategy, SqldNamespacedPartitionStrategy,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AggregateStrategy;

pub type AggregatePartition = NamedPartition<AggregateId>;

impl PartitionStrategy for AggregateStrategy {
    type Partition = AggregatePartition;

    fn partition_for_aggregate(
        &self,
        aggregate_id: &AggregateId,
    ) -> Result<Self::Partition, Error> {
        Ok(AggregatePartition::new(
            aggregate_id.to_string(),
            aggregate_id.clone(),
        ))
    }

    fn read_plan(&self, partition: &Self::Partition) -> PartitionRead {
        PartitionRead::Direct(partition.key().clone())
    }

    fn read_plan_by_type(
        &self,
        partition: &Self::Partition,
        aggregate_type: &AggregateType,
    ) -> PartitionRead {
        if partition.key().aggregate_type() == aggregate_type {
            PartitionRead::Direct(partition.key().clone())
        } else {
            PartitionRead::Skip
        }
    }
}

impl PartitionNamingStrategy for AggregateStrategy {
    fn partition_name<'a>(&self, partition: &'a Self::Partition) -> PartitionName<'a> {
        PartitionName::Named(partition.name())
    }

    fn partition_from_name(&self, name: &str) -> Result<Self::Partition, Error> {
        let aggregate_id = name.parse::<AggregateId>().map_err(|error| {
            Error::Configuration(format!(
                "invalid aggregate partition name '{name}': {error}"
            ))
        })?;
        Ok(AggregatePartition::new(
            aggregate_id.to_string(),
            aggregate_id,
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
                "SELECT DISTINCT aggregate_type, aggregate_key
                 FROM events
                 ORDER BY aggregate_type, aggregate_key",
                (),
            )
            .await?;

        let Some(row) = rows.next().await? else {
            return Ok(None);
        };

        let aggregate_type: String = row.get(0)?;
        let aggregate_key: String = row.get(1)?;
        if rows.next().await?.is_some() {
            return Err(Error::Configuration(format!(
                "aggregate partition '{name}' contains multiple aggregates"
            )));
        }

        let aggregate_id = AggregateId::new(aggregate_type, aggregate_key);
        Ok(Some(AggregatePartition::new(
            aggregate_id.to_string(),
            aggregate_id,
        )))
    }
}

impl LocalPartitionStrategy for AggregateStrategy {
    fn initialize_root(&self, root: &Path) -> Result<(), Error> {
        std::fs::create_dir_all(root)?;
        Ok(())
    }

    fn local_partition_layout(&self) -> LocalPartitionLayout {
        LocalPartitionLayout::NamedDatabases
    }
}

impl SqldNamespacedPartitionStrategy for AggregateStrategy {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store::DatabaseTarget;

    #[test]
    fn partition_name_matches_aggregate_id() {
        let aggregate_id = AggregateId::new("campaign/run", "urn:uuid:abc/123");
        let partition = AggregateStrategy
            .partition_for_aggregate(&aggregate_id)
            .expect("routing should succeed");

        assert_eq!(partition.name(), "campaign/run:urn:uuid:abc/123");
        assert_eq!(
            partition.key(),
            &AggregateId::new("campaign/run", "urn:uuid:abc/123")
        );
    }

    #[test]
    fn partition_from_name_restores_aggregate_partition() {
        let partition = AggregateStrategy
            .partition_from_name("campaign/run:urn:uuid:abc/123")
            .expect("partition restore should succeed");

        assert_eq!(
            partition,
            AggregatePartition::new(
                "campaign/run:urn:uuid:abc/123",
                AggregateId::new("campaign/run", "urn:uuid:abc/123"),
            )
        );
    }

    #[tokio::test]
    async fn partition_from_target_name_reads_exact_aggregate_from_store() {
        let temp_dir = tempfile::tempdir().expect("tempdir should succeed");
        let path = temp_dir.path().join("aggregate.db");
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
                "01ARZ3NDEKTSV4RRFFQ69G5FAX",
                "campaign/run",
                "urn:uuid:abc/123",
                "created",
                "01ARZ3NDEKTSV4RRFFQ69G5FAY",
                "json",
                Vec::<u8>::new(),
            ],
        )
        .await
        .expect("insert should succeed");

        let partition = AggregateStrategy
            .partition_from_target_name("campaign-run-urn-uuid-abc-123", &target)
            .await
            .expect("discovery should succeed")
            .expect("non-empty partition should be discovered");

        assert_eq!(
            partition,
            AggregatePartition::new(
                "campaign/run:urn:uuid:abc/123",
                AggregateId::new("campaign/run", "urn:uuid:abc/123"),
            )
        );
    }
}
