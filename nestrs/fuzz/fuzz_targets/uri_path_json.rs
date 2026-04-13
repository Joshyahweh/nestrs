#![no_main]

//! Fuzz **URI** construction / parse (path-param-shaped URLs) and arbitrary **JSON** bytes.
//! Complements HTTP routing benches; catches panics in `http::Uri` + `serde_json` on hostile input.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let lossy = String::from_utf8_lossy(data);
    let compact: String = lossy
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect();
    let path = format!("/v1/bench/items/{compact}");
    let _ = path.parse::<http::Uri>();

    let _ = serde_json::from_slice::<serde_json::Value>(data);
});
