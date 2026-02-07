# Security Validation Process

## Overview

This document outlines the security validation processes for the FastSkill project, including local CodeQL analysis and pre-commit hooks to catch security vulnerabilities before they reach CI/CD.

## Recent Security Fixes

### Path Injection Vulnerabilities (Fixed)

We identified and fixed 30 path injection vulnerabilities across the codebase where user-provided data was used in path expressions without proper validation. These have been resolved by:

1. **Created centralized security module** (`src/security/path.rs`):
   - `sanitize_path_component()` - Filters dangerous characters from path components
   - `validate_path_component()` - Validates path components for traversal attempts
   - `validate_path_within_root()` - Ensures paths stay within allowed directories
   - `safe_join()` - Safely joins user paths to root directories

2. **Applied validation across all affected modules**:
   - `src/http/handlers/manifest.rs` - 8 vulnerabilities fixed
   - `src/http/handlers/skill_storage.rs` - 8 vulnerabilities fixed
   - `src/http/handlers/registry_publish.rs` - 4 vulnerabilities fixed
   - `src/core/registry/staging.rs` - 3 vulnerabilities fixed
   - `src/core/registry_index.rs` - 2 vulnerabilities fixed
   - `src/http/handlers/skills.rs` - 1 vulnerability fixed
   - `src/core/manifest.rs` - 1 vulnerability fixed
   - `src/core/lock.rs` - 1 vulnerability fixed

## Local CodeQL Setup

### Prerequisites

1. **Install CodeQL CLI**:
   ```bash
   # Download CodeQL CLI
   wget https://github.com/github/codeql-cli-binaries/releases/latest/download/codeql-linux64.zip
   unzip codeql-linux64.zip -d ~/.local/
   export PATH="$HOME/.local/codeql:$PATH"

   # Add to ~/.bashrc or ~/.zshrc for persistence
   echo 'export PATH="$HOME/.local/codeql:$PATH"' >> ~/.bashrc
   ```

2. **Clone CodeQL Standard Libraries**:
   ```bash
   git clone https://github.com/github/codeql.git ~/.local/codeql-repo
   ```

### Running CodeQL Locally

1. **Create CodeQL Database**:
   ```bash
   # From project root
   codeql database create codeql-db \
     --language=rust \
     --source-root=. \
     --overwrite
   ```

2. **Run Security Queries**:
   ```bash
   # Run all security queries
   codeql database analyze codeql-db \
     --format=sarif-latest \
     --output=codeql-results.sarif \
     ~/.local/codeql-repo/rust/ql/src/codeql-suites/rust-security-extended.qls

   # View results in JSON format
   codeql database analyze codeql-db \
     --format=json \
     --output=codeql-results.json \
     ~/.local/codeql-repo/rust/ql/src/codeql-suites/rust-security-extended.qls
   ```

3. **Run Specific Query (e.g., path injection)**:
   ```bash
   codeql database analyze codeql-db \
     --format=sarif-latest \
     --output=path-injection-results.sarif \
     ~/.local/codeql-repo/rust/ql/src/Security/CWE-022/PathInjection.ql
   ```

### Understanding Results

CodeQL results are in SARIF format. To view them:

```bash
# Install sarif-tools for better viewing
pip install sarif-tools

# View results
sarif summary codeql-results.sarif
sarif ls codeql-results.sarif

# Get detailed report
sarif html codeql-results.sarif -o codeql-report.html
```

## Git Hooks for Security Validation

### Pre-commit Hook Setup

Create a pre-commit hook to run security checks before each commit:

```bash
# Create pre-commit hook
cat > .git/hooks/pre-commit << 'EOF'
#!/bin/bash

set -e

echo "🔒 Running security validation..."

# 1. Run Cargo Clippy with security lints
echo "  → Running Clippy..."
cargo clippy --all-targets --all-features -- \
  -W clippy::suspicious \
  -W clippy::correctness \
  -W clippy::complexity \
  -D warnings 2>&1 | head -50

# 2. Run security-focused tests
echo "  → Running security tests..."
cargo test --lib security 2>&1 | tail -20

# 3. Check for common security patterns
echo "  → Checking for security patterns..."
ISSUES=0

# Check for unsafe blocks without documentation
if git diff --cached --name-only | grep -E '\.rs$' | xargs grep -n "unsafe {" | grep -v "// SAFETY:"; then
  echo "❌ Found undocumented unsafe blocks"
  ISSUES=$((ISSUES + 1))
fi

# Check for direct file operations without validation
if git diff --cached --name-only | grep -E 'src/http/.*\.rs$' | xargs grep -E "(fs::read|fs::write|File::open)\s*\(" | grep -v "canonical" | grep -v "validate"; then
  echo "⚠️  Found file operations that may need validation"
  echo "    Ensure all user-provided paths are validated with security::path utilities"
fi

# Check for command execution
if git diff --cached --diff-filter=AM --name-only | grep -E '\.rs$' | xargs grep -l "Command::new" 2>/dev/null; then
  echo "⚠️  Found command execution - ensure input is sanitized"
fi

if [ $ISSUES -gt 0 ]; then
  echo "❌ Security check failed with $ISSUES issue(s)"
  exit 1
fi

echo "✅ Security validation passed"
exit 0
EOF

# Make executable
chmod +x .git/hooks/pre-commit
```

### Pre-push Hook with CodeQL (Optional)

For more thorough validation before pushing:

```bash
cat > .git/hooks/pre-push << 'EOF'
#!/bin/bash

set -e

echo "🔒 Running comprehensive security scan..."

# Only run if CodeQL is installed
if ! command -v codeql &> /dev/null; then
  echo "⚠️  CodeQL not installed, skipping deep scan"
  exit 0
fi

# Create temporary database
TEMP_DB=$(mktemp -d)
trap "rm -rf $TEMP_DB" EXIT

echo "  → Creating CodeQL database..."
codeql database create "$TEMP_DB/db" \
  --language=rust \
  --source-root=. \
  --threads=0 \
  --quiet || {
    echo "❌ CodeQL database creation failed"
    exit 1
  }

echo "  → Running security queries..."
codeql database analyze "$TEMP_DB/db" \
  --format=json \
  --output="$TEMP_DB/results.json" \
  --sarif-category=security \
  ~/.local/codeql-repo/rust/ql/src/codeql-suites/rust-security-and-quality.qls || {
    echo "❌ CodeQL analysis failed"
    exit 1
  }

# Check for new vulnerabilities
VULN_COUNT=$(jq '[.runs[].results[] | select(.level == "error" or .level == "warning")] | length' "$TEMP_DB/results.json")
if [ "$VULN_COUNT" -gt 0 ]; then
  echo "❌ Found $VULN_COUNT security issue(s):"
  jq -r '.runs[].results[] | "  - \(.message.text) at \(.locations[0].physicalLocation.artifactLocation.uri):\(.locations[0].physicalLocation.region.startLine)"' "$TEMP_DB/results.json" | head -10
  echo ""
  echo "Run locally with: codeql database analyze"
  exit 1
fi

echo "✅ No security issues found"
exit 0
EOF

chmod +x .git/hooks/pre-push
```

## CI/CD Integration

### GitHub Actions Workflow

The project already has CodeQL enabled in `.github/workflows/codeql-analysis.yml`. Ensure it's configured to:

1. Run on all pull requests
2. Block merges if critical vulnerabilities are found
3. Upload results to GitHub Security tab

### Required Status Checks

Add CodeQL as a required status check:

1. Go to repository Settings → Branches
2. Edit branch protection rules for `main`
3. Add "CodeQL" to required status checks
4. Enable "Require branches to be up to date before merging"

## Security Best Practices

### Path Validation

Always validate user-provided paths using the security module:

```rust
use crate::security::{validate_path_component, safe_join};

// Validate individual path components
validate_path_component(&user_input)?;

// Safely join paths
let safe_path = safe_join(&root_dir, &user_provided_path)?;

// Canonicalize existing paths
let canonical = existing_path.canonicalize()?;
if !canonical.starts_with(&root_dir) {
    return Err("Path escapes root");
}
```

### Command Execution

When executing commands, always validate inputs:

```rust
use crate::security::validate_path_component;

// Validate before using in commands
validate_path_component(&skill_id)?;
let output = Command::new("fastskill")
    .arg("install")
    .arg(&skill_id)  // Already validated
    .output()?;
```

### File Operations

Use security utilities for all file operations in HTTP handlers:

```rust
// Bad - user input directly in path
let path = base_dir.join(&user_input);
fs::write(path, data)?;

// Good - validated and canonicalized
use crate::security::safe_join;
let safe_path = safe_join(&base_dir, &user_input)?;
if safe_path.exists() {
    let canonical = safe_path.canonicalize()?;
    if canonical.starts_with(&base_dir.canonicalize()?) {
        fs::write(canonical, data)?;
    }
}
```

## Regular Security Audits

### Weekly Checks

Run these commands weekly:

```bash
# Update dependencies and check for vulnerabilities
cargo update
cargo audit

# Run Clippy with security lints
cargo clippy --all-targets --all-features -- -W clippy::suspicious

# Run all tests including security tests
cargo test
```

### Monthly Deep Scan

Perform a comprehensive security scan monthly:

```bash
# Full CodeQL scan
./scripts/run-codeql-scan.sh

# Dependency audit with fix suggestions
cargo audit fix --dry-run

# Check for outdated dependencies
cargo outdated
```

## Troubleshooting

### CodeQL Database Creation Fails

If database creation fails:

```bash
# Ensure the project builds
cargo clean
cargo build

# Try with verbose output
codeql database create codeql-db --language=rust --source-root=. --verbose
```

### False Positives

If CodeQL reports false positives:

1. Verify the code is actually safe
2. Add inline comments explaining why it's safe
3. Consider using `#[allow(clippy::...)]` for specific cases
4. Document in code reviews

### Performance Issues

For faster local scans:

```bash
# Run only critical security queries
codeql database analyze codeql-db \
  --format=sarif-latest \
  --output=results.sarif \
  --threads=4 \
  ~/.local/codeql-repo/rust/ql/src/codeql-suites/rust-security.qls
```

## Resources

- [CodeQL Documentation](https://codeql.github.com/docs/)
- [Rust CodeQL Queries](https://github.com/github/codeql/tree/main/rust)
- [OWASP Top 10](https://owasp.org/www-project-top-ten/)
- [Rust Security Working Group](https://www.rust-lang.org/governance/wgs/wg-security)
