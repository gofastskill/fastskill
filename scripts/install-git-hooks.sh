#!/bin/bash
set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}🔧 Installing Git Hooks for Security Validation${NC}"
echo ""

# Check if we're in a git repository
if [ ! -d .git ]; then
    echo "❌ Not in a git repository. Run this from the project root."
    exit 1
fi

# Install pre-commit hook
echo "Installing pre-commit hook..."
cat > .git/hooks/pre-commit << 'EOF'
#!/bin/bash
set -e

echo "🔒 Running security validation..."

# 0. Check formatting (match run-tests.sh)
echo "  → Running cargo fmt --check..."
if ! cargo fmt --check 2>&1; then
  echo "❌ Format check failed. Run 'cargo fmt' to fix."
  exit 1
fi

# 1. Run Cargo Clippy with security lints
echo "  → Running Clippy with security lints..."
if ! cargo clippy --all-targets --all-features -- \
  -W clippy::suspicious \
  -W clippy::correctness \
  -D warnings 2>&1 | head -50; then
  echo "❌ Clippy found issues"
  exit 1
fi

# 2. Run security-focused tests
echo "  → Running security tests..."
if ! cargo test --lib security 2>&1 | tail -20; then
  echo "❌ Security tests failed"
  exit 1
fi

# 3. Check for common security patterns
echo "  → Checking for security patterns..."
ISSUES=0

# Check for unsafe blocks without SAFETY comments
UNSAFE_FILES=$(git diff --cached --name-only | grep -E '\.rs$' || true)
if [ -n "$UNSAFE_FILES" ]; then
  for file in $UNSAFE_FILES; do
    if grep -n "unsafe {" "$file" | grep -v "// SAFETY:" > /dev/null 2>&1; then
      echo "⚠️  Found undocumented unsafe block in $file"
      ISSUES=$((ISSUES + 1))
    fi
  done
fi

# Check for direct file operations in HTTP handlers without validation
HTTP_FILES=$(git diff --cached --name-only | grep -E 'src/http/.*\.rs$' || true)
if [ -n "$HTTP_FILES" ]; then
  for file in $HTTP_FILES; do
    if git diff --cached "$file" | grep -E "^\+.*fs::(read|write|remove)" | grep -v "canonical" | grep -v "validate" | grep -v "safe_" > /dev/null 2>&1; then
      echo "⚠️  Found file operation in $file that may need validation"
      echo "    Ensure user-provided paths use security::path utilities"
    fi
  done
fi

if [ $ISSUES -gt 0 ]; then
  echo "❌ Pre-commit validation failed with $ISSUES issue(s)"
  echo ""
  echo "Fix the issues above or use 'git commit --no-verify' to skip (not recommended)"
  exit 1
fi

echo "✅ Security validation passed"
exit 0
EOF

chmod +x .git/hooks/pre-commit
echo -e "${GREEN}✅ Pre-commit hook installed${NC}"
echo ""

# Optionally install pre-push hook
read -p "Install pre-push hook with CodeQL? (requires CodeQL CLI) [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    cat > .git/hooks/pre-push << 'EOF'
#!/bin/bash
set -e

echo "🔒 Running comprehensive security scan..."

# Only run if CodeQL is installed
if ! command -v codeql &> /dev/null; then
  echo "⚠️  CodeQL not installed, skipping deep scan"
  echo "    Install CodeQL to enable pre-push security scanning"
  echo "    See docs/SECURITY_VALIDATION.md for instructions"
  exit 0
fi

# Quick security scan with limited queries for faster feedback
TEMP_DB=$(mktemp -d)
trap "rm -rf $TEMP_DB" EXIT

echo "  → Creating CodeQL database..."
if ! codeql database create "$TEMP_DB/db" \
  --language=rust \
  --source-root=. \
  --threads=0 \
  --quiet; then
  echo "❌ CodeQL database creation failed"
  exit 1
fi

echo "  → Running security queries..."
if ! codeql database analyze "$TEMP_DB/db" \
  --format=json \
  --output="$TEMP_DB/results.json" \
  --sarif-category=security \
  ~/.local/codeql-repo/rust/ql/src/codeql-suites/rust-security.qls; then
  echo "❌ CodeQL analysis failed"
  exit 1
fi

# Check for new vulnerabilities
VULN_COUNT=$(jq '[.runs[].results[] | select(.level == "error")] | length' "$TEMP_DB/results.json" 2>/dev/null || echo "0")
if [ "$VULN_COUNT" -gt 0 ]; then
  echo "❌ Found $VULN_COUNT critical security issue(s):"
  jq -r '.runs[].results[] | select(.level == "error") | "  - \(.message.text) at \(.locations[0].physicalLocation.artifactLocation.uri):\(.locations[0].physicalLocation.region.startLine)"' "$TEMP_DB/results.json" | head -10
  echo ""
  echo "Run './scripts/run-codeql-scan.sh' for full details"
  echo "Use 'git push --no-verify' to skip (not recommended)"
  exit 1
fi

echo "✅ No critical security issues found"
exit 0
EOF

    chmod +x .git/hooks/pre-push
    echo -e "${GREEN}✅ Pre-push hook installed${NC}"
else
    echo "Skipping pre-push hook installation"
fi

echo ""
echo -e "${GREEN}✅ Git hooks installation complete!${NC}"
echo ""
echo "Hooks installed:"
echo "  ✓ pre-commit: Fast security checks before each commit"
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "  ✓ pre-push: Deep CodeQL scan before pushing"
fi
echo ""
echo "To bypass hooks (not recommended): git commit --no-verify"
echo "To uninstall: rm .git/hooks/pre-commit .git/hooks/pre-push"
