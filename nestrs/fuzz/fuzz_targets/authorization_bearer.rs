#![no_main]

//! Fuzz [`nestrs::parse_authorization_bearer`] (header parsing boundary).

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    let _ = nestrs::parse_authorization_bearer(s.as_ref());
});
