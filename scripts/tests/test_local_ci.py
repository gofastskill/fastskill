"""Exercise local gate orchestration without compiling Rust or accessing networks."""

import os
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class LocalCiTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "scripts").mkdir()
        (self.root / "webdocs").mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        for name in ("check-local-ci.sh", "run-tests.sh"):
            shutil.copy(ROOT / "scripts" / name, self.root / "scripts" / name)
        for name in ("check-source-size.sh", "smoke-binary.sh"):
            (self.root / "scripts" / name).write_text(
                'echo "bash ' + name + ' $*" >> "$COMMAND_LOG"\n'
                '[[ "$FAIL_GATE" != "' + name + '" ]]\n'
            )
        stub = '''#!/bin/bash
name=${0##*/}
echo "$name $*" >> "$COMMAND_LOG"
[[ "$name $*" != "$FAIL_GATE" ]] || exit 23
case "$name $*" in
  'rustup component list --installed') echo llvm-tools-x86_64-unknown-linux-gnu ;;
  'node --version') echo v22.0.0 ;;
  'pnpm --version') echo 10.21.0 ;;
  'git status '*) printf '%s' "$DIRTY_RUST" ;;
  'cargo nextest run --all-features '*)
    echo 'PASS safe_extract'
    echo 'PASS test_safe_join'
    echo 'Summary [ 1.000s] 2 tests run: 2 passed, 0 failed, 0 skipped'
    ;;
esac
exit 0
'''
        for name in ("cargo", "cargo-nextest", "cargo-llvm-cov", "rustup",
                     "python3", "pnpm", "node", "git"):
            target = self.bin / name
            target.write_text(stub)
            target.chmod(0o755)
        self.log = self.root / "commands.log"
        # Do not inherit a developer's Corepack shim or other package managers.
        self.env = dict(os.environ, PATH=f"{self.bin}:/usr/bin:/bin",
                        COMMAND_LOG=str(self.log), FAIL_GATE="", DIRTY_RUST="",
                        TMPDIR=str(self.root))

    def run_gate(self, failure="", dirty=""):
        return subprocess.run(
            ["bash", str(self.root / "scripts/check-local-ci.sh"), "target-branch"],
            env=dict(self.env, FAIL_GATE=failure, DIRTY_RUST=dirty),
            capture_output=True, text=True,
        )

    def test_success_runs_required_gates(self):
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stderr)
        commands = self.log.read_text()
        for command in (
            "cargo fmt --all -- --check", "check-source-size.sh",
            "cargo clippy --workspace --all-targets --all-features",
            "cargo build --all-features", "smoke-binary.sh",
            "cargo nextest run --retries 3 --fail-fast -E not test(install_e2e_tests)",
            "cargo llvm-cov clean --workspace",
            "cargo llvm-cov nextest --all-features -E not test(install_e2e_tests) --no-report",
            "check-modified-rust-coverage.py target-branch",
            "pnpm install --frozen-lockfile", "pnpm check:content", "pnpm lint",
            "pnpm typecheck", "pnpm build", "pnpm check:export",
        ):
            self.assertIn(command, commands)
        self.assertIn("remain remote checks", result.stderr)

    def test_required_failures_propagate(self):
        for command in (
            "git rev-parse --verify target-branch^{commit}",
            "cargo fmt --all -- --check", "check-source-size.sh",
            "cargo clippy --workspace --all-targets --all-features",
            "cargo build --all-features", "smoke-binary.sh",
            "cargo nextest run --retries 3 --fail-fast -E not test(install_e2e_tests)",
            "cargo llvm-cov clean --workspace",
            "cargo llvm-cov nextest --all-features -E not test(install_e2e_tests) --no-report",
            "pnpm install --frozen-lockfile", "pnpm check:content", "pnpm lint",
            "pnpm typecheck", "pnpm build", "pnpm check:export",
        ):
            with self.subTest(command=command):
                self.assertNotEqual(self.run_gate(command).returncode, 0)

    def test_coverage_failure_blocks_docs(self):
        # Coverage report paths are generated at runtime; intercept the checker.
        target = self.bin / "python3"
        target.write_text(target.read_text().replace(
            'case "$name $*" in',
            '[[ "$1" != scripts/check-modified-rust-coverage.py ]] || exit 42\n'
            'case "$name $*" in'))
        result = self.run_gate()
        self.assertEqual(result.returncode, 42)
        self.assertNotIn("pnpm install", self.log.read_text())

    def test_audit_is_advisory(self):
        result = self.run_gate("pnpm audit --prod --audit-level high")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("advisory", result.stderr)

    def test_corepack_uses_project_pin_from_webdocs(self):
        corepack = self.bin / "corepack"
        corepack.write_text(
            '#!/bin/bash\n'
            '[[ "$1" == pnpm && "${PWD##*/}" == webdocs ]] || exit 71\n'
            'shift\n'
            'echo "corepack-project $*" >> "$COMMAND_LOG"\n'
            'exec pnpm "$@"\n'
        )
        corepack.chmod(0o755)
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stderr)
        commands = self.log.read_text()
        self.assertIn("corepack-project --version", commands)
        self.assertIn("corepack-project install --frozen-lockfile", commands)

    def test_uncommitted_rust_is_not_silently_omitted(self):
        result = self.run_gate(dirty="?? crates/fastskill-core/src/new.rs")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("commit Rust changes", result.stderr)
        self.assertNotIn("cargo build", self.log.read_text())

    def test_entrypoint_propagates_gate_failure(self):
        result = subprocess.run(
            ["bash", str(self.root / "scripts/run-tests.sh"), "--base", "target-branch", "-f", "json"],
            env=dict(self.env, FAIL_GATE="cargo build --all-features"),
            capture_output=True, text=True,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('"status": "completed"', result.stdout)

    def test_entrypoint_json_is_valid_and_runs_all_features(self):
        result = subprocess.run(
            ["bash", str(self.root / "scripts/run-tests.sh"), "--base", "target-branch", "-f", "json"],
            env=self.env, capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report["exit_code"], 0)
        self.assertEqual(report["test_statistics"]["passed"], 2)
        self.assertIn("cargo nextest run --all-features --retries 3 --fail-fast -E not test(install_e2e_tests)",
                      self.log.read_text())

    def test_missing_argument(self):
        result = subprocess.run(
            ["bash", str(self.root / "scripts/run-tests.sh"), "--base"],
            env=self.env, capture_output=True, text=True,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Missing value", result.stderr)


if __name__ == "__main__":
    unittest.main()
