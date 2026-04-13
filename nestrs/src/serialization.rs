//! JSON helpers for Nest-style response shaping (optional null stripping, etc.).

/// Recursively removes JSON `null` values from objects and arrays (shallow keys only at each level).
pub fn strip_null_json_value(mut v: serde_json::Value) -> serde_json::Value {
    match &mut v {
        serde_json::Value::Object(map) => {
            map.retain(|_, val| !val.is_null());
            for (_, val) in map.iter_mut() {
                *val = strip_null_json_value(std::mem::take(val));
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                *item = strip_null_json_value(std::mem::take(item));
            }
        }
        _ => {}
    }
    v
}
