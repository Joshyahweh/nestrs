use nestrs::prelude::*;
use serial_test::serial;

#[test]
#[serial(tracing_global)]
fn try_init_tracing_is_idempotent() {
    let first = try_init_tracing(TracingConfig::default());
    assert!(first.is_ok(), "{first:?}");
    let second = try_init_tracing(TracingConfig::builder().format(TracingFormat::Json));
    assert!(second.is_ok(), "{second:?}");
}
