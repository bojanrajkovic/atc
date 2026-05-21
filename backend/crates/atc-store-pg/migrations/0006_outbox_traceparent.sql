-- Add a W3C traceparent column to the outbox so the drain task can connect
-- broadcast spans back to the originating webhook.handler trace via an OTel
-- span link. The traceparent is captured Rust-side at INSERT time from the
-- current span's W3C trace context.
--
-- Column shape:
--   - VARCHAR(55) — the W3C `traceparent` format is exactly 55 ASCII chars:
--       2-byte version + "-" + 32-byte trace_id + "-" + 16-byte span_id + "-" + 2-byte flags
--     The CHECK constraint is intentionally loose (length only); the format
--     check is Rust-side because malformed values must be tolerated as
--     "no link", not rejected as a write failure.
--   - NULLABLE — pre-migration rows have no traceparent. Inserts under an
--     OTel-disabled deployment (`OTEL_EXPORTER_OTLP_ENDPOINT` unset → no-op
--     meter and span exporter) also write NULL because `Span::current()`
--     returns no valid SpanContext.
--
-- No index needed: the column is only read row-at-a-time by the drain
-- pagination query, never used as a filter.

ALTER TABLE outbox
    ADD COLUMN traceparent VARCHAR(55) NULL;
