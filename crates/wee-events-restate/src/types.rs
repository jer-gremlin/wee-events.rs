use serde::{Deserialize, Serialize};
use wee_events::{AggregateId, CommandName, Revision};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub correlation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRequest {
    pub name: CommandName,
    pub target: AggregateId,
    pub command: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteRequest {
    pub command: CommandRequest,
    pub metadata: Metadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteResponse {
    pub aggregate: AggregateId,
    pub revision: Revision,
    pub state: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteNotification {
    pub command: CommandRequest,
    pub response: ExecuteResponse,
    pub metadata: Metadata,
}
