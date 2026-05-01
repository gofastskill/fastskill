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
    /// A raw JSON line from the agent
    RawJson { data: serde_json::Value },
    /// A raw text line from stdout
    RawLine { line: String },
    /// A raw bytes chunk (base64-encoded)
    RawBytes { b64: String },
    /// Execution error
    Error { message: String },
    /// Case timed out
    Timeout,
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
                } => TracePayload::RawJson {
                    data: serde_json::json!({
                        "type": "token_usage",
                        "usage": usage,
                        "source": source,
                        "raw_agent_line_seq": raw_agent_line_seq
                    }),
                },
                _ => TracePayload::RawLine {
                    line: format!("<unhandled aikit event variant @ seq {}>", ev.seq),
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
}
