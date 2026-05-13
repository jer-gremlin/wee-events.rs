//! Guarantee (implicit, via type names): an error wrapped in
//! `EncodeError` originated during encoding.
//! Reality (crates/wee-events/src/memory_store.rs:35-39): every
//! `serde_json::Error` lands in `EncodeError::Json`, even decodes.

use serde::Deserialize;
use wee_events::{EncodeError, memory::MemoryStoreError};

#[derive(Debug, Deserialize)]
struct Thing {
    _x: u32,
}

fn main() {
    let err: serde_json::Error = serde_json::from_slice::<Thing>(
        b"not fucking json JASON, nor should it be coercable into a Thing{_x:$this} ",
    )
    .unwrap_err(); // a call to .unwrap(), to be clear DOES expose this, because serde_json is doing things properly, the 'issue' is below: 
    let store_err: MemoryStoreError = err.into(); // this feels like unintended behaviour.
    // dbg!(&store_err);
    /* What you get:
    [crates/sins/src/bin/memory_store_decode_labelled_encode.rs:20:5] &store_err = Codec(
    Json(
        Error("expected ident", line: 1, column: 2),
        ),
    )
    */
    // Is this what you want?

    // SUGGESTION:
    let MemoryStoreError::Codec(EncodeError::Json(_)) = store_err else {
        return; // would have been the honest path
    };
    panic!(
        "decode failure routed through `EncodeError::Json` — variant name lies about provenance"
    );
}
