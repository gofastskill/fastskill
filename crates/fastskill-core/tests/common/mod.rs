//! Shared fixture for the `git daemon` integration tests
//! (`git_content_cache_test.rs`, `offline_verification_sweep_test.rs`,
//! `repo_index_refresh_git_test.rs`). All three spin up a real `git daemon`
//! serving bare repos over `git://127.0.0.1` on a loopback port -- this is
//! not "the network": no DNS, no external host, nothing that can be flaky in
//! CI -- it is the only way to exercise the real `ls_remote`/`clone_repository`
//! code paths without a local filesystem path, which SEC-11 refuses outright
//! (`protocol.file.allow=never`; see `storage::git::build_clone_args`).
//!
//! `tests/common/mod.rs` (rather than `tests/common.rs`) is the idiomatic way
//! to share code between Rust integration-test binaries: `mod.rs` files are
//! never themselves collected as a `[[test]]` target by cargo/nextest, only
//! pulled in via `mod common;` in each binary that needs it. Because each
//! test binary compiles this module separately and uses only a subset of it,
//! `#![allow(dead_code)]` below is expected and correct, not a code smell.

#![allow(dead_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// A local `git daemon` exporting every repo under its base path (no
/// `git-daemon-export-ok` marker required), bound to an OS-assigned loopback
/// port. Killed on drop.
pub struct GitDaemonFixture {
    child: Child,
    port: u16,
    _base: TempDir,
}

impl GitDaemonFixture {
    pub fn start(base: TempDir) -> Self {
        // `free_loopback_port` can only *suggest* a port: it binds :0, reads the
        // number, then drops the listener so `git daemon` can take it. Under a
        // parallel test run another process can win that gap, and the daemon
        // then dies immediately with "address already in use". Retry on a fresh
        // port instead of failing the test for an unlucky race.
        const MAX_ATTEMPTS: u32 = 10;
        for attempt in 1..=MAX_ATTEMPTS {
            let port = free_loopback_port();
            let mut child = git_command()
                .args([
                    "daemon",
                    "--reuseaddr",
                    "--export-all",
                    "--listen=127.0.0.1",
                    &format!("--port={port}"),
                    &format!("--base-path={}", base.path().display()),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("failed to start `git daemon`; is git installed with daemon support?");

            if wait_for_port(port, &mut child) {
                return Self {
                    child,
                    port,
                    _base: base,
                };
            }

            // Did not come up: reap this attempt before trying another port.
            let _ = child.kill();
            let _ = child.wait();
            assert!(
                attempt < MAX_ATTEMPTS,
                "git daemon failed to start listening after {MAX_ATTEMPTS} attempts"
            );
        }
        unreachable!("loop either returns or asserts on the final attempt")
    }

    pub fn repo_url(&self, repo_name: &str) -> String {
        format!("git://127.0.0.1:{}/{repo_name}", self.port)
    }

    /// Kill the daemon and wait for the port to actually stop accepting
    /// connections. This is the "network egress made to fail" step some
    /// tests need (proving a subsequent install can only be served from
    /// cache): deterministic (polls a local condition, no sleep-and-hope)
    /// and fast, with no reliance on the test environment actually lacking
    /// internet access.
    ///
    /// Portable process-tree kill, because a plain `child.kill()` is not
    /// enough to prove the port is dead on every platform:
    /// - Linux: `git daemon` re-execs into a child `git-daemon` process that
    ///   is what actually holds the listening socket, separate from the pid
    ///   `spawn` hands back. We enumerate descendants via
    ///   `/proc/<pid>/task/<pid>/children` and `kill -9` each by exact pid,
    ///   rather than a process-group-wide signal -- this test process was not
    ///   itself given a fresh process group, so a group-wide signal risks
    ///   hitting far more than the daemon it is scoped to.
    /// - Windows: there is no `/proc` and no `fork()`, so there is no
    ///   equivalent descendant-enumeration step to port. `taskkill /F /T /PID`
    ///   kills the whole process tree rooted at the spawned pid in one shot,
    ///   which covers both "daemon runs in-process" and "daemon spawned a
    ///   child" without needing to know which.
    /// - Other Unix (e.g. macOS, not exercised in this repo's CI): fall back
    ///   to a plain `child.kill()`.
    ///
    /// A failed kill must never hang the suite: reaping the child below is
    /// bounded rather than a blocking `wait()`, and the port-poll after it
    /// has its own deadline and panics with a clear message instead of
    /// spinning forever.
    pub fn kill_and_wait(mut self) {
        #[cfg(target_os = "linux")]
        {
            let mut victims = descendant_pids(self.child.id());
            victims.push(self.child.id());
            for pid in victims {
                let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
            }
        }
        #[cfg(target_os = "windows")]
        {
            // `/F` force-kills, `/T` kills the tree rooted at `/PID`. Ignore
            // the exit status: taskkill fails if the process already exited,
            // which is not an error for us -- the port-poll below is what
            // actually proves the daemon is gone.
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &self.child.id().to_string()])
                .status();
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = self.child.kill();
        }

        reap_bounded(&mut self.child, Duration::from_secs(5));

        let deadline = Instant::now() + Duration::from_secs(5);
        while TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
            if Instant::now() >= deadline {
                panic!(
                    "git daemon on port {} did not stop accepting connections",
                    self.port
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Every descendant pid of `pid` (children, grandchildren, ...), read from
/// procfs. Linux-only; `kill_and_wait` is the only caller and falls back to
/// a portable kill on every other platform.
#[cfg(target_os = "linux")]
fn descendant_pids(pid: u32) -> Vec<u32> {
    let mut result = Vec::new();
    let mut frontier = vec![pid];
    while let Some(p) = frontier.pop() {
        let children_path = format!("/proc/{p}/task/{p}/children");
        if let Ok(contents) = std::fs::read_to_string(&children_path) {
            for token in contents.split_whitespace() {
                if let Ok(child) = token.parse::<u32>() {
                    result.push(child);
                    frontier.push(child);
                }
            }
        }
    }
    result
}

/// Reap `child` with a bound rather than a blocking `wait()`: if a kill
/// attempt somehow failed to land, a blocking wait would hang this test --
/// and therefore the whole suite -- instead of failing it loudly. Giving up
/// after the deadline is safe: `Drop`/`kill_and_wait`'s callers only rely on
/// this to avoid leaking a zombie, never on it actually completing before
/// they move on.
fn reap_bounded(child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => {
                if Instant::now() >= deadline {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

impl Drop for GitDaemonFixture {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        {
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &self.child.id().to_string()])
                .status();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = self.child.kill();
        }
        reap_bounded(&mut self.child, Duration::from_secs(2));
    }
}

/// `git` exports these to its own subprocesses (hooks, `rebase --exec`) to pin
/// them to the invoking repository. A test run *from* such a context — most
/// obviously this repo's own `pre-push` hook — would otherwise inherit them,
/// and every `git` below would retarget the real repo instead of the scratch
/// one, firing the real hooks. Always spawn git through this.
pub fn git_command() -> Command {
    let mut cmd = Command::new("git");
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_PREFIX",
        "GIT_NAMESPACE",
        "GIT_CEILING_DIRECTORIES",
    ] {
        cmd.env_remove(var);
    }
    cmd
}

pub fn free_loopback_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind ephemeral port");
    listener
        .local_addr()
        .expect("failed to read local addr")
        .port()
}

/// Poll until `port` accepts connections. Returns `false` (rather than
/// panicking) if the daemon exits first or the deadline passes, so the caller
/// can retry on a different port.
pub fn wait_for_port(port: u16, child: &mut Child) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        // A daemon that already exited is never going to listen -- fail this
        // attempt immediately instead of burning the whole deadline.
        if matches!(child.try_wait(), Ok(Some(_))) {
            return false;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Run a git command in `dir`, panicking with stderr on failure.
pub fn run_git(dir: &Path, args: &[&str]) {
    let output = git_command()
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to execute `git {}`: {e}", args.join(" ")));
    assert!(
        output.status.success(),
        "`git {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}
