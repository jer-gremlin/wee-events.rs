//! Property-based tests for the counter domain.
//!
//! These test the **functional core** — the pure handler functions that take
//! (context, state, command) and return events. No stores, no Restate, no I/O.
//!
//! Because the handlers are pure functions with injected context, we can:
//! - Control non-determinism via `SeededRandom`
//! - Generate arbitrary states, commands, and seeds
//! - Assert properties that must hold for ALL inputs

use proptest::prelude::*;
use wee_events::{AggregateId, Entity, Revision};

use restate_counter::domain::{self, Adjust, Counter, Decrement, Increment};
use restate_counter::random::{HasRandomSource, SeededRandom};

/// Build an entity with a given balance for testing.
fn entity(value: i64) -> Entity<Counter> {
    Entity {
        aggregate_id: AggregateId::new("counter", "test"),
        revision: Revision::zero(),
        state: Counter { value },
    }
}

/// Extract the "amount" field from the first event's JSON data.
fn event_amount(events: &[wee_events::RawEvent]) -> i64 {
    let data: serde_json::Value = events[0].data.deserialize_json().unwrap();
    data["amount"].as_i64().unwrap()
}

// ---------------------------------------------------------------------------
// Increment properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn increment_always_produces_one_event(
        balance in 0i64..10_000,
        amount in 1i64..1_000,
        seed in 0.0f64..1.0,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = SeededRandom::new(seed);
        let e = entity(balance);
        let cmd = Increment { amount };

        let events = rt.block_on(domain::increment(&ctx, &e, cmd)).unwrap();

        prop_assert_eq!(events.len(), 1);
        prop_assert_eq!(events[0].event_type.as_str(), "counter:incremented");
    }

    #[test]
    fn increment_total_is_at_least_requested(
        balance in 0i64..10_000,
        amount in 1i64..1_000,
        seed in 0.0f64..1.0,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = SeededRandom::new(seed);
        let e = entity(balance);
        let cmd = Increment { amount };

        let events = rt.block_on(domain::increment(&ctx, &e, cmd)).unwrap();
        let total = event_amount(&events);

        prop_assert!(total >= amount, "total {} < requested {}", total, amount);
    }

    #[test]
    fn increment_no_bonus_when_balance_is_zero(
        amount in 1i64..1_000,
        seed in 0.0f64..1.0,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = SeededRandom::new(seed);
        let e = entity(0);
        let cmd = Increment { amount };

        let events = rt.block_on(domain::increment(&ctx, &e, cmd)).unwrap();
        let total = event_amount(&events);

        prop_assert_eq!(total, amount, "expected no bonus at zero balance");
    }
}

// ---------------------------------------------------------------------------
// Decrement properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn decrement_rejected_when_balance_is_zero(
        amount in 1i64..1_000,
        seed in 0.0f64..1.0,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = SeededRandom::new(seed);
        let e = entity(0);
        let cmd = Decrement { amount };

        let result = rt.block_on(domain::decrement(&ctx, &e, cmd));

        prop_assert!(result.is_err());
        let rejection = result.unwrap_err();
        prop_assert_eq!(rejection.code, "INSUFFICIENT_VALUE");
    }

    #[test]
    fn decrement_never_exceeds_balance(
        balance in 1i64..10_000,
        amount in 1i64..20_000,
        seed in 0.0f64..1.0,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = SeededRandom::new(seed);
        let e = entity(balance);
        let cmd = Decrement { amount };

        let events = rt.block_on(domain::decrement(&ctx, &e, cmd)).unwrap();
        let effective = event_amount(&events);

        prop_assert!(
            effective <= balance,
            "effective {} > balance {}",
            effective,
            balance
        );
    }

    #[test]
    fn decrement_effective_is_at_least_one(
        balance in 1i64..10_000,
        amount in 1i64..20_000,
        seed in 0.0f64..1.0,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = SeededRandom::new(seed);
        let e = entity(balance);
        let cmd = Decrement { amount };

        let events = rt.block_on(domain::decrement(&ctx, &e, cmd)).unwrap();
        let effective = event_amount(&events);

        prop_assert!(effective >= 1, "effective {} < 1", effective);
    }

    #[test]
    fn decrement_always_produces_one_event_when_balance_positive(
        balance in 1i64..10_000,
        amount in 1i64..20_000,
        seed in 0.0f64..1.0,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = SeededRandom::new(seed);
        let e = entity(balance);
        let cmd = Decrement { amount };

        let events = rt.block_on(domain::decrement(&ctx, &e, cmd)).unwrap();

        prop_assert_eq!(events.len(), 1);
        prop_assert_eq!(events[0].event_type.as_str(), "counter:decremented");
    }
}

// ---------------------------------------------------------------------------
// Adjust properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn adjust_produces_at_most_one_event(
        balance in 0i64..10_000,
        seed in 0.0f64..1.0,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = SeededRandom::new(seed);
        let e = entity(balance);

        let events = rt.block_on(domain::adjust(&ctx, &e, Adjust))
            .unwrap();

        prop_assert!(events.len() <= 1, "got {} events", events.len());
    }

    #[test]
    fn adjust_increment_events_have_positive_amount(
        balance in 0i64..10_000,
        seed in 0.0f64..1.0,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = SeededRandom::new(seed);
        let e = entity(balance);

        let events = rt.block_on(domain::adjust(&ctx, &e, Adjust))
            .unwrap();

        for event in &events {
            if event.event_type.as_str() == "counter:incremented" {
                let amount = event_amount(std::slice::from_ref(event));
                prop_assert!(amount > 0, "increment amount {} <= 0", amount);
            }
        }
    }

    #[test]
    fn adjust_decrement_events_have_positive_amount(
        balance in 0i64..10_000,
        seed in 0.0f64..1.0,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ctx = SeededRandom::new(seed);
        let e = entity(balance);

        let events = rt.block_on(domain::adjust(&ctx, &e, Adjust))
            .unwrap();

        for event in &events {
            if event.event_type.as_str() == "counter:decremented" {
                let amount = event_amount(std::slice::from_ref(event));
                prop_assert!(amount > 0, "decrement amount {} <= 0", amount);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SeededRandom properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn seeded_random_stays_in_range(
        seed in 0.0f64..1.0,
        min in -1000i64..0,
        max in 1i64..1000,
    ) {
        let ctx = SeededRandom::new(seed);
        let value = ctx.random_in_range(min, max);

        prop_assert!(value >= min, "value {} < min {}", value, min);
        prop_assert!(value <= max, "value {} > max {}", value, max);
    }

    #[test]
    fn seeded_random_deterministic(
        seed in 0.0f64..1.0,
        min in -1000i64..0,
        max in 1i64..1000,
    ) {
        let a = SeededRandom::new(seed);
        let b = SeededRandom::new(seed);

        prop_assert_eq!(
            a.random_in_range(min, max),
            b.random_in_range(min, max),
            "same seed must produce same result"
        );
    }
}
