use std::collections::{HashMap, HashSet};

use crate::aggregate::Aggregate;
use crate::entity::Entity;
use crate::event::{DeserializeJsonError, EventData, RecordedEvent};
use crate::id::{AggregateId, EventId, EventType, Revision};

/// A reducer function pointer. Receives mutable state, the event type, and the
/// event data (encoding + bytes). Responsible for deserializing and applying.
pub type ReduceFn<S, E = DeserializeJsonError> =
    fn(&mut S, &EventType, &EventData) -> Result<(), E>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventPattern {
    Exact(EventType),
    Glob(String),
}

impl EventPattern {
    pub fn exact(event_type: impl Into<EventType>) -> Self {
        Self::Exact(event_type.into())
    }

    pub fn glob(pattern: impl Into<String>) -> Result<Self, EventPatternError> {
        let pattern = pattern.into();
        if pattern.is_empty() {
            return Err(EventPatternError::EmptyGlob);
        }
        Ok(Self::Glob(pattern))
    }

    fn matches(&self, event_type: &EventType) -> bool {
        match self {
            Self::Exact(pattern) => pattern == event_type,
            Self::Glob(pattern) => glob_matches(pattern, event_type.as_str()),
        }
    }
}

impl From<EventType> for EventPattern {
    fn from(event_type: EventType) -> Self {
        Self::Exact(event_type)
    }
}

impl From<&str> for EventPattern {
    fn from(event_type: &str) -> Self {
        Self::Exact(EventType::new(event_type))
    }
}

impl From<String> for EventPattern {
    fn from(event_type: String) -> Self {
        Self::Exact(EventType::new(event_type))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EventPatternError {
    #[error("glob event pattern cannot be empty")]
    EmptyGlob,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError<E> {
    #[error("unhandled event type {context}")]
    UnhandledEventType { context: Box<RenderEventContext> },
    #[error("failed to apply event {context}: {source}")]
    ApplyFailed {
        context: Box<RenderEventContext>,
        #[source]
        source: E,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderEventContext {
    pub aggregate_id: AggregateId,
    pub event_id: EventId,
    pub event_type: EventType,
    pub revision: Revision,
    pub encoding: String,
}

impl std::fmt::Display for RenderEventContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at revision {} in aggregate {}",
            self.event_type, self.revision, self.aggregate_id
        )
    }
}

impl RenderError<DeserializeJsonError> {
    /// Project this render error into any store error type.
    pub fn into_store_error<E>(self) -> E
    where
        E: From<crate::Error> + From<serde_json::Error>,
    {
        match self {
            Self::UnhandledEventType { context } => crate::Error::UnhandledEventType {
                event_type: context.event_type.to_string(),
            }
            .into(),
            Self::ApplyFailed { source, .. } => source.into_store_error(),
        }
    }
}

/// Stateless projection engine. Folds an aggregate's event stream through
/// registered reducers to produce an `Entity<S>`.
///
/// Entity rendering is strict: event types without a reducer fail rendering
/// unless explicitly ignored.
pub struct Renderer<S, E = DeserializeJsonError> {
    reducers: HashMap<EventType, ReduceFn<S, E>>,
    pattern_reducers: Vec<(EventPattern, ReduceFn<S, E>)>,
    ignored: HashSet<EventType>,
    ignored_patterns: Vec<EventPattern>,
}

impl<S: Default, E> Renderer<S, E> {
    pub fn new() -> Self {
        Self {
            reducers: HashMap::new(),
            pattern_reducers: Vec::new(),
            ignored: HashSet::new(),
            ignored_patterns: Vec::new(),
        }
    }

    /// Registers a reducer for a given event type or pattern.
    ///
    /// Plain strings are exact matches. Use [`EventPattern::glob`] for
    /// wildcard matching.
    pub fn with(mut self, pattern: impl Into<EventPattern>, reducer: ReduceFn<S, E>) -> Self {
        match pattern.into() {
            EventPattern::Exact(event_type) => {
                self.reducers.insert(event_type, reducer);
            }
            pattern => self.pattern_reducers.push((pattern, reducer)),
        }
        self
    }

    /// Registers a reducer for a given event type or pattern (mutating).
    pub fn register(&mut self, pattern: impl Into<EventPattern>, reducer: ReduceFn<S, E>) {
        match pattern.into() {
            EventPattern::Exact(event_type) => {
                self.reducers.insert(event_type, reducer);
            }
            pattern => self.pattern_reducers.push((pattern, reducer)),
        }
    }

    /// Explicitly ignores matching event types during rendering.
    ///
    /// Plain strings are exact matches. Use [`EventPattern::glob`] for
    /// wildcard matching.
    pub fn ignore(mut self, pattern: impl Into<EventPattern>) -> Self {
        match pattern.into() {
            EventPattern::Exact(event_type) => {
                self.ignored.insert(event_type);
            }
            pattern => self.ignored_patterns.push(pattern),
        }
        self
    }

    /// Explicitly ignores matching event types during rendering (mutating).
    pub fn register_ignore(&mut self, pattern: impl Into<EventPattern>) {
        match pattern.into() {
            EventPattern::Exact(event_type) => {
                self.ignored.insert(event_type);
            }
            pattern => self.ignored_patterns.push(pattern),
        }
    }

    /// Folds the aggregate's event stream into projected state.
    ///
    /// Unhandled event types fail rendering unless explicitly ignored.
    pub fn render(&self, aggregate: &Aggregate) -> Result<Entity<S>, RenderError<E>> {
        let mut state = S::default();

        for event in aggregate.events() {
            if let Some(reducer) = self.reducers.get(&event.event_type) {
                reducer(&mut state, &event.event_type, &event.data).map_err(|source| {
                    RenderError::ApplyFailed {
                        context: Box::new(RenderEventContext::new(aggregate, event.as_ref())),
                        source,
                    }
                })?;
                continue;
            }
            if self.ignored.contains(&event.event_type) {
                continue;
            }
            if let Some((_, reducer)) = self
                .pattern_reducers
                .iter()
                .find(|(pattern, _)| pattern.matches(&event.event_type))
            {
                reducer(&mut state, &event.event_type, &event.data).map_err(|source| {
                    RenderError::ApplyFailed {
                        context: Box::new(RenderEventContext::new(aggregate, event.as_ref())),
                        source,
                    }
                })?;
                continue;
            }
            if self
                .ignored_patterns
                .iter()
                .any(|pattern| pattern.matches(&event.event_type))
            {
                continue;
            }
            return Err(RenderError::UnhandledEventType {
                context: Box::new(RenderEventContext::new(aggregate, event.as_ref())),
            });
        }

        Ok(Entity {
            aggregate_id: aggregate.id.clone(),
            revision: aggregate.revision().clone(),
            state,
        })
    }
}

impl RenderEventContext {
    fn new(aggregate: &Aggregate, event: &RecordedEvent) -> Self {
        Self {
            aggregate_id: aggregate.id.clone(),
            event_id: event.event_id.clone(),
            event_type: event.event_type.clone(),
            revision: event.revision.clone(),
            encoding: event.data.encoding.to_string(),
        }
    }
}

fn glob_matches(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let (mut pattern_idx, mut text_idx) = (0, 0);
    let mut star_idx = None;
    let mut star_text_idx = 0;

    while text_idx < text.len() {
        if pattern_idx < pattern.len() && pattern[pattern_idx] == text[text_idx] {
            pattern_idx += 1;
            text_idx += 1;
        } else if pattern_idx < pattern.len() && pattern[pattern_idx] == b'*' {
            star_idx = Some(pattern_idx);
            pattern_idx += 1;
            star_text_idx = text_idx;
        } else if let Some(star) = star_idx {
            pattern_idx = star + 1;
            star_text_idx += 1;
            text_idx = star_text_idx;
        } else {
            return false;
        }
    }

    while pattern_idx < pattern.len() && pattern[pattern_idx] == b'*' {
        pattern_idx += 1;
    }

    pattern_idx == pattern.len()
}

impl<S: Default, E> Default for Renderer<S, E> {
    fn default() -> Self {
        Self::new()
    }
}
