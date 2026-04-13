use restate_sdk::prelude::*;

use std::sync::Arc;

use wee_events::{memory::MemoryStore, EventStore, PublishOptions, Renderer};
use wee_events_restate::{EntityResponse, ExecuteRequest};

use crate::domain::{Counter, CounterDispatcher};
use crate::random::{RandomServiceClient, SeededRandom};

/// Shared state for the counter Restate handlers.
pub struct CounterComponents {
    pub dispatcher: CounterDispatcher,
    pub store: Arc<MemoryStore>,
    pub renderer: Renderer<Counter>,
}

// ---------------------------------------------------------------------------
// Restate service: Counter commands
//
// Orchestrates command execution using the Restate context:
// 1. Fetches a random seed durably via the RandomService
// 2. Loads entity state
// 3. Dispatches the command with the seeded context
// 4. Publishes resulting events
// ---------------------------------------------------------------------------

#[restate_sdk::service]
#[name = "counter-executor"]
pub trait CounterExecutor {
    async fn run(request: Json<ExecuteRequest>) -> Result<Json<EntityResponse>, HandlerError>;
}

pub struct CounterCommands {
    pub components: CounterComponents,
}

impl CounterExecutor for CounterCommands {
    async fn run(
        &self,
        ctx: Context<'_>,
        request: Json<ExecuteRequest>,
    ) -> Result<Json<EntityResponse>, HandlerError> {
        let req = request.into_inner();

        // 1. Fetch random seed durably — on replay, Restate returns the
        //    journaled value so the handler produces the same result.
        let seed = ctx
            .service_client::<RandomServiceClient>()
            .seed()
            .call()
            .await?
            .into_inner();

        let handler_ctx = SeededRandom::new(seed);

        // 2. Load current entity state
        let aggregate = self
            .components
            .store
            .load(&req.command.target)
            .await
            .map_err(|e| TerminalError::new(e.to_string()))?;
        let entity = self
            .components
            .renderer
            .render(&aggregate)
            .map_err(|e| TerminalError::new(e.to_string()))?;

        // 3. Dispatch command — deterministic given the seeded context
        let events = self
            .components
            .dispatcher
            .dispatch(&handler_ctx, &entity, &req.command.name, req.command.command)
            .await
            .map_err(|r| {
                let payload = serde_json::json!({
                    "code": r.code,
                    "message": r.message,
                    "context": r.context,
                });
                TerminalError::new(payload.to_string())
            })?;

        if events.is_empty() {
            return Ok(Json(EntityResponse {
                aggregate: entity.aggregate_id,
                revision: entity.revision,
                state: serde_json::to_value(&entity.state)
                    .map_err(|e| TerminalError::new(e.to_string()))?,
            }));
        }

        // 4. Publish events with optimistic concurrency
        let options = PublishOptions {
            expected_revision: Some(entity.revision.clone()),
            ..Default::default()
        };
        self.components
            .store
            .publish(&req.command.target, options, events)
            .await
            .map_err(|e| TerminalError::new(e.to_string()))?;

        // 5. Reload and return updated state
        let updated = self
            .components
            .store
            .load(&req.command.target)
            .await
            .map_err(|e| TerminalError::new(e.to_string()))?;
        let updated = self
            .components
            .renderer
            .render(&updated)
            .map_err(|e| TerminalError::new(e.to_string()))?;

        Ok(Json(EntityResponse {
            aggregate: updated.aggregate_id,
            revision: updated.revision,
            state: serde_json::to_value(&updated.state)
                .map_err(|e| TerminalError::new(e.to_string()))?,
        }))
    }
}

// ---------------------------------------------------------------------------
// Restate service: Counter queries
// ---------------------------------------------------------------------------

#[restate_sdk::service]
#[name = "counter-loader"]
pub trait CounterLoader {
    async fn load(
        target: Json<wee_events::AggregateId>,
    ) -> Result<Json<EntityResponse>, HandlerError>;
}

pub struct CounterQueries {
    pub components: CounterComponents,
}

impl CounterLoader for CounterQueries {
    async fn load(
        &self,
        _ctx: Context<'_>,
        target: Json<wee_events::AggregateId>,
    ) -> Result<Json<EntityResponse>, HandlerError> {
        let target = target.into_inner();
        let aggregate = self
            .components
            .store
            .load(&target)
            .await
            .map_err(|e| TerminalError::new(e.to_string()))?;
        let entity = self
            .components
            .renderer
            .render(&aggregate)
            .map_err(|e| TerminalError::new(e.to_string()))?;

        Ok(Json(EntityResponse {
            aggregate: entity.aggregate_id,
            revision: entity.revision,
            state: serde_json::to_value(&entity.state)
                .map_err(|e| TerminalError::new(e.to_string()))?,
        }))
    }
}
