//! Tests for the `#[upload_to]` proc-macro. The macro stamps
//! three accessors on the decorated function:
//! `__upload_to_<fn>_bucket()`, `__upload_to_<fn>_template()`,
//! `__upload_to_<fn>_params()`. These tests assert that the
//! accessors exist, return the expected values, and that the
//! runtime helpers (`resolve_upload_key`, `upload_location`,
//! `upload_to`) compose with the stamped metadata end-to-end.
//!
//! All tests run with the `storage` feature off — the
//! proc-macro itself doesn't need the runtime crate (the
//! user wires the helpers in at the call site).

use nestrs::upload_to;

// --- Single-placeholder template ----------------------------------------

#[upload_to("photos", "uploads/{user_id}/file.jpg")]
#[allow(unused_variables)]
fn upload_photo(user_id: &str) -> &'static str {
    "ok"
}

#[test]
fn upload_to_macro_stamps_bucket_accessor() {
    assert_eq!(__upload_to_upload_photo_bucket(), "photos");
}

#[test]
fn upload_to_macro_stamps_template_accessor() {
    assert_eq!(
        __upload_to_upload_photo_template(),
        "uploads/{user_id}/file.jpg"
    );
}

#[test]
fn upload_to_macro_stamps_params_accessor() {
    assert_eq!(__upload_to_upload_photo_params(), &["user_id"]);
}

#[test]
fn upload_to_macro_preserves_function_body() {
    // The decorator is a passthrough on the body — the
    // function still returns "ok".
    assert_eq!(upload_photo("42"), "ok");
}

// --- Multi-placeholder template -----------------------------------------

#[upload_to("uploads", "{kind}/{date}/{user_id}/{filename}.bin")]
#[allow(unused_variables)]
fn upload_blob(kind: &str, date: &str, user_id: &str, filename: &str) -> String {
    format!("{kind}/{date}/{user_id}/{filename}")
}

#[test]
fn upload_to_macro_extracts_all_params_in_order() {
    assert_eq!(
        __upload_to_upload_blob_params(),
        &["kind", "date", "user_id", "filename"]
    );
    assert_eq!(
        __upload_to_upload_blob_template(),
        "{kind}/{date}/{user_id}/{filename}.bin"
    );
    assert_eq!(__upload_to_upload_blob_bucket(), "uploads");
    assert_eq!(
        upload_blob("photos", "2026-09-05", "7", "hello"),
        "photos/2026-09-05/7/hello"
    );
}

// --- Template with no placeholders --------------------------------------

#[upload_to("static", "robots.txt")]
fn upload_static() -> &'static str {
    "static"
}
// Note: `upload_static` has no params, so no `#[allow]` is needed.

#[test]
fn upload_to_macro_with_no_placeholders_yields_empty_params() {
    assert_eq!(
        __upload_to_upload_static_params() as &[&str],
        &[] as &[&str]
    );
    assert_eq!(__upload_to_upload_static_template(), "robots.txt");
    assert_eq!(__upload_to_upload_static_bucket(), "static");
    assert_eq!(upload_static(), "static");
}

// --- Multiple decorated functions coexist -------------------------------

#[upload_to("a", "k1/{x}")]
#[allow(unused_variables)]
fn fn_one(x: &str) -> String {
    x.into()
}

#[upload_to("b", "k2/{y}/{z}")]
#[allow(unused_variables)]
fn fn_two(y: &str, z: &str) -> String {
    format!("{y}{z}")
}

#[test]
fn upload_to_macro_emits_per_fn_accessors() {
    assert_eq!(__upload_to_fn_one_bucket(), "a");
    assert_eq!(__upload_to_fn_one_template(), "k1/{x}");
    assert_eq!(__upload_to_fn_one_params(), &["x"]);

    assert_eq!(__upload_to_fn_two_bucket(), "b");
    assert_eq!(__upload_to_fn_two_template(), "k2/{y}/{z}");
    assert_eq!(__upload_to_fn_two_params(), &["y", "z"]);

    // Bodies are untouched and don't share a namespace.
    assert_eq!(fn_one("hi"), "hi");
    assert_eq!(fn_two("foo", "bar"), "foobar");
}

// --- Macro + runtime helpers compose ------------------------------------

// We don't actually need the `storage` feature to use the
// stamped accessors — the runtime helpers live in
// `nestrs_storage`. To keep this test dep-free, we hand-roll
// the equivalent of `resolve_upload_key` against the macro
// output. This proves the contract: the macro stamps what the
// runtime helper consumes.
//
// (The actual `nestrs_storage::resolve_upload_key` is
// exercised in `nestrs-storage/tests/upload_to.rs`.)

fn manual_resolve(template: &str, params: &[&str]) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    let mut pi = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'}' {
                end += 1;
            }
            out.push_str(params[pi]);
            pi += 1;
            i = end + 1;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

#[test]
fn upload_to_macro_params_compose_with_resolver() {
    // Stamp a key from the macro, then run the manual
    // resolver against the macro's template + params. The
    // output should be the templated key, ready for the
    // handler to call `upload_to(&storage, &key, body)`.
    let bucket = __upload_to_upload_blob_bucket();
    let template = __upload_to_upload_blob_template();
    let params = __upload_to_upload_blob_params();
    assert_eq!(params, &["kind", "date", "user_id", "filename"]);
    let values = ["photos", "2026-09-05", "7", "hello"];
    assert_eq!(params.len(), values.len());

    let key = manual_resolve(template, &values);
    let loc = format!("{bucket}/{key}");
    assert_eq!(loc, "uploads/photos/2026-09-05/7/hello.bin");
}
