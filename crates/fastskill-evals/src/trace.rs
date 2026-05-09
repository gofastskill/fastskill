//! Trace event types for eval case execution

use aikit_sdk::{AgentEvent, AgentEventPayload};
use serde::{Deserialize, Serialize};

/// A single line in a trace.jsonl file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    /// Sequence number (0-based)
    pub seq: usize,
    /// Event payload
    pub payload: TracePayload,
}

/// Payload of a trace event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TracePayload {
    /// A raw JSON line from the agent (tool commands, structured output)
    RawJson { data: serde_json::Value },
    /// A raw text line from stdout
    RawLine { line: String },
    /// A raw bytes chunk (base64-encoded)
    RawBytes { b64: String },
    /// Execution error
    Error { message: String },
    /// Case timed out
    Timeout,
    /// Token usage event emitted by the SDK during agent execution
    TokenUsageLine {
        usage: serde_json::Value,
        source: String,
        raw_agent_line_seq: u64,
    },
}

/// Convert aikit-sdk AgentEvent to internal TraceEvent
pub fn agent_events_to_trace(events: &[AgentEvent]) -> Vec<TraceEvent> {
    events
        .iter()
        .map(|ev| {
            let payload = match &ev.payload {
                AgentEventPayload::JsonLine(value) => TracePayload::RawJson {
                    data: value.clone(),
                },
                AgentEventPayload::RawLine(line) => TracePayload::RawLine { line: line.clone() },
                AgentEventPayload::RawBytes(bytes) => {
                    use base64::{engine::general_purpose::STANDARD, Engine as _};
                    TracePayload::RawBytes {
                        b64: STANDARD.encode(bytes),
                    }
                }
                AgentEventPayload::TokenUsageLine {
                    usage,
                    source,
                    raw_agent_line_seq,
                } => TracePayload::TokenUsageLine {
                    usage: serde_json::to_value(usage).unwrap_or(serde_json::Value::Null),
                    source: serde_json::to_value(source)
                        .ok()
                        .and_then(|v| v.as_str().map(|s| s.to_lowercase()))
                        .unwrap_or_else(|| format!("{:?}", source).to_lowercase()),
                    raw_agent_line_seq: *raw_agent_line_seq,
                },
                _ => TracePayload::RawJson {
                    data: serde_json::json!({
                        "type": "unknown_agent_event_payload",
                        "raw": format!("{:?}", ev.payload)
                    }),
                },
            };
            TraceEvent {
                seq: ev.seq as usize,
                payload,
            }
        })
        .collect()
}

/// Convert raw stdout lines to trace events
pub fn stdout_to_trace(stdout: &[u8]) -> Vec<TraceEvent> {
    let text = String::from_utf8_lossy(stdout);
    let mut events = Vec::new();

    for (seq, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Try to parse as JSON first
        let payload = if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            TracePayload::RawJson { data: value }
        } else {
            TracePayload::RawLine {
                line: line.to_string(),
            }
        };

        events.push(TraceEvent { seq, payload });
    }

    events
}

/// Serialize trace events to JSONL format
pub fn trace_to_jsonl(events: &[TraceEvent]) -> String {
    events
        .iter()
        .filter_map(|e| serde_json::to_string(e).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdout_to_trace_text_lines() {
        let stdout = b"hello world\nfoo bar\n";
        let events = stdout_to_trace(stdout);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 0);
        assert!(
            matches!(&events[0].payload, TracePayload::RawLine { line } if line == "hello world")
        );
    }

    #[test]
    fn test_stdout_to_trace_json_lines() {
        let stdout = b"{\"key\": \"value\"}\nplain line\n";
        let events = stdout_to_trace(stdout);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0].payload, TracePayload::RawJson { .. }));
        assert!(matches!(&events[1].payload, TracePayload::RawLine { .. }));
    }

    #[test]
    fn test_trace_to_jsonl() {
        let events = vec![TraceEvent {
            seq: 0,
            payload: TracePayload::RawLine {
                line: "test".to_string(),
            },
        }];
        let jsonl = trace_to_jsonl(&events);
        assert!(jsonl.contains("\"seq\":0"));
        assert!(jsonl.contains("raw_line"));
    }

    #[test]
    fn test_token_usage_line_serializes_as_distinct_type() {
        let event = TraceEvent {
            seq: 7,
            payload: TracePayload::TokenUsageLine {
                usage: serde_json::json!({"input_tokens": 1234, "output_tokens": 567}),
                source: "claude".to_string(),
                raw_agent_line_seq: 6,
            },
        };
        let jsonl = trace_to_jsonl(&[event.clone()]);
        assert!(
            jsonl.contains("\"type\":\"token_usage_line\""),
            "expected token_usage_line type tag, got: {}",
            jsonl
        );
        assert!(
            !jsonl.contains("\"type\":\"raw_json\""),
            "token_usage_line must not serialize as raw_json, got: {}",
            jsonl
        );
        let deserialized: TraceEvent = serde_json::from_str(&jsonl).unwrap();
        assert!(
            matches!(deserialized.payload, TracePayload::TokenUsageLine { .. }),
            "deserialized payload must be TokenUsageLine"
        );
    }

    #[test]
    fn test_count_raw_json_excludes_token_usage_line() {
        use crate::checks::count_raw_json_events;
        let events = vec![
            TraceEvent {
                seq: 0,
                payload: TracePayload::TokenUsageLine {
                    usage: serde_json::json!({"input_tokens": 100, "output_tokens": 50}),
                    source: "claude".to_string(),
                    raw_agent_line_seq: 0,
                },
            },
            TraceEvent {
                seq: 1,
                payload: TracePayload::TokenUsageLine {
                    usage: serde_json::json!({"input_tokens": 200, "output_tokens": 100}),
                    source: "codex".to_string(),
                    raw_agent_line_seq: 1,
                },
            },
        ];
        let jsonl = trace_to_jsonl(&events);
        let count = count_raw_json_events(&jsonl);
        assert_eq!(
            count, 0,
            "count_raw_json_events must return 0 for token_usage_line-only traces, got: {}",
            count
        );
    }
}
