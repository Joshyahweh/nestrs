//! `#[upload_to]` decorator + `upload_to` helper.
//!
//! The proc-macro in `nestrs-macros` (`#[upload_to("bucket",
//! "prefix/{id}")]`) is a thin stamp: it emits three accessors
//! on the function — `__upload_to_<fn>_bucket`, `_template`,
//! `_params`. This module provides the runtime helpers the
//! handler calls to actually perform the upload and resolve the
//! templated key.
//!
//! The two-step shape keeps the proc-macro honest: parsing
//! `{placeholders}` and substituting them in happens at
//! runtime, so the user can debug the template behaviour
//! without fighting a compile error.

use bytes::Bytes;

use crate::storage::{Storage, StorageError};

/// Stream `data` to `storage` under the given `key`. Used
/// directly by handlers that have already resolved the key, or
/// as the final step of the `#[upload_to]` flow once
/// `resolve_upload_key` has substituted the template.
pub async fn upload_to<S: Storage + ?Sized>(
    storage: &S,
    key: &str,
    data: Bytes,
) -> Result<(), StorageError> {
    storage.put(key, data).await
}

/// Resolve a `#[upload_to]` key template by substituting
/// `{placeholder}` segments with positional values from `params`.
///
/// `template` is the literal template string
/// (e.g. `"uploads/{user_id}/{filename}"`); `params` is the
/// ordered list of values to substitute. The number of
/// `{name}` placeholders in the template must equal
/// `params.len()` and (when the macro stamps it) the names must
/// match what `__upload_to_<fn>_params()` returned.
///
/// Returns `Err(StorageError::BadRequest)` when the placeholder
/// count doesn't match, so the caller can surface a 400 instead
/// of producing a malformed key.
pub fn resolve_upload_key(template: &str, params: &[&str]) -> Result<String, StorageError> {
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
            if end >= bytes.len() {
                return Err(StorageError::BadRequest(format!(
                    "upload_to template `{template}` has unterminated `{{`"
                )));
            }
            if pi >= params.len() {
                return Err(StorageError::BadRequest(format!(
                    "upload_to template `{template}` expects more placeholders than provided"
                )));
            }
            out.push_str(params[pi]);
            pi += 1;
            i = end + 1;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    if pi != params.len() {
        return Err(StorageError::BadRequest(format!(
            "upload_to template `{template}` uses {pi} placeholders, but {} value(s) provided",
            params.len()
        )));
    }
    Ok(out)
}

/// Compute the absolute storage key (bucket + key) for a
/// `#[upload_to]` upload. The bucket comes from the decorator
/// attribute; the key is the resolved template. The
/// concatenation uses `'/'` as the separator; callers that need
/// a different shape can build their own.
pub fn upload_location(bucket: &str, key: &str) -> String {
    let bucket = bucket.trim_end_matches('/');
    let key = key.trim_start_matches('/');
    if bucket.is_empty() {
        key.to_string()
    } else {
        format!("{bucket}/{key}")
    }
}
