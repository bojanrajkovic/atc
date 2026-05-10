use std::env;

use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{MetricExporter, Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::{
    Aggregation, InstrumentKind, PeriodicReader, SdkMeterProvider, Stream,
};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{
    BatchSpanProcessor, Sampler, SdkTracer, SdkTracerProvider, ShouldSample,
};
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};

use crate::config::Config;

const DEFAULT_SERVICE_NAME: &str = "atc";
const HISTOGRAM_MAX_SIZE: u32 = 160;
const HISTOGRAM_MAX_SCALE: i8 = 20;
const TRACER_SCOPE_NAME: &str = "atc";

pub struct OtelHandles {
    pub tracer_provider: SdkTracerProvider,
    pub meter_provider: SdkMeterProvider,
    pub tracer: SdkTracer,
}

pub fn init_otel(_cfg: &Config) -> Option<OtelHandles> {
    let endpoint = match env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        Ok(value) if !value.is_empty() => value,
        _ => return None,
    };

    let resource = build_resource();

    let tracer_provider = match build_tracer_provider(&endpoint, resource.clone()) {
        Ok(provider) => provider,
        Err(err) => {
            tracing::error!(%err, "failed to build OTel tracer provider; OTel will be disabled");
            return None;
        }
    };

    let meter_provider = match build_meter_provider(&endpoint, resource) {
        Ok(provider) => provider,
        Err(err) => {
            tracing::error!(%err, "failed to build OTel meter provider; OTel will be disabled");
            let _ = tracer_provider.shutdown();
            return None;
        }
    };

    let tracer = tracer_provider.tracer(TRACER_SCOPE_NAME);

    opentelemetry::global::set_tracer_provider(tracer_provider.clone());
    opentelemetry::global::set_meter_provider(meter_provider.clone());
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    Some(OtelHandles {
        tracer_provider,
        meter_provider,
        tracer,
    })
}

fn build_resource() -> Resource {
    let mut attrs = vec![KeyValue::new(SERVICE_VERSION, env!("CARGO_PKG_VERSION"))];

    if env::var_os("OTEL_SERVICE_NAME").is_none() {
        attrs.push(KeyValue::new(SERVICE_NAME, DEFAULT_SERVICE_NAME));
    }

    let git_sha = env!("VERGEN_GIT_SHA");
    if !git_sha.is_empty() {
        attrs.push(KeyValue::new("atc.git_sha", git_sha));
    }

    Resource::builder().with_attributes(attrs).build()
}

fn build_tracer_provider(
    endpoint: &str,
    resource: Resource,
) -> Result<SdkTracerProvider, Box<dyn std::error::Error>> {
    let exporter = SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .with_endpoint(endpoint)
        .build()?;

    let processor = BatchSpanProcessor::builder(exporter).build();

    Ok(SdkTracerProvider::builder()
        .with_resource(resource)
        .with_sampler(load_sampler_from_env())
        .with_span_processor(processor)
        .build())
}

fn build_meter_provider(
    endpoint: &str,
    resource: Resource,
) -> Result<SdkMeterProvider, Box<dyn std::error::Error>> {
    let exporter = MetricExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .with_endpoint(endpoint)
        .build()?;

    let reader = PeriodicReader::builder(exporter).build();

    let view = |inst: &opentelemetry_sdk::metrics::Instrument| -> Option<Stream> {
        if matches!(inst.kind(), InstrumentKind::Histogram) {
            Stream::builder()
                .with_aggregation(Aggregation::Base2ExponentialHistogram {
                    max_size: HISTOGRAM_MAX_SIZE,
                    max_scale: HISTOGRAM_MAX_SCALE,
                    record_min_max: true,
                })
                .build()
                .ok()
        } else {
            None
        }
    };

    Ok(SdkMeterProvider::builder()
        .with_resource(resource)
        .with_reader(reader)
        .with_view(view)
        .build())
}

fn load_sampler_from_env() -> Sampler {
    let raw = match env::var("OTEL_TRACES_SAMPLER") {
        Ok(value) if !value.is_empty() => value,
        _ => return default_sampler(),
    };

    let arg = env::var("OTEL_TRACES_SAMPLER_ARG").ok();
    let normalized = raw.to_ascii_lowercase();

    match normalized.as_str() {
        "always_on" => Sampler::AlwaysOn,
        "always_off" => Sampler::AlwaysOff,
        "traceidratio" => match parse_ratio(arg.as_deref()) {
            Some(ratio) => Sampler::TraceIdRatioBased(ratio),
            None => default_sampler(),
        },
        "parentbased_always_on" => default_sampler(),
        "parentbased_always_off" => Sampler::ParentBased(box_sampler(Sampler::AlwaysOff)),
        "parentbased_traceidratio" => match parse_ratio(arg.as_deref()) {
            Some(ratio) => Sampler::ParentBased(box_sampler(Sampler::TraceIdRatioBased(ratio))),
            None => default_sampler(),
        },
        other => {
            tracing::warn!(
                sampler = other,
                "unknown OTEL_TRACES_SAMPLER value; falling back to parentbased_always_on"
            );
            default_sampler()
        }
    }
}

fn default_sampler() -> Sampler {
    Sampler::ParentBased(box_sampler(Sampler::AlwaysOn))
}

fn box_sampler<S: ShouldSample + 'static>(sampler: S) -> Box<dyn ShouldSample> {
    Box::new(sampler)
}

fn parse_ratio(raw: Option<&str>) -> Option<f64> {
    let raw = raw?;
    match raw.parse::<f64>() {
        Ok(value) if (0.0..=1.0).contains(&value) => Some(value),
        Ok(value) => {
            tracing::warn!(
                value,
                "OTEL_TRACES_SAMPLER_ARG out of [0, 1]; falling back to parentbased_always_on"
            );
            None
        }
        Err(err) => {
            tracing::warn!(
                %err,
                raw,
                "OTEL_TRACES_SAMPLER_ARG could not be parsed as f64; falling back to parentbased_always_on"
            );
            None
        }
    }
}
