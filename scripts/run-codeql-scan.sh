#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}🔒 FastSkill CodeQL Security Scanner${NC}"
echo ""

# Check if CodeQL is installed
if ! command -v codeql &> /dev/null; then
    echo -e "${RED}❌ CodeQL CLI not found${NC}"
    echo ""
    echo "Install CodeQL CLI:"
    echo "  1. Download: wget https://github.com/github/codeql-cli-binaries/releases/latest/download/codeql-linux64.zip"
    echo "  2. Extract: unzip codeql-linux64.zip -d ~/.local/"
    echo "  3. Add to PATH: export PATH=\"\$HOME/.local/codeql:\$PATH\""
    echo "  4. Clone queries: git clone https://github.com/github/codeql.git ~/.local/codeql-repo"
    echo ""
    echo "See docs/SECURITY_VALIDATION.md for detailed instructions"
    exit 1
fi

# Check for CodeQL queries
CODEQL_QUERIES="$HOME/.local/codeql-repo"
if [ ! -d "$CODEQL_QUERIES" ]; then
    echo -e "${YELLOW}⚠️  CodeQL queries not found at $CODEQL_QUERIES${NC}"
    echo "Cloning CodeQL queries repository..."
    git clone --depth 1 https://github.com/github/codeql.git "$CODEQL_QUERIES"
fi

# Configuration
DB_DIR="codeql-db"
RESULTS_DIR="codeql-results"
SUITE="${1:-security-extended}"

echo "Configuration:"
echo "  Database: $DB_DIR"
echo "  Results: $RESULTS_DIR"
echo "  Suite: $SUITE"
echo ""

# Clean old database
if [ -d "$DB_DIR" ]; then
    echo -e "${YELLOW}🗑️  Removing old database...${NC}"
    rm -rf "$DB_DIR"
fi

# Create results directory
mkdir -p "$RESULTS_DIR"

# Step 1: Create database
echo -e "${GREEN}📊 Creating CodeQL database...${NC}"
codeql database create "$DB_DIR" \
    --language=rust \
    --source-root=. \
    --threads=0 \
    --overwrite

echo -e "${GREEN}✅ Database created${NC}"
echo ""

# Step 2: Run analysis
echo -e "${GREEN}🔍 Running security analysis...${NC}"

case $SUITE in
    security)
        QUERY_SUITE="$CODEQL_QUERIES/rust/ql/src/codeql-suites/rust-security.qls"
        ;;
    security-extended)
        QUERY_SUITE="$CODEQL_QUERIES/rust/ql/src/codeql-suites/rust-security-extended.qls"
        ;;
    quality)
        QUERY_SUITE="$CODEQL_QUERIES/rust/ql/src/codeql-suites/rust-code-scanning.qls"
        ;;
    path-injection)
        QUERY_SUITE="$CODEQL_QUERIES/rust/ql/src/Security/CWE-022/PathInjection.ql"
        ;;
    *)
        echo -e "${RED}❌ Unknown suite: $SUITE${NC}"
        echo "Available suites: security, security-extended, quality, path-injection"
        exit 1
        ;;
esac

# Run analysis with both SARIF and JSON output
codeql database analyze "$DB_DIR" \
    --format=sarif-latest \
    --output="$RESULTS_DIR/results.sarif" \
    "$QUERY_SUITE"

codeql database analyze "$DB_DIR" \
    --format=json \
    --output="$RESULTS_DIR/results.json" \
    "$QUERY_SUITE"

echo -e "${GREEN}✅ Analysis complete${NC}"
echo ""

# Step 3: Display results
echo -e "${GREEN}📋 Results Summary:${NC}"
echo ""

# Count issues by severity
CRITICAL=$(jq '[.runs[].results[] | select(.level == "error")] | length' "$RESULTS_DIR/results.json")
WARNING=$(jq '[.runs[].results[] | select(.level == "warning")] | length' "$RESULTS_DIR/results.json")
NOTE=$(jq '[.runs[].results[] | select(.level == "note")] | length' "$RESULTS_DIR/results.json")

echo "  Critical: $CRITICAL"
echo "  Warning:  $WARNING"
echo "  Note:     $NOTE"
echo ""

# Display issues if any
if [ "$CRITICAL" -gt 0 ] || [ "$WARNING" -gt 0 ]; then
    echo -e "${RED}❌ Security issues found:${NC}"
    echo ""

    # Show first 20 issues
    jq -r '.runs[].results[] |
        select(.level == "error" or .level == "warning") |
        "  [\(.level | ascii_upcase)] \(.message.text)\n    → \(.locations[0].physicalLocation.artifactLocation.uri):\(.locations[0].physicalLocation.region.startLine)"' \
        "$RESULTS_DIR/results.json" | head -40

    echo ""
    echo -e "${YELLOW}Full results saved to:${NC}"
    echo "  - $RESULTS_DIR/results.sarif (SARIF format)"
    echo "  - $RESULTS_DIR/results.json (JSON format)"
    echo ""

    # Generate HTML report if sarif-tools is installed
    if command -v sarif &> /dev/null; then
        echo -e "${GREEN}📄 Generating HTML report...${NC}"
        sarif html "$RESULTS_DIR/results.sarif" -o "$RESULTS_DIR/report.html"
        echo "  - $RESULTS_DIR/report.html (HTML report)"
        echo ""
    fi

    exit 1
else
    echo -e "${GREEN}✅ No security issues found!${NC}"
    echo ""
fi

# Cleanup option
read -p "Clean up database? (y/N) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    rm -rf "$DB_DIR"
    echo -e "${GREEN}✅ Database cleaned up${NC}"
fi
