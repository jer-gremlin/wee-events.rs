use std::marker::PhantomData;

use wee_events::{AggregateId, CommandName, Entity, Rejection};

use crate::names;
use crate::types::{CommandRequest, EntityResponse, ExecuteRequest, Metadata};

/// Restate-backed service client implementing `EntityLoader<S>` + `CommandExecutor<S>`
/// by calling the executor workflow and loader service over the Restate ingress HTTP API.
pub struct RestateClient<S> {
    http: reqwest::Client,
    ingress_url: String,
    service_name: String,
    _state: PhantomData<S>,
}

impl<S> RestateClient<S> {
    pub fn new(ingress_url: impl Into<String>, service_name: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            ingress_url: ingress_url.into(),
            service_name: service_name.into(),
            _state: PhantomData,
        }
    }

    fn executor_name(&self) -> String {
        names::executor_name(&self.service_name)
    }

    fn loader_name(&self) -> String {
        names::loader_name(&self.service_name)
    }

    fn encode_key(target: &AggregateId) -> String {
        format!("{}:{}", target.aggregate_type(), target.aggregate_key())
    }

    fn generate_correlation_id(target: &AggregateId, command_name: &CommandName) -> String {
        let key = Self::encode_key(target);
        format!("{}-{}-{}", key, command_name, nanoid::nanoid!())
    }

    /// Execute a command with an explicit idempotency key. Resubmitting
    /// the same key returns the original result without re-executing.
    ///
    /// The key is used as the Restate workflow ID, so it must be unique
    /// across all commands for this service.
    pub async fn execute_idempotent(
        &self,
        name: &CommandName,
        target: &AggregateId,
        command: serde_json::Value,
        idempotency_key: impl Into<String>,
    ) -> wee_events::Result<Entity<S>>
    where
        S: serde::de::DeserializeOwned,
    {
        let idempotency_key = idempotency_key.into();
        let correlation_id = Self::generate_correlation_id(target, name);

        let request = ExecuteRequest {
            command: CommandRequest {
                name: name.clone(),
                target: target.clone(),
                command,
            },
            metadata: Metadata {
                correlation_id,
                causation_id: None,
                idempotency_key: Some(idempotency_key.clone()),
            },
        };

        // Use the idempotency key as the workflow ID
        let url = format!(
            "{}/{}/{}/run",
            self.ingress_url,
            self.executor_name(),
            idempotency_key,
        );

        let resp = self
            .http
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| wee_events::Error::Store(Box::new(e)))?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            if let Ok(rejection) = serde_json::from_str::<Rejection>(&text) {
                return Err(wee_events::Error::Rejection(rejection));
            }
            if let Ok(envelope) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(message) = envelope.get("message").and_then(|m| m.as_str()) {
                    if let Ok(rejection) = serde_json::from_str::<Rejection>(message) {
                        return Err(wee_events::Error::Rejection(rejection));
                    }
                }
            }
            return Err(wee_events::Error::Store(text.into()));
        }

        let exec_resp: EntityResponse = resp
            .json()
            .await
            .map_err(|e| wee_events::Error::Store(Box::new(e)))?;

        let state: S = serde_json::from_value(exec_resp.state)?;
        Ok(Entity {
            aggregate_id: exec_resp.aggregate,
            revision: exec_resp.revision,
            state,
        })
    }
}

impl<S> wee_events::EntityLoader<S> for RestateClient<S>
where
    S: serde::de::DeserializeOwned + Send + Sync,
{
    async fn load(&self, id: &AggregateId) -> wee_events::Result<Entity<S>> {
        let url = format!("{}/{}/load", self.ingress_url, self.loader_name());

        let resp = self
            .http
            .post(&url)
            .json(id)
            .send()
            .await
            .map_err(|e| wee_events::Error::Store(Box::new(e)))?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(wee_events::Error::Store(text.into()));
        }

        let exec_resp: EntityResponse = resp
            .json()
            .await
            .map_err(|e| wee_events::Error::Store(Box::new(e)))?;

        let state: S = serde_json::from_value(exec_resp.state)?;
        Ok(Entity {
            aggregate_id: exec_resp.aggregate,
            revision: exec_resp.revision,
            state,
        })
    }
}

impl<S> wee_events::CommandExecutor<S> for RestateClient<S>
where
    S: serde::de::DeserializeOwned + Send + Sync,
{
    async fn execute(
        &self,
        name: &CommandName,
        target: &AggregateId,
        command: serde_json::Value,
    ) -> wee_events::Result<Entity<S>> {
        let correlation_id = Self::generate_correlation_id(target, name);

        let request = ExecuteRequest {
            command: CommandRequest {
                name: name.clone(),
                target: target.clone(),
                command,
            },
            metadata: Metadata {
                correlation_id: correlation_id.clone(),
                causation_id: None,
                idempotency_key: None,
            },
        };

        let url = format!(
            "{}/{}/{}/run",
            self.ingress_url,
            self.executor_name(),
            correlation_id,
        );

        let resp = self
            .http
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| wee_events::Error::Store(Box::new(e)))?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            // Try to parse as a Rejection from the terminal error payload
            if let Ok(rejection) = serde_json::from_str::<Rejection>(&text) {
                return Err(wee_events::Error::Rejection(rejection));
            }
            // Try the Restate error envelope (message field contains the JSON)
            if let Ok(envelope) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(message) = envelope.get("message").and_then(|m| m.as_str()) {
                    if let Ok(rejection) = serde_json::from_str::<Rejection>(message) {
                        return Err(wee_events::Error::Rejection(rejection));
                    }
                }
            }
            return Err(wee_events::Error::Store(text.into()));
        }

        let exec_resp: EntityResponse = resp
            .json()
            .await
            .map_err(|e| wee_events::Error::Store(Box::new(e)))?;

        let state: S = serde_json::from_value(exec_resp.state)?;
        Ok(Entity {
            aggregate_id: exec_resp.aggregate,
            revision: exec_resp.revision,
            state,
        })
    }
}
