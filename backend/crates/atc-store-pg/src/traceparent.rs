//! W3C `traceparent` capture + parse helpers for outbox causal-chain linking.
//!
//! The webhook write path inserts a row into the `outbox` table from within
//! the `webhook.handler` trace. The drain task reads that row later in a
//! different async context (its own per-tick `drain.pass` root span) and
//! broadcasts to WS clients. Without correlation, the two trees are
//! disconnected in Tempo — operators can't trace "webhook in → frame out".
//!
//! This module captures the W3C `traceparent` of the current span at INSERT
//! time and stores it as a column on the outbox row. At drain time the value
//! is parsed back into an OTel `SpanContext` and attached as a span LINK to
//! the `drain.broadcast` span. Link (not parent) because the drain's per-tick
//! root invariant is load-bearing — see
//! `docs/architecture/metrics.md` § "Task-lifetime root spans".
//!
//! Format reference: https://www.w3.org/TR/trace-context/#traceparent-header
//! Layout: `00-{32-hex-trace-id}-{16-hex-span-id}-{2-hex-flags}` (55 chars).

use opentelemetry::trace::{SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState};
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Return the W3C `traceparent` string for the current tracing span, if its
/// underlying OTel `SpanContext` is valid (i.e., the SDK is initialized AND
/// the span is part of a sampled trace).
///
/// Returns `None` under any of:
/// - The OTel SDK is uninstalled (no-op meter / tracer — `OTEL_EXPORTER_OTLP_ENDPOINT` unset)
/// - The current span has no parent and `parentbased_*` samplers chose not to sample
/// - The current span's SpanContext is otherwise invalid
///
/// Callers should treat `None` as "no link" — the outbox column is nullable
/// and the drain side tolerates absent traceparents.
#[must_use]
pub fn current() -> Option<String> {
    let span_ref = Span::current();
    let context = span_ref.context();
    let span_ctx = context.span().span_context().clone();
    if !span_ctx.is_valid() {
        return None;
    }
    // W3C traceparent: 00-<trace_id>-<span_id>-<flags>
    Some(format!(
        "00-{}-{}-{:02x}",
        span_ctx.trace_id(),
        span_ctx.span_id(),
        span_ctx.trace_flags().to_u8(),
    ))
}

/// Parse a W3C `traceparent` string into an OTel [`SpanContext`].
///
/// Returns `None` for any malformed input — the caller treats this as
/// "no link" rather than failing the broadcast.
#[must_use]
pub fn parse(traceparent: &str) -> Option<SpanContext> {
    // Strict W3C check: 4 dash-separated segments of lengths (2, 32, 16, 2).
    let mut parts = traceparent.split('-');
    let version = parts.next()?;
    let trace_id_hex = parts.next()?;
    let span_id_hex = parts.next()?;
    let flags_hex = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if version.len() != 2
        || trace_id_hex.len() != 32
        || span_id_hex.len() != 16
        || flags_hex.len() != 2
    {
        return None;
    }
    let trace_id = TraceId::from_hex(trace_id_hex).ok()?;
    let span_id = SpanId::from_hex(span_id_hex).ok()?;
    let flags_byte = u8::from_str_radix(flags_hex, 16).ok()?;
    let trace_flags = TraceFlags::new(flags_byte);
    // `is_remote = true` because this SpanContext was reconstructed from a
    // wire format (not derived from a live in-process span). Attaching it as
    // a Link encodes the cross-trace correlation; OTel SDK exporters surface
    // remote links specially.
    Some(SpanContext::new(
        trace_id,
        span_id,
        trace_flags,
        true,
        TraceState::default(),
    ))
}

/// Attach the parsed traceparent as an OTel span LINK on the given tracing
/// `Span`. No-op if the traceparent is `None` or malformed.
pub fn attach_link(span: &Span, traceparent: Option<&str>) {
    let Some(tp) = traceparent else { return };
    let Some(span_ctx) = parse(tp) else {
        tracing::debug!(
            traceparent = tp,
            "malformed outbox traceparent; skipping link"
        );
        return;
    };
    span.add_link(span_ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_round_trip() {
        let tp = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let ctx = parse(tp).expect("well-formed traceparent");
        assert_eq!(
            format!("{}", ctx.trace_id()),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
        assert_eq!(format!("{}", ctx.span_id()), "00f067aa0ba902b7");
        assert_eq!(ctx.trace_flags().to_u8(), 0x01);
        assert!(ctx.is_remote());
    }

    #[test]
    fn parse_rejects_malformed() {
        for bad in [
            "",
            "not-a-traceparent",
            "00-short-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-tooshort-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-0",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
        ] {
            assert!(parse(bad).is_none(), "should reject {bad:?}");
        }
    }

    #[test]
    fn current_returns_none_when_otel_disabled() {
        // No OTel SDK installed in unit tests: current() returns None because
        // the SpanContext is invalid (zero trace_id).
        assert!(current().is_none());
    }
}
