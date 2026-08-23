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
//!   `<= FAKE_AGENT_PASS_LIMIT` (default 3), fails (exit 1) after -- used to
//!   exercise trial pass/fail threshold semantics.
//! - `"interval"`: sleeps 500ms, then appends a locked
//!   `"<start_ns> <end_ns>"` line to
//!   `FASTSKILL_TEST_STATE_DIR/intervals.txt` -- used to prove trials
//!   actually ran concurrently (overlapping windows) rather than serially.
//!
//! `--version` always exits 0 immediately regardless of mode: aikit-sdk's
//! probe only checks the exit code, never the printed text.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.get(1).map(String::as_str) == Some("--version") {
        std::process::exit(0);
    }

    match env::var("FAKE_AGENT_MODE").as_deref() {
        Ok("counter") => run_counter(),
        Ok("interval") => run_interval(),
        _ => run_simple(),
    }
}

fn run_simple() {
    println!("{{\"event\":\"ok\"}}");
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

fn run_counter() {
    let dir = state_dir();
    let pass_limit: u64 = env::var("FAKE_AGENT_PASS_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);

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

    // Always emit the JSON event line regardless of pass/fail, matching the
    // original bash fixture: a persisted trace still records the attempt
    // even on the trial that pushes the count past the threshold.
    println!("{{\"event\":\"ok\"}}");

    if count > pass_limit {
        std::process::exit(1);
    }
}

fn run_interval() {
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

    println!("{{\"event\":\"ok\"}}");
}

fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before UNIX_EPOCH")
        .as_nanos()
}
