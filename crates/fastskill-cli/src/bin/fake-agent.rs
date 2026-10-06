//! Minimal cross-platform fake "agent" CLI, used ONLY by fastskill-cli's own
//! integration tests (see `tests/cli/eval_tests.rs`) to simulate an
//! installed eval runtime without depending on any real agent CLI being
//! present on the machine or CI runner.
//!
//! Not part of the product surface: `release.yml` copies only the
//! `fastskill`/`fastskill.exe` binary out of `target/<triple>/release/`, so
//! this bin target is compiled but never shipped.
//!
//! `aikit-sdk`'s runtime-availability probe (`is_agent_available`) discovers
//! runtimes by looking for a binary of the right logical name on `PATH` and
//! running it with `--version`, checking only the *exit code* -- stdout
//! content is ignored. Tests that need a runtime to be "available" copy this
//! one compiled binary into a scratch directory under the logical name they
//! want to probe (e.g. `agent` / `agent.exe`, `codex` / `codex.exe`) and
//! prepend that directory to `PATH`. Because it is a real compiled
//! executable rather than a bash script, the exact same fixture works
//! unmodified on Windows and Unix: no shebang to interpret, no PATHEXT gap
//! (Windows never finds an extension-less file via aikit-sdk's PATH+PATHEXT
//! resolution), and no Unix-only `:` PATH-separator assumption.
//!
//! Behavior is controlled entirely by environment variables, selected via
//! `FAKE_AGENT_MODE`:
//! - unset / `"simple"` (default): always succeeds, printing one raw JSON
//!   event line.
//! - `"counter"`: cross-process-locked increment of a shared count file
//!   under `FASTSKILL_TEST_STATE_DIR`; succeeds while the running count is
//!   `<= FAKE_AGENT_PASS_LIMIT` (default 3), and past that limit fails in the
//!   way `FAKE_AGENT_FAIL_KIND` selects -- used to exercise trial pass/fail
//!   threshold semantics. The two kinds are not interchangeable:
//!   - `"exit"` (default): exit 1. A nonzero exit means the trial produced no
//!     measurement at all, so it is recorded `error` and left *out* of the
//!     pass rate rather than counted against it.
//!   - `"answer"`: exit 0 with a complete, well-formed turn that simply omits
//!     `FAKE_AGENT_MARKER` from the answer. This is the shape of a genuine
//!     failure -- the agent ran and got it wrong -- and it is the only one a
//!     pass rate can move on. Within the limit the marker is printed as
//!     assistant text, so a `command_contains` check on it separates the two.
//! - `"interval"`: sleeps 500ms, then appends a locked
//!   `"<start_ns> <end_ns>"` line to
//!   `FASTSKILL_TEST_STATE_DIR/intervals.txt` -- used to prove trials
//!   actually ran concurrently (overlapping windows) rather than serially.
//!
//! `--version` always exits 0 immediately regardless of mode: aikit-sdk's
//! probe only checks the exit code, never the printed text.
//!
//! Every mode ends its output with the *terminal frame* of whichever backend
//! it was installed as (see `terminal_frame`). Some decoders declare a
//! `terminal_event` capability, which is aikit-sdk's way of saying "this
//! backend always announces the end of a turn, so a stream that just stops is
//! a truncated run, not a quiet one". Those runs are recorded as `error` and
//! excluded from the pass rate. A fixture that claims to be `codex` while
//! emitting a stream `codex` could never produce would make every trial an
//! error, so the frame is part of impersonating the backend faithfully.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.get(1).map(String::as_str) == Some("--version") {
        std::process::exit(0);
    }

    // Drain stdin to EOF before anything else. aikit-sdk's runner writes the
    // eval prompt to the child's stdin and then drops the handle. A trivial
    // fake agent exits far faster than a real agent CLI (no interpreter
    // startup, no model call), so it can be gone before that write lands --
    // the parent then hits a broken pipe and reports the whole invocation as
    // an error, overriding this process's own exit code. That made the
    // counter-mode trials fail nondeterministically. Holding the read end
    // open until the parent finishes writing closes the race.
    let mut discarded = String::new();
    let _ = std::io::stdin().read_to_string(&mut discarded);

    let terminal = terminal_frame(logical_name(&args).as_deref());

    match env::var("FAKE_AGENT_MODE").as_deref() {
        Ok("counter") => run_counter(terminal),
        Ok("interval") => run_interval(terminal),
        _ => run_simple(terminal),
    }
}

/// The logical runtime name this copy was installed under. Tests copy one
/// compiled binary to `<scratch>/codex` (or `codex.exe`) and prepend that
/// directory to `PATH`, so argv[0]'s file stem is the backend being faked.
fn logical_name(args: &[String]) -> Option<String> {
    Path::new(args.first()?)
        .file_stem()?
        .to_str()
        .map(str::to_string)
}

/// The line that tells a `terminal_event` decoder the turn ended cleanly.
///
/// Only the backends whose decoders actually declare that capability need one;
/// for everything else the drain reads to EOF and a stream that simply stops is
/// a complete run. `pi` is deliberately absent: it is an RPC server, and a
/// fixture that answered its handshake would be a different program than this.
fn terminal_frame(logical_name: Option<&str>) -> Option<&'static str> {
    match logical_name {
        Some("codex") => Some("{\"type\":\"turn.completed\"}"),
        Some("claude") => Some("{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false}"),
        _ => None,
    }
}

fn emit(terminal: Option<&str>) {
    println!("{{\"event\":\"ok\"}}");
    if let Some(frame) = terminal {
        println!("{frame}");
    }
}

fn run_simple(terminal: Option<&str>) {
    emit(terminal);
}

/// Resolves `FASTSKILL_TEST_STATE_DIR`, creating it if needed.
fn state_dir() -> PathBuf {
    let dir = env::var("FASTSKILL_TEST_STATE_DIR")
        .expect("FASTSKILL_TEST_STATE_DIR must be set for this fake-agent mode");
    let path = PathBuf::from(dir);
    fs::create_dir_all(&path).expect("failed to create FASTSKILL_TEST_STATE_DIR");
    path
}

/// Runs `f` while holding an exclusive, cross-process, cross-platform lock
/// on `<state_dir>/lock` (via `std::fs::File::lock`, which uses `flock` on
/// Unix and `LockFileEx` on Windows) so concurrent fake-agent invocations
/// can safely read-modify-write shared state files.
fn with_lock<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
    let lock_path = dir.join("lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .expect("failed to open lock file");
    lock_file.lock().expect("failed to acquire lock");
    let result = f();
    let _ = lock_file.unlock();
    result
}

fn run_counter(terminal: Option<&str>) {
    let dir = state_dir();
    let pass_limit: u64 = env::var("FAKE_AGENT_PASS_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let fail_by_answer = env::var("FAKE_AGENT_FAIL_KIND").as_deref() == Ok("answer");
    let marker = env::var("FAKE_AGENT_MARKER").unwrap_or_else(|_| "fake-agent-ok".to_string());

    let count = with_lock(&dir, || {
        let count_path = dir.join("count");
        let current: u64 = fs::read_to_string(&count_path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let next = current + 1;
        fs::write(&count_path, next.to_string()).expect("failed to write count file");
        next
    });

    let within_limit = count <= pass_limit;

    // The marker is the answer's content, so it goes out as assistant text
    // rather than as the opaque `{"event":"ok"}` line: a check reads the
    // decoded trace, and raw stdout is deliberately not a haystack.
    if within_limit && fail_by_answer {
        println!("{{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"{marker}\"}}");
    }

    // Always emit the JSON event line regardless of pass/fail, matching the
    // original bash fixture: a persisted trace still records the attempt
    // even on the trial that pushes the count past the threshold.
    emit(terminal);

    if !within_limit && !fail_by_answer {
        std::process::exit(1);
    }
}

fn run_interval(terminal: Option<&str>) {
    let dir = state_dir();
    let start_ns = now_ns();
    std::thread::sleep(Duration::from_millis(500));
    let end_ns = now_ns();

    with_lock(&dir, || {
        let intervals_path = dir.join("intervals.txt");
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&intervals_path)
            .expect("failed to open intervals file");
        writeln!(f, "{start_ns} {end_ns}").expect("failed to append interval line");
    });

    emit(terminal);
}

fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before UNIX_EPOCH")
        .as_nanos()
}
