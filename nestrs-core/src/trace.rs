//! W3C Trace Context propagation ([`traceparent`](https://www.w3.org/TR/trace-context/))
//! as an ambient per-task slot.
//!
//! The HTTP middleware in `nestrs` parses the `traceparent` header and
//! installs a [`TraceContext`] for the duration of the request, so handlers,
//! guards, resolvers and tool bodies can read it via
//! [`current_trace_context`] (or the convenience accessors on
//! `nestrs::core::ExecutionContext`). Non-HTTP transports (WS handshake,
//! message consumers, MCP hosts) call [`with_trace_context`] directly.
//!
//! This is intentionally independent of the `otel` feature: it parses the
//! header per the W3C spec and exposes plain hex strings, no SDK types.

/// Parsed W3C trace context (`traceparent` + optional `tracestate`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceContext {
    /// 16-byte trace id (W3C: non-zero).
    pub trace_id: [u8; 16],
    /// 8-byte parent/self span id (W3C: non-zero).
    pub span_id: [u8; 8],
    /// Trace flags byte (`sampled` is bit 0).
    pub trace_flags: u8,
    /// Raw `tracestate` header value, when present.
    pub tracestate: Option<String>,
}

impl TraceContext {
    /// 32-character lowercase hex trace id.
    pub fn trace_id_hex(&self) -> String {
        hex_encode(&self.trace_id)
    }

    /// 16-character lowercase hex span id.
    pub fn span_id_hex(&self) -> String {
        hex_encode(&self.span_id)
    }

    /// 2-character lowercase hex trace flags.
    pub fn trace_flags_hex(&self) -> String {
        format!("{:02x}", self.trace_flags)
    }

    /// `true` when the `sampled` flag (bit 0) is set.
    pub fn sampled(&self) -> bool {
        self.trace_flags & 0x01 != 0
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks(2) {
        out.push(hex_value(pair[0])? * 16 + hex_value(pair[1])?);
    }
    Some(out)
}

/// Parse a `traceparent` header value per the W3C Trace Context spec.
///
/// Strict validation: `VERSION-TRACEID-SPANID-FLAGS` with 2-hex version
/// (`ff` invalid), 32-hex non-zero trace id, 16-hex non-zero span id and
/// 2-hex flags (`ff` invalid). Case-insensitive on input; the returned
/// hex accessors always render lowercase.
pub fn parse_traceparent(value: &str) -> Option<TraceContext> {
    let value = value.trim();
    let mut parts = value.split('-');
    let version_hex = parts.next()?;
    let trace_id_hex = parts.next()?;
    let span_id_hex = parts.next()?;
    let flags_hex = parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    // Version: exactly 2 hex digits, `ff` forbidden.
    if version_hex.len() != 2 || !version_hex.bytes().all(|b| hex_value(b).is_some()) {
        return None;
    }
    let version = hex_bytes(version_hex)?[0];
    if version == 0xff {
        return None;
    }

    // Trace id: exactly 32 hex digits, not all zero.
    let mut trace_id = [0u8; 16];
    let trace_bytes = hex_bytes(trace_id_hex)?;
    if trace_bytes.len() != 16 || trace_bytes.iter().all(|&b| b == 0) {
        return None;
    }
    trace_id.copy_from_slice(&trace_bytes);

    // Span id: exactly 16 hex digits, not all zero.
    let mut span_id = [0u8; 8];
    let span_bytes = hex_bytes(span_id_hex)?;
    if span_bytes.len() != 8 || span_bytes.iter().all(|&b| b == 0) {
        return None;
    }
    span_id.copy_from_slice(&span_bytes);

    // Flags: exactly 2 hex digits, `ff` forbidden.
    if flags_hex.len() != 2 || !flags_hex.bytes().all(|b| hex_value(b).is_some()) {
        return None;
    }
    let trace_flags = hex_bytes(flags_hex)?[0];
    if trace_flags == 0xff {
        return None;
    }

    Some(TraceContext {
        trace_id,
        span_id,
        trace_flags,
        tracestate: None,
    })
}

tokio::task_local! {
    static TRACE_SLOT: std::cell::RefCell<Option<TraceContext>>;
}

/// Run `future` with the given trace context installed in the per-task
/// trace slot. Non-HTTP transports (WS gateways, message consumers, MCP
/// hosts) call this directly; the HTTP middleware does it per request.
pub async fn with_trace_context<F, T>(ctx: TraceContext, future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    TRACE_SLOT
        .scope(std::cell::RefCell::new(Some(ctx)), future)
        .await
}

/// Read the ambient trace context for the current task, if one was
/// installed via [`with_trace_context`] (or by the HTTP middleware).
pub fn current_trace_context() -> Option<TraceContext> {
    TRACE_SLOT.try_with(|c| c.borrow().clone()).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

    #[test]
    fn parse_valid_version_00() {
        let ctx = parse_traceparent(VALID).expect("valid traceparent");
        assert_eq!(ctx.trace_id_hex(), "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(ctx.span_id_hex(), "00f067aa0ba902b7");
        assert_eq!(ctx.trace_flags_hex(), "01");
        assert!(ctx.sampled());
    }

    #[test]
    fn parse_accepts_uppercase_and_strips_whitespace() {
        let ctx = parse_traceparent(" 00-4BF92F3577B34DA6A3CE929D0E0E4736-00F067AA0BA902B7-00 ")
            .expect("uppercase ok");
        assert_eq!(ctx.trace_id_hex(), "4bf92f3577b34da6a3ce929d0e0e4736");
        assert!(!ctx.sampled());
    }

    #[test]
    fn rejects_invalid_inputs() {
        // Version ff is invalid; so is a non-hex / wrong-length version.
        assert!(
            parse_traceparent("ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01").is_none()
        );
        assert!(
            parse_traceparent("0-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01").is_none()
        );
        // Zero trace id is invalid.
        assert!(
            parse_traceparent("00-00000000000000000000000000000000-00f067aa0ba902b7-01").is_none()
        );
        // Wrong-length trace id / span id.
        assert!(
            parse_traceparent("00-4bf92f3577b34da6a3ce929d0e0e473-00f067aa0ba902b7-01").is_none()
        );
        assert!(
            parse_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b-01").is_none()
        );
        // Flags ff is invalid; non-hex flags too.
        assert!(
            parse_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-ff").is_none()
        );
        assert!(
            parse_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-xy").is_none()
        );
        // Extra segment / too few segments / empty.
        assert!(
            parse_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra")
                .is_none()
        );
        assert!(
            parse_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7").is_none()
        );
        assert!(parse_traceparent("").is_none());
        // Zero span id is invalid.
        assert!(
            parse_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01").is_none()
        );
    }

    #[tokio::test]
    async fn slot_scopes_and_restores() {
        let ctx = parse_traceparent(VALID).expect("valid");
        let outside = current_trace_context();
        assert!(outside.is_none());
        let inside = with_trace_context(ctx.clone(), async move {
            let seen = current_trace_context().expect("installed");
            assert_eq!(seen, ctx);
            true
        })
        .await;
        assert!(inside);
        assert!(current_trace_context().is_none());
    }
}
