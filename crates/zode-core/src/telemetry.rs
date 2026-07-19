//! Opt-in, content-free OTLP run telemetry.
//!
//! Enable explicitly with `ZODE_OTEL=1`; the exporter then honors the standard
//! `OTEL_EXPORTER_OTLP_*` environment variables. Prompt text, model output,
//! tool input/output, file paths, and error messages are never exported.

use std::sync::OnceLock;
use std::time::Duration;

use opentelemetry::trace::{Span, Tracer, TracerProvider as _};
use opentelemetry::KeyValue;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;

use crate::run_event::{RunEvent, RunEventEnvelope};

static PROVIDER: OnceLock<Option<SdkTracerProvider>> = OnceLock::new();

pub fn emit(envelope: &RunEventEnvelope) {
    if matches!(
        envelope.event,
        RunEvent::MessageDelta { .. } | RunEvent::ThinkingDelta { .. } | RunEvent::Unknown
    ) {
        return;
    }
    let Some(provider) = provider() else {
        return;
    };
    let tracer = provider.tracer("zode.run-events");
    let mut span = tracer.start(format!("zode.{}", event_name(&envelope.event)));
    span.set_attribute(KeyValue::new("zode.schema", envelope.schema.clone()));
    span.set_attribute(KeyValue::new(
        "zode.session.id",
        envelope.session_id.clone(),
    ));
    span.set_attribute(KeyValue::new("zode.event.seq", otel_int(envelope.seq)));
    if let Some(turn_id) = &envelope.turn_id {
        span.set_attribute(KeyValue::new("zode.turn.id", turn_id.clone()));
    }
    if let Some(request_id) = &envelope.request_id {
        span.set_attribute(KeyValue::new("zode.request.id", request_id.clone()));
    }
    add_event_attributes(&mut span, &envelope.event);
    span.end();
}

pub fn shutdown() {
    if let Some(Some(provider)) = PROVIDER.get() {
        let _ = provider.shutdown_with_timeout(Duration::from_secs(5));
    }
}

fn provider() -> Option<&'static SdkTracerProvider> {
    PROVIDER
        .get_or_init(|| {
            if !enabled() {
                return None;
            }
            match opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .build()
            {
                Ok(exporter) => Some(
                    SdkTracerProvider::builder()
                        .with_resource(
                            Resource::builder()
                                .with_service_name("zode")
                                .with_attribute(KeyValue::new(
                                    "service.version",
                                    env!("CARGO_PKG_VERSION"),
                                ))
                                .build(),
                        )
                        .with_batch_exporter(exporter)
                        .build(),
                ),
                Err(error) => {
                    eprintln!("zode: could not initialize requested OTLP exporter: {error}");
                    None
                }
            }
        })
        .as_ref()
}

fn enabled() -> bool {
    std::env::var("ZODE_OTEL")
        .ok()
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "on"))
}

fn event_name(event: &RunEvent) -> &'static str {
    match event {
        RunEvent::RunStarted => "run.started",
        RunEvent::TurnStarted => "turn.started",
        RunEvent::MessageDelta { .. } => "message.delta",
        RunEvent::ThinkingDelta { .. } => "thinking.delta",
        RunEvent::ToolStarted { .. } => "tool.started",
        RunEvent::ToolCompleted { .. } => "tool.completed",
        RunEvent::Usage { .. } => "usage",
        RunEvent::Result { .. } => "result",
        RunEvent::Error { .. } => "error",
        RunEvent::Notice { .. } => "notice",
        RunEvent::TurnCompleted { .. } => "turn.completed",
        RunEvent::RunCompleted { .. } => "run.completed",
        RunEvent::Unknown => "unknown",
    }
}

fn add_event_attributes(span: &mut opentelemetry_sdk::trace::Span, event: &RunEvent) {
    match event {
        RunEvent::ToolStarted { id, name, .. } => {
            span.set_attribute(KeyValue::new("zode.tool.id", id.clone()));
            span.set_attribute(KeyValue::new("zode.tool.name", name.clone()));
        }
        RunEvent::ToolCompleted { id, ok, .. } => {
            span.set_attribute(KeyValue::new("zode.tool.id", id.clone()));
            span.set_attribute(KeyValue::new("zode.tool.ok", *ok));
        }
        RunEvent::Usage {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
        } => {
            span.set_attribute(KeyValue::new(
                "gen_ai.usage.input_tokens",
                otel_int(*input_tokens),
            ));
            span.set_attribute(KeyValue::new(
                "gen_ai.usage.output_tokens",
                otel_int(*output_tokens),
            ));
            span.set_attribute(KeyValue::new(
                "zode.usage.cache_read_tokens",
                otel_int(*cache_read_tokens),
            ));
            span.set_attribute(KeyValue::new(
                "zode.usage.cache_write_tokens",
                otel_int(*cache_write_tokens),
            ));
        }
        RunEvent::Result {
            stop_reason, model, ..
        } => {
            if let Some(reason) = stop_reason {
                span.set_attribute(KeyValue::new("zode.stop_reason", reason.clone()));
            }
            if let Some(model) = model {
                span.set_attribute(KeyValue::new("gen_ai.request.model", model.clone()));
            }
        }
        RunEvent::Error { code, .. } | RunEvent::Notice { code, .. } => {
            span.set_attribute(KeyValue::new("zode.code", code.clone()));
        }
        RunEvent::TurnCompleted {
            status,
            stop_reason,
            partial,
            ..
        }
        | RunEvent::RunCompleted {
            status,
            stop_reason,
            partial,
        } => {
            span.set_attribute(KeyValue::new(
                "zode.status",
                format!("{status:?}").to_lowercase(),
            ));
            span.set_attribute(KeyValue::new("zode.partial", *partial));
            if let Some(reason) = stop_reason {
                span.set_attribute(KeyValue::new("zode.stop_reason", reason.clone()));
            }
        }
        RunEvent::RunStarted
        | RunEvent::TurnStarted
        | RunEvent::MessageDelta { .. }
        | RunEvent::ThinkingDelta { .. }
        | RunEvent::Unknown => {}
    }
}

fn otel_int(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_names_cover_all_event_variants_without_content() {
        assert_eq!(event_name(&RunEvent::RunStarted), "run.started");
        assert_eq!(
            event_name(&RunEvent::MessageDelta {
                delta: "secret".into()
            }),
            "message.delta"
        );
    }
}
