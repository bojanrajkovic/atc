use std::env;

use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{MetricExporter, Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::{
    Aggregation, Instrument, InstrumentKind, PeriodicReader, SdkMeterProvider, Stream,
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
const METER_SCOPE_NAME: &str = "atc";

pub struct OtelHandles {
    pub tracer_provider: SdkTracerProvider,
    pub meter_provider: SdkMeterProvider,
    pub tracer: SdkTracer,
}

/// Returns true when `OTEL_EXPORTER_OTLP_ENDPOINT` is set to a non-empty,
/// parseable URL with an explicit scheme and host.
///
/// This is the gate `init_otel` uses to decide whether to install the SDK; it
/// is exposed separately so tests can verify the gate without triggering the
/// process-global side effects of `init_otel` (provider registration, recorder
/// install) that would bleed into other tests in the same integration binary.
///
/// Validation is deliberate. The OTel SDK's HTTP exporter env-resolution
/// silently swallows `Uri` parse errors and falls back to
/// `http://localhost:4318/v1/*` (see opentelemetry-otlp 0.31
/// `resolve_http_endpoint`). Without this guard, a typo like
/// `htttp://collector:4318` or a missing-scheme value like `collector:4318`
/// would silently route production telemetry to localhost and lose it.
/// Parsing here means a bad endpoint disables OTel with a clear stderr
/// warning instead.
pub fn endpoint_configured() -> bool {
    let raw = match env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        Ok(v) if !v.is_empty() => v,
        _ => return false,
    };
    match raw.parse::<http::Uri>() {
        Ok(uri) if uri.scheme().is_some() && uri.host().is_some() => true,
        Ok(uri) => {
            eprintln!(
                "atc-server: OTEL_EXPORTER_OTLP_ENDPOINT={raw:?} parsed but is missing scheme or host (scheme={:?}, host={:?}); disabling OTel — the SDK would otherwise silently route to http://localhost:4318",
                uri.scheme_str(),
                uri.host()
            );
            false
        }
        Err(err) => {
            eprintln!(
                "atc-server: OTEL_EXPORTER_OTLP_ENDPOINT={raw:?} failed to parse as URI ({err}); disabling OTel — the SDK would otherwise silently route to http://localhost:4318"
            );
            false
        }
    }
}

pub fn init_otel(_cfg: &Config) -> Option<OtelHandles> {
    if !endpoint_configured() {
        return None;
    }

    let resource = build_resource();

    let tracer_provider = match build_tracer_provider(resource.clone()) {
        Ok(provider) => provider,
        Err(err) => {
            // The tracing subscriber is initialized AFTER init_otel returns,
            // so any tracing::* macro fired here would dispatch to the
            // no-op global subscriber and silently disappear. eprintln!
            // bypasses tracing so operators see the misconfiguration.
            eprintln!(
                "atc-server: failed to build OTel tracer provider ({err}); OTel will be disabled"
            );
            return None;
        }
    };

    let meter_provider = match build_meter_provider(resource) {
        Ok(provider) => provider,
        Err(err) => {
            eprintln!(
                "atc-server: failed to build OTel meter provider ({err}); OTel will be disabled"
            );
            let _ = tracer_provider.shutdown();
            return None;
        }
    };

    let tracer = tracer_provider.tracer(TRACER_SCOPE_NAME);

    opentelemetry::global::set_tracer_provider(tracer_provider.clone());
    opentelemetry::global::set_meter_provider(meter_provider.clone());
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    install_metrics_recorder(&meter_provider);

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
    resource: Resource,
) -> Result<SdkTracerProvider, Box<dyn std::error::Error>> {
    // Do NOT call `.with_endpoint(...)` here. Per OTel spec, programmatic
    // endpoint config is treated as the FULL signal URL (no `/v1/traces`
    // append), while `OTEL_EXPORTER_OTLP_ENDPOINT` from env is treated as
    // the BASE and the SDK appends the signal path. We let the SDK read
    // the env var so operators get spec-compliant behavior — set
    // `OTEL_EXPORTER_OTLP_ENDPOINT=https://collector:4318` and the SDK
    // posts traces to `.../v1/traces` and metrics to `.../v1/metrics`.
    let exporter = SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .build()?;

    let processor = BatchSpanProcessor::builder(exporter).build();

    Ok(SdkTracerProvider::builder()
        .with_resource(resource)
        .with_sampler(load_sampler_from_env())
        .with_span_processor(processor)
        .build())
}

fn build_meter_provider(
    resource: Resource,
) -> Result<SdkMeterProvider, Box<dyn std::error::Error>> {
    // See `build_tracer_provider` for why `.with_endpoint(...)` is omitted.
    let exporter = MetricExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .build()?;

    let reader = PeriodicReader::builder(exporter).build();

    Ok(SdkMeterProvider::builder()
        .with_resource(resource)
        .with_reader(reader)
        .with_view(exponential_histogram_view)
        .build())
}

/// Flush and tear down the OTel SDK providers carried in `handles`.
///
/// Consumes `handles` because both providers are unusable after `shutdown()`
/// returns — keeping a reachable copy in scope would surface as silent noop
/// emissions on any subsequent call. Errors from individual providers are
/// logged and swallowed so a misbehaving exporter cannot block process exit:
/// the orchestration that calls this helper has already crossed every other
/// shutdown bound, and cooperative shutdown's contract is "exit, even if
/// something refuses to flush."
pub fn shutdown(handles: OtelHandles) {
    if let Err(err) = handles.tracer_provider.shutdown() {
        tracing::warn!(%err, "OTel tracer provider shutdown returned an error");
    }
    if let Err(err) = handles.meter_provider.shutdown() {
        tracing::warn!(%err, "OTel meter provider shutdown returned an error");
    }
}

/// Map every `Histogram` instrument to a base-2 exponential aggregation.
///
/// Shared by production `init_otel` and the test harness so tests observe the
/// same aggregation shape as production. Requires the
/// `spec_unstable_metrics_views` feature on `opentelemetry_sdk`.
pub fn exponential_histogram_view(inst: &Instrument) -> Option<Stream> {
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
}

/// Install the `metrics-rs` global recorder backed by the OTel meter provider.
///
/// Tolerates `SetRecorderError` so a process that already has a recorder
/// installed (e.g. an integration-test binary that wired the OTel test harness
/// in `tests/integration/common/mod.rs` before `init_otel` ran) does not
/// abort. The existing recorder remains in place; the new meter is unreachable
/// from the `metrics-rs` facade in that case.
fn install_metrics_recorder(meter_provider: &SdkMeterProvider) {
    use opentelemetry::metrics::MeterProvider as _;
    let meter = meter_provider.meter(METER_SCOPE_NAME);
    let recorder = metrics_exporter_otel::OpenTelemetryRecorder::new(meter);
    if let Err(err) = metrics::set_global_recorder(recorder) {
        tracing::debug!(%err, "global metrics recorder already installed; reusing existing");
    }
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
        "traceidratio" => match parse_ratio_with_spec_default(arg.as_deref()) {
            Some(ratio) => Sampler::TraceIdRatioBased(ratio),
            None => default_sampler(),
        },
        "parentbased_always_on" => default_sampler(),
        "parentbased_always_off" => Sampler::ParentBased(box_sampler(Sampler::AlwaysOff)),
        "parentbased_traceidratio" => match parse_ratio_with_spec_default(arg.as_deref()) {
            Some(ratio) => Sampler::ParentBased(box_sampler(Sampler::TraceIdRatioBased(ratio))),
            None => default_sampler(),
        },
        other => {
            eprintln!(
                "atc-server: unknown OTEL_TRACES_SAMPLER value {other:?}; falling back to parentbased_always_on"
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

/// Parse the ratio argument for `traceidratio` / `parentbased_traceidratio`,
/// honoring the OTel spec default of `1.0` when the argument is missing or
/// empty. Returns `None` only for an explicitly-present-but-invalid argument
/// (out of range, unparseable), in which case the caller falls back to the
/// default sampler. Without this distinction, a `traceidratio` selection with
/// no arg would silently switch to `parentbased_always_on` — surprising and
/// inconsistent with both the spec and the operator's stated intent.
fn parse_ratio_with_spec_default(raw: Option<&str>) -> Option<f64> {
    match raw {
        None | Some("") => Some(1.0),
        Some(value) => parse_ratio(value),
    }
}

fn parse_ratio(raw: &str) -> Option<f64> {
    match raw.parse::<f64>() {
        Ok(value) if (0.0..=1.0).contains(&value) => Some(value),
        Ok(value) => {
            eprintln!(
                "atc-server: OTEL_TRACES_SAMPLER_ARG={value} is outside [0, 1]; falling back to parentbased_always_on"
            );
            None
        }
        Err(err) => {
            eprintln!(
                "atc-server: OTEL_TRACES_SAMPLER_ARG={raw:?} could not be parsed as f64 ({err}); falling back to parentbased_always_on"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn sampler_kind(s: &Sampler) -> &'static str {
        // Discriminate variants without depending on PartialEq for trait
        // objects. ParentBased uses inner discriminant matching since the
        // boxed inner sampler is the load-bearing variant for this code.
        match s {
            Sampler::AlwaysOn => "AlwaysOn",
            Sampler::AlwaysOff => "AlwaysOff",
            Sampler::TraceIdRatioBased(_) => "TraceIdRatioBased",
            Sampler::ParentBased(inner) => match format!("{inner:?}").as_str() {
                d if d.starts_with("AlwaysOn") => "ParentBased(AlwaysOn)",
                d if d.starts_with("AlwaysOff") => "ParentBased(AlwaysOff)",
                d if d.starts_with("TraceIdRatioBased") => "ParentBased(TraceIdRatioBased)",
                _ => "ParentBased(other)",
            },
            _ => "other",
        }
    }

    fn ratio_value(s: &Sampler) -> Option<f64> {
        match s {
            Sampler::TraceIdRatioBased(r) => Some(*r),
            Sampler::ParentBased(inner) => {
                let dbg = format!("{inner:?}");
                // Inner Debug looks like `TraceIdRatioBased(0.1)`; pull the
                // numeric tail. Returns None for any other variant.
                let lhs = dbg.strip_prefix("TraceIdRatioBased(")?;
                let raw = lhs.strip_suffix(')')?;
                raw.parse::<f64>().ok()
            }
            _ => None,
        }
    }

    fn clear_sampler_env() {
        unsafe {
            std::env::remove_var("OTEL_TRACES_SAMPLER");
            std::env::remove_var("OTEL_TRACES_SAMPLER_ARG");
        }
    }

    #[test]
    #[serial]
    fn parse_ratio_with_spec_default_returns_one_when_arg_missing() {
        assert_eq!(parse_ratio_with_spec_default(None), Some(1.0));
    }

    #[test]
    #[serial]
    fn parse_ratio_with_spec_default_returns_one_when_arg_empty() {
        assert_eq!(parse_ratio_with_spec_default(Some("")), Some(1.0));
    }

    #[test]
    #[serial]
    fn parse_ratio_with_spec_default_round_trips_valid_value() {
        assert_eq!(parse_ratio_with_spec_default(Some("0.25")), Some(0.25));
        assert_eq!(parse_ratio_with_spec_default(Some("0")), Some(0.0));
        assert_eq!(parse_ratio_with_spec_default(Some("1")), Some(1.0));
    }

    #[test]
    #[serial]
    fn parse_ratio_returns_none_for_out_of_range() {
        assert_eq!(parse_ratio("1.5"), None);
        assert_eq!(parse_ratio("-0.1"), None);
    }

    #[test]
    #[serial]
    fn parse_ratio_returns_none_for_unparseable() {
        assert_eq!(parse_ratio("not-a-number"), None);
        assert_eq!(parse_ratio("0.5x"), None);
    }

    #[test]
    #[serial]
    fn load_sampler_unset_returns_default() {
        clear_sampler_env();
        assert_eq!(
            sampler_kind(&load_sampler_from_env()),
            "ParentBased(AlwaysOn)"
        );
    }

    #[test]
    #[serial]
    fn load_sampler_always_on() {
        clear_sampler_env();
        unsafe {
            std::env::set_var("OTEL_TRACES_SAMPLER", "always_on");
        }
        let s = load_sampler_from_env();
        clear_sampler_env();
        assert_eq!(sampler_kind(&s), "AlwaysOn");
    }

    #[test]
    #[serial]
    fn load_sampler_always_off() {
        clear_sampler_env();
        unsafe {
            std::env::set_var("OTEL_TRACES_SAMPLER", "always_off");
        }
        let s = load_sampler_from_env();
        clear_sampler_env();
        assert_eq!(sampler_kind(&s), "AlwaysOff");
    }

    #[test]
    #[serial]
    fn load_sampler_parentbased_always_off() {
        clear_sampler_env();
        unsafe {
            std::env::set_var("OTEL_TRACES_SAMPLER", "parentbased_always_off");
        }
        let s = load_sampler_from_env();
        clear_sampler_env();
        assert_eq!(sampler_kind(&s), "ParentBased(AlwaysOff)");
    }

    #[test]
    #[serial]
    fn load_sampler_traceidratio_with_arg() {
        clear_sampler_env();
        unsafe {
            std::env::set_var("OTEL_TRACES_SAMPLER", "traceidratio");
            std::env::set_var("OTEL_TRACES_SAMPLER_ARG", "0.1");
        }
        let s = load_sampler_from_env();
        clear_sampler_env();
        assert_eq!(sampler_kind(&s), "TraceIdRatioBased");
        assert_eq!(ratio_value(&s), Some(0.1));
    }

    #[test]
    #[serial]
    fn load_sampler_traceidratio_without_arg_defaults_to_one() {
        // Per OTel spec, OTEL_TRACES_SAMPLER_ARG default is 1.0 when unset
        // for ratio samplers. Operators selecting traceidratio with no arg
        // should sample everything, NOT silently fall back to parentbased.
        clear_sampler_env();
        unsafe {
            std::env::set_var("OTEL_TRACES_SAMPLER", "traceidratio");
        }
        let s = load_sampler_from_env();
        clear_sampler_env();
        assert_eq!(sampler_kind(&s), "TraceIdRatioBased");
        assert_eq!(ratio_value(&s), Some(1.0));
    }

    #[test]
    #[serial]
    fn load_sampler_parentbased_traceidratio_without_arg_defaults_to_one() {
        clear_sampler_env();
        unsafe {
            std::env::set_var("OTEL_TRACES_SAMPLER", "parentbased_traceidratio");
        }
        let s = load_sampler_from_env();
        clear_sampler_env();
        assert_eq!(sampler_kind(&s), "ParentBased(TraceIdRatioBased)");
        assert_eq!(ratio_value(&s), Some(1.0));
    }

    #[test]
    #[serial]
    fn load_sampler_traceidratio_with_invalid_arg_falls_back_to_default() {
        clear_sampler_env();
        unsafe {
            std::env::set_var("OTEL_TRACES_SAMPLER", "traceidratio");
            std::env::set_var("OTEL_TRACES_SAMPLER_ARG", "1.5");
        }
        let s = load_sampler_from_env();
        clear_sampler_env();
        assert_eq!(sampler_kind(&s), "ParentBased(AlwaysOn)");
    }

    #[test]
    #[serial]
    fn load_sampler_unknown_value_falls_back_to_default() {
        clear_sampler_env();
        unsafe {
            std::env::set_var("OTEL_TRACES_SAMPLER", "tail_based_or_something");
        }
        let s = load_sampler_from_env();
        clear_sampler_env();
        assert_eq!(sampler_kind(&s), "ParentBased(AlwaysOn)");
    }

    #[test]
    #[serial]
    fn load_sampler_case_insensitive() {
        clear_sampler_env();
        unsafe {
            std::env::set_var("OTEL_TRACES_SAMPLER", "ALWAYS_OFF");
        }
        let s = load_sampler_from_env();
        clear_sampler_env();
        assert_eq!(sampler_kind(&s), "AlwaysOff");
    }
}
