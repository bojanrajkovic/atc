//! Verifies that `otel::shutdown` flushes buffered spans and metrics before the
//! providers are torn down.
//!
//! The test stands up LOCAL `SdkTracerProvider` and `SdkMeterProvider` rather
//! than reusing `OtelTestHarness`. Both SDK providers are `Clone` over
//! `Arc<Inner>` and share an `is_shutdown` atomic with every other clone — the
//! globally-installed clones registered via `set_tracer_provider` /
//! `set_meter_provider` included. Calling `.shutdown()` on the harness's
//! providers would silently turn every subsequent span/metric emission in the
//! integration binary into a noop, poisoning unrelated serial tests. The local
//! providers here are not registered globally, so the shutdown is contained.
//!
//! The local providers also use `BatchSpanProcessor` + `PeriodicReader` rather
//! than the harness's synchronous `SimpleSpanProcessor`. Buffering processors
//! are what make the assertion meaningful: a regression that turned
//! `otel::shutdown` into a noop would leave the spans/metrics stranded in the
//! buffers, and the test would fail to find them in the in-memory exporters.
//!
//! `InMemorySpanExporter` clears its buffer on shutdown by default and the
//! escape hatch (`keep_records_on_shutdown`) is `pub(crate)` in the 0.31 SDK,
//! so the span assertion uses a small local `RetainingSpanExporter` that keeps
//! the buffer intact across shutdown. The metric exporter is keep-on-shutdown
//! out of the box, so the metric assertion uses `InMemoryMetricExporter`
//! directly.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use opentelemetry::KeyValue;
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry::trace::{Span as _, Tracer as _, TracerProvider as _};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{
    InMemoryMetricExporter, InMemoryMetricExporterBuilder, PeriodicReader, SdkMeterProvider,
    Temporality,
};
use opentelemetry_sdk::trace::{BatchSpanProcessor, SdkTracerProvider, SpanData, SpanExporter};
use serial_test::serial;

use atc_server::otel::{self, OtelHandles};

/// Span exporter that appends to a shared buffer and never clears it,
/// independent of shutdown. Mirrors the persistent semantics of
/// `InMemoryMetricExporter` so this test can read spans after `otel::shutdown`
/// without losing them to the `InMemorySpanExporter` shutdown reset.
#[derive(Clone, Default, Debug)]
struct RetainingSpanExporter {
    spans: Arc<Mutex<Vec<SpanData>>>,
}

impl RetainingSpanExporter {
    fn collected(&self) -> Vec<SpanData> {
        self.spans
            .lock()
            .expect("RetainingSpanExporter mutex")
            .clone()
    }
}

impl SpanExporter for RetainingSpanExporter {
    async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
        self.spans
            .lock()
            .expect("RetainingSpanExporter mutex")
            .extend(batch);
        Ok(())
    }
}

/// Spans emitted via the tracer captured in `OtelHandles` should reach the
/// in-memory exporter once `otel::shutdown` flushes the batch processor.
#[tokio::test]
#[serial]
async fn shutdown_flushes_buffered_span_through_provider() {
    let span_exporter = RetainingSpanExporter::default();
    let processor = BatchSpanProcessor::builder(span_exporter.clone()).build();
    let tracer_provider = SdkTracerProvider::builder()
        .with_span_processor(processor)
        .build();

    let metric_exporter = build_metric_exporter();
    let reader = PeriodicReader::builder(metric_exporter.clone()).build();
    let meter_provider = SdkMeterProvider::builder().with_reader(reader).build();

    let tracer = tracer_provider.tracer("atc-shutdown-flush-test");
    let mut span = tracer.start("test.span");
    span.end();

    let handles = OtelHandles {
        tracer_provider,
        meter_provider,
        tracer,
    };

    otel::shutdown(handles);

    let spans = span_exporter.collected();
    assert!(
        spans.iter().any(|s| s.name == "test.span"),
        "expected `test.span` to survive shutdown flush; got {:?}",
        spans.iter().map(|s| s.name.as_ref()).collect::<Vec<_>>()
    );
}

/// Metric instruments registered against the local meter should appear in the
/// in-memory exporter after `otel::shutdown` triggers the periodic reader's
/// final collection cycle.
#[tokio::test]
#[serial]
async fn shutdown_flushes_buffered_metric_through_provider() {
    let span_exporter = RetainingSpanExporter::default();
    let processor = BatchSpanProcessor::builder(span_exporter).build();
    let tracer_provider = SdkTracerProvider::builder()
        .with_span_processor(processor)
        .build();

    let metric_exporter = build_metric_exporter();
    let reader = PeriodicReader::builder(metric_exporter.clone()).build();
    let meter_provider = SdkMeterProvider::builder().with_reader(reader).build();

    let meter = meter_provider.meter("atc-shutdown-flush-test");
    let counter = meter.u64_counter("test.shutdown_counter").build();
    counter.add(7, &[KeyValue::new("source", "shutdown_flush_test")]);

    let tracer = tracer_provider.tracer("atc-shutdown-flush-test");

    let handles = OtelHandles {
        tracer_provider,
        meter_provider,
        tracer,
    };

    otel::shutdown(handles);

    let batches = metric_exporter
        .get_finished_metrics()
        .expect("get_finished_metrics");
    let observed = sum_counter(&batches, "test.shutdown_counter");
    assert_eq!(
        observed, 7,
        "expected counter total of 7 to flush during shutdown; saw {observed}"
    );
}

/// `otel::shutdown` returns within a small bound even when no spans or
/// metrics are emitted — covers the path where the orchestration calls
/// shutdown on an idle pipeline (e.g., immediately after startup).
#[tokio::test]
#[serial]
async fn shutdown_with_idle_handles_completes() {
    let span_exporter = RetainingSpanExporter::default();
    let processor = BatchSpanProcessor::builder(span_exporter).build();
    let tracer_provider = SdkTracerProvider::builder()
        .with_span_processor(processor)
        .build();

    let metric_exporter = build_metric_exporter();
    let reader = PeriodicReader::builder(metric_exporter).build();
    let meter_provider = SdkMeterProvider::builder().with_reader(reader).build();

    let tracer = tracer_provider.tracer("atc-shutdown-flush-test");

    let handles = OtelHandles {
        tracer_provider,
        meter_provider,
        tracer,
    };

    let result = tokio::time::timeout(Duration::from_secs(5), async {
        otel::shutdown(handles);
    })
    .await;

    assert!(result.is_ok(), "otel::shutdown should not block past 5s");
}

fn build_metric_exporter() -> InMemoryMetricExporter {
    InMemoryMetricExporterBuilder::new()
        .with_temporality(Temporality::Delta)
        .build()
}

fn sum_counter(batches: &[opentelemetry_sdk::metrics::data::ResourceMetrics], name: &str) -> u64 {
    let mut total: u64 = 0;
    for batch in batches {
        for scope in batch.scope_metrics() {
            for metric in scope.metrics() {
                if metric.name() != name {
                    continue;
                }
                if let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() {
                    for dp in sum.data_points() {
                        total = total.saturating_add(dp.value());
                    }
                }
            }
        }
    }
    total
}
