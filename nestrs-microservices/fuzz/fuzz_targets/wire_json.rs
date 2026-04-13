#![no_main]

//! Fuzz JSON decode of [`nestrs_microservices::wire`] types (same bytes transporters put on the wire
//! and gRPC carries inside protobuf). Targets panic-free serde paths; fix any crash as a bug.

use libfuzzer_sys::fuzz_target;
use nestrs_microservices::wire::{WireError, WireRequest, WireResponse};

fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<WireRequest>(data);
    let _ = serde_json::from_slice::<WireResponse>(data);
    let _ = serde_json::from_slice::<WireError>(data);
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = serde_json::from_str::<WireRequest>(s);
        let _ = serde_json::from_str::<WireResponse>(s);
        let _ = serde_json::from_str::<WireError>(s);
    }
});
