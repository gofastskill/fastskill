//! Command output routing.
//!
//! Commands must not write to `stdout` directly. Under `fastskill mcp serve
//! --transport stdio`, `stdout` *is* the JSON-RPC channel, so a stray
//! `println!` corrupts the protocol stream and the tool result is whatever the
//! framework saw instead -- in practice the literal string `"OK"`, with the real
//! output interleaved between JSON-RPC frames:
//!
//! ```text
//! {"jsonrpc":"2.0","id":1,"result":{...}}
//! ID          Name        Description          <- raw table on stdout
//! demo-skill  demo-skill  A demo skill...      <- corrupts the stream
//! {"jsonrpc":"2.0","id":4,"result":{"content":[{"type":"text","text":"OK"}]}}
//! ```
//!
//! Every command therefore emits through [`emit`] (usually via the [`outln!`]
//! macro), which routes according to the process-wide [`Mode`] chosen once at
//! startup:
//!
//! - [`Mode::Direct`] -- ordinary CLI use. Writes straight to `stdout`, so
//!   long-running commands keep streaming their progress as before.
//! - [`Mode::Capture`] -- `mcp serve`. Writes into a task-local buffer that the
//!   command wrapper drains into `ctx.framework_println`, which the framework
//!   turns into the tool's `content`.
//!
//! # Why task-local rather than a plain global
//!
//! MCP tool calls are dispatched concurrently, each on its own task. A global
//! buffer would interleave the output of simultaneous calls; a task-local keeps
//! each call's output to itself and still crosses `.await` points within that
//! task.
//!
//! A task spawned *inside* a command does not inherit the task-local. Such
//! output falls back to `stderr` under [`Mode::Capture`] -- missing from the
//! tool result rather than corrupting the protocol, which is the safe direction
//! to fail. No command currently emits from a spawned task (the per-trial
//! `JoinSet` in `eval run` returns its results for the parent to render), so
//! there is deliberately no API for propagating the buffer into one; add it
//! alongside the first caller that needs it.

use std::sync::{Arc, Mutex, OnceLock};

/// Where [`emit`] sends output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Write to `stdout` immediately (ordinary CLI use).
    Direct,
    /// Buffer into the task-local sink; fall back to `stderr` (`mcp serve`).
    Capture,
}

static MODE: OnceLock<Mode> = OnceLock::new();

/// A captured output buffer, shared with any explicitly-propagated child task.
pub type Sink = Arc<Mutex<String>>;

tokio::task_local! {
    static SINK: Sink;
}

/// Fix the output mode for this process. The first call wins; later calls are
/// ignored so a stray re-initialisation cannot redirect output mid-run.
pub fn init(mode: Mode) {
    let _ = MODE.set(mode);
}

/// The active mode, defaulting to [`Mode::Direct`] when [`init`] was never
/// called (unit tests, and any path that never reaches `main`).
pub fn mode() -> Mode {
    MODE.get().copied().unwrap_or(Mode::Direct)
}

/// Emit one line of user-visible output.
///
/// Prefer the [`outln!`] macro, which mirrors `println!`'s formatting.
pub fn emit(line: &str) {
    match mode() {
        Mode::Direct => println!("{}", line),
        Mode::Capture => {
            let captured = SINK.try_with(|s| {
                if let Ok(mut buf) = s.lock() {
                    buf.push_str(line);
                    buf.push('\n');
                }
            });
            // No task-local sink in scope: this came from a task spawned inside
            // a command. Route to stderr rather than stdout so the JSON-RPC
            // stream stays clean.
            if captured.is_err() {
                eprintln!("{}", line);
            }
        }
    }
}

/// Run `fut` with `sink` installed as the task-local output buffer.
async fn with_sink<F: std::future::Future>(sink: Sink, fut: F) -> F::Output {
    SINK.scope(sink, fut).await
}

/// Run `fut` with a fresh buffer, returning its output alongside everything
/// emitted during the call.
pub async fn capture<F: std::future::Future>(fut: F) -> (F::Output, String) {
    let sink: Sink = Arc::new(Mutex::new(String::new()));
    let out = with_sink(Arc::clone(&sink), fut).await;
    let text = sink.lock().map(|b| b.clone()).unwrap_or_default();
    (out, text)
}

/// `println!` for command output, routed through [`emit`].
#[macro_export]
macro_rules! outln {
    () => { $crate::output::emit("") };
    ($($arg:tt)*) => { $crate::output::emit(&format!($($arg)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_defaults_to_direct_when_uninitialised() {
        // `MODE` is a process-wide `OnceLock`; this test only asserts the
        // default read, it must not call `init`.
        assert_eq!(Mode::Direct, mode());
    }

    #[tokio::test]
    async fn capture_collects_emitted_lines() {
        let (_, text) = capture(async {
            emit_as_capture("first");
            emit_as_capture("second");
        })
        .await;
        assert_eq!(text, "first\nsecond\n");
    }

    #[tokio::test]
    async fn capture_returns_the_future_output() {
        let (value, text) = capture(async { 42 }).await;
        assert_eq!(value, 42);
        assert!(text.is_empty());
    }

    #[tokio::test]
    async fn concurrent_captures_do_not_interleave() {
        // The property that rules out a plain global buffer.
        let a = capture(async {
            emit_as_capture("a1");
            tokio::task::yield_now().await;
            emit_as_capture("a2");
        });
        let b = capture(async {
            emit_as_capture("b1");
            tokio::task::yield_now().await;
            emit_as_capture("b2");
        });
        let ((_, ta), (_, tb)) = tokio::join!(a, b);
        assert_eq!(ta, "a1\na2\n");
        assert_eq!(tb, "b1\nb2\n");
    }

    #[tokio::test]
    async fn output_from_a_spawned_task_never_reaches_the_sink() {
        // Documents the known limitation: a spawned task does not inherit the
        // task-local, so its output is absent from the tool result rather than
        // being written to stdout, where it would corrupt JSON-RPC.
        let (_, text) = capture(async {
            emit_as_capture("parent");
            tokio::spawn(async { assert!(SINK.try_with(|_| ()).is_err()) })
                .await
                .unwrap();
        })
        .await;
        assert_eq!(text, "parent\n");
    }

    #[tokio::test]
    async fn capture_survives_await_points() {
        let (_, text) = capture(async {
            emit_as_capture("before");
            tokio::task::yield_now().await;
            emit_as_capture("after");
        })
        .await;
        assert_eq!(text, "before\nafter\n");
    }

    /// `emit` consults the process-wide mode, which unit tests must not pin.
    /// This writes to the task-local sink directly so the capture behaviour can
    /// be tested independently of `MODE`.
    fn emit_as_capture(line: &str) {
        SINK.with(|s| {
            let mut buf = s.lock().unwrap();
            buf.push_str(line);
            buf.push('\n');
        });
    }
}
