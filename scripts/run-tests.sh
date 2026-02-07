#!/bin/bash

# Newton Test Runner Script
# Runs tests with cargo-nextest, captures results, and generates statistics
#
# Usage: ./run-tests.sh [OPTIONS]
#
# Options:
#   -o, --output FILE    Output markdown report file (default: test_results.md)
#   -j, --json FILE      JSON results file (default: test_results.json)
#   -h, --help           Show this help message

set -e  # Exit on any error

# Default values
OUTPUT_FILE="test_results.md"
JSON_FILE="test_results.json"
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print error and exit
error_exit() {
    echo -e "${RED}Error: $1${NC}" >&2
    echo "Usage: $0 [OPTIONS]" >&2
    echo "" >&2
    echo "Options:" >&2
    echo "  -o, --output FILE    Output markdown report file (default: test_results.md)" >&2
    echo "  -j, --json FILE      JSON results file (default: test_results.json)" >&2
    echo "  -h, --help           Show this help message" >&2
    exit 1
}

# Function to check if command exists
check_command() {
    local cmd=$1
    local description=$2
    if ! command -v "$cmd" >/dev/null 2>&1; then
        error_exit "$description ($cmd) is not installed or not in PATH. Please install it first."
    fi
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -o|--output)
            OUTPUT_FILE="$2"
            shift 2
            ;;
        -j|--json)
            JSON_FILE="$2"
            shift 2
            ;;
        -h|--help)
            echo "Newton Test Runner Script"
            echo ""
            echo "Runs tests with cargo-nextest, captures results, and generates statistics"
            echo ""
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  -o, --output FILE    Output markdown report file (default: test_results.md)"
            echo "  -j, --json FILE      JSON results file (default: test_results.json)"
            echo "  -h, --help           Show this help message"
            echo ""
            echo "Requirements:"
            echo "  - cargo-nextest: Fast test runner for Rust"
            echo ""
            echo "Install requirements:"
            echo "  cargo install cargo-nextest"
            exit 0
            ;;
        *)
            error_exit "Unknown option: $1"
            ;;
    esac
done

# Check dependencies
echo -e "${YELLOW}Checking dependencies...${NC}" >&2

check_command "cargo" "Cargo (Rust package manager)"
check_command "cargo-nextest" "cargo-nextest (install with: cargo install cargo-nextest)"

echo -e "${GREEN}All dependencies found!${NC}" >&2
echo "" >&2

# Change to the newton directory (assuming script is run from there)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NEWTON_DIR="$(dirname "$SCRIPT_DIR")"

echo -e "${YELLOW}Running tests in: $NEWTON_DIR${NC}" >&2
cd "$NEWTON_DIR"

# Run tests and capture output
echo -e "${YELLOW}Running tests with cargo-nextest...${NC}" >&2

# First run security-sensitive tests explicitly and ensure they exist
echo -e "${YELLOW}Checking for ZIP slip security tests...${NC}" >&2

# Run just the security tests by running all tests and filtering
FULL_TEST_OUTPUT=$(cargo nextest run --all-features 2>&1)
FULL_EXIT_CODE=$?

# Check if security tests ran and passed
SECURITY_TESTS_PASSED=true
SECURITY_TESTS_FOUND=false
ZIP_SLIP_TESTS_FOUND=false
ZIP_SLIP_TESTS_PASSED=true
PATH_SECURITY_TESTS_FOUND=false
PATH_SECURITY_TESTS_PASSED=true

# Check for ZIP slip security test patterns
if echo "$FULL_TEST_OUTPUT" | grep -q "safe_extract"; then
    ZIP_SLIP_TESTS_FOUND=true
    # Check if any of the ZIP slip tests failed
    if echo "$FULL_TEST_OUTPUT" | grep "safe_extract" | grep -q "FAIL"; then
        ZIP_SLIP_TESTS_PASSED=false
        SECURITY_TESTS_PASSED=false
    fi
fi

if echo "$FULL_TEST_OUTPUT" | grep -q "test_add_from_zip_rejects_path_traversal"; then
    ZIP_SLIP_TESTS_FOUND=true
    if echo "$FULL_TEST_OUTPUT" | grep "test_add_from_zip_rejects_path_traversal" | grep -q "FAIL"; then
        ZIP_SLIP_TESTS_PASSED=false
        SECURITY_TESTS_PASSED=false
    fi
fi

if echo "$FULL_TEST_OUTPUT" | grep -q "test_extract_zip_to_temp_rejects_path_traversal"; then
    ZIP_SLIP_TESTS_FOUND=true
    if echo "$FULL_TEST_OUTPUT" | grep "test_extract_zip_to_temp_rejects_path_traversal" | grep -q "FAIL"; then
        ZIP_SLIP_TESTS_PASSED=false
        SECURITY_TESTS_PASSED=false
    fi
fi

# Check for path security test patterns
if echo "$FULL_TEST_OUTPUT" | grep -q "test_validate_path_component"; then
    PATH_SECURITY_TESTS_FOUND=true
    if echo "$FULL_TEST_OUTPUT" | grep "test_validate_path_component" | grep -q "FAIL"; then
        PATH_SECURITY_TESTS_PASSED=false
        SECURITY_TESTS_PASSED=false
    fi
fi

if echo "$FULL_TEST_OUTPUT" | grep -q "test_safe_join"; then
    PATH_SECURITY_TESTS_FOUND=true
    if echo "$FULL_TEST_OUTPUT" | grep "test_safe_join" | grep -q "FAIL"; then
        PATH_SECURITY_TESTS_PASSED=false
        SECURITY_TESTS_PASSED=false
    fi
fi

if echo "$FULL_TEST_OUTPUT" | grep -q "test_validate_path_within_root"; then
    PATH_SECURITY_TESTS_FOUND=true
    if echo "$FULL_TEST_OUTPUT" | grep "test_validate_path_within_root" | grep -q "FAIL"; then
        PATH_SECURITY_TESTS_PASSED=false
        SECURITY_TESTS_PASSED=false
    fi
fi

# Set overall security tests found flag
if [ "$ZIP_SLIP_TESTS_FOUND" = true ] || [ "$PATH_SECURITY_TESTS_FOUND" = true ]; then
    SECURITY_TESTS_FOUND=true
fi

if [ "$SECURITY_TESTS_FOUND" = false ]; then
    echo -e "${RED}No security tests found!${NC}" >&2
    echo -e "${YELLOW}Expected tests matching patterns:${NC}" >&2
    echo -e "${YELLOW}  - ZIP slip: safe_extract, test_add_from_zip_rejects_path_traversal, test_extract_zip_to_temp_rejects_path_traversal${NC}" >&2
    echo -e "${YELLOW}  - Path security: test_validate_path_component, test_safe_join, test_validate_path_within_root${NC}" >&2
    OVERALL_STATUS="FAILED"
    EXIT_CODE=1
    TEST_OUTPUT="$FULL_TEST_OUTPUT"
elif [ "$SECURITY_TESTS_PASSED" = false ]; then
    echo -e "${RED}Security tests failed!${NC}" >&2
    OVERALL_STATUS="FAILED"
    EXIT_CODE=1
    TEST_OUTPUT="$FULL_TEST_OUTPUT"
else
    echo -e "${GREEN}Security tests passed!${NC}" >&2

    if [ $FULL_EXIT_CODE -eq 0 ]; then
        OVERALL_STATUS="PASSED"
        EXIT_CODE=0
        echo -e "${GREEN}All tests completed successfully!${NC}" >&2
    else
        OVERALL_STATUS="FAILED"
        EXIT_CODE=1
        echo -e "${RED}Some tests failed!${NC}" >&2
    fi
    TEST_OUTPUT="$FULL_TEST_OUTPUT"
fi

echo "" >&2

# Parse test results from output
echo -e "${YELLOW}Parsing test results...${NC}" >&2

# Check if there are compilation errors (no tests were run)
if echo "$TEST_OUTPUT" | grep -q "error\[" || echo "$TEST_OUTPUT" | grep -q "could not compile"; then
    echo -e "${YELLOW}Compilation errors detected - no tests could run${NC}" >&2
    PASSED=0
    FAILED=0
    SKIPPED=0
    TOTAL=0
    PASSING_PERCENTAGE=0
    STATS_AVAILABLE=false
    COMPILATION_FAILED=true
else
    COMPILATION_FAILED=false

    # Look for summary line like: "Summary [   0.039s] 20 tests run: 20 passed, 0 failed, 0 skipped"
    SUMMARY_LINE=$(echo "$TEST_OUTPUT" | grep "Summary.*tests run:" | head -1)

    if [ -n "$SUMMARY_LINE" ]; then
        # Extract numbers from summary line
        PASSED=$(echo "$SUMMARY_LINE" | sed -n 's/.* \([0-9]*\) passed.*/\1/p')
        FAILED=$(echo "$SUMMARY_LINE" | sed -n 's/.* \([0-9]*\) failed.*/\1/p')
        SKIPPED=$(echo "$SUMMARY_LINE" | sed -n 's/.* \([0-9]*\) skipped.*/\1/p')

        # If parsing failed, try alternative format
        if [ -z "$PASSED" ]; then
            # Try format: "Summary [   0.039s] 20 tests run: 20 passed (0 slow), 0 failed, 0 skipped"
            PASSED=$(echo "$SUMMARY_LINE" | sed -n 's/.*: \([0-9]*\) passed.*/\1/p')
            FAILED=$(echo "$SUMMARY_LINE" | sed -n 's/.* \([0-9]*\) failed.*/\1/p')
            SKIPPED=$(echo "$SUMMARY_LINE" | sed -n 's/.* \([0-9]*\) skipped.*/\1/p')
        fi

        # Calculate total and percentage
        TOTAL=$((PASSED + FAILED + SKIPPED))

        if [ "$TOTAL" -gt 0 ]; then
            PASSING_PERCENTAGE=$((PASSED * 100 / TOTAL))
        else
            PASSING_PERCENTAGE=0
        fi

        STATS_AVAILABLE=true
    else
        # Fallback: try to parse from individual test results
        PASSED_COUNT=$(echo "$TEST_OUTPUT" | grep -c "PASS\|✓")
        FAILED_COUNT=$(echo "$TEST_OUTPUT" | grep -c "FAIL\|✗")
        SKIPPED_COUNT=$(echo "$TEST_OUTPUT" | grep -c "SKIP")

        PASSED=${PASSED_COUNT:-0}
        FAILED=${FAILED_COUNT:-0}
        SKIPPED=${SKIPPED_COUNT:-0}
        TOTAL=$((PASSED + FAILED + SKIPPED))

        if [ "$TOTAL" -gt 0 ]; then
            PASSING_PERCENTAGE=$((PASSED * 100 / TOTAL))
        else
            PASSING_PERCENTAGE=0
        fi

        STATS_AVAILABLE=true
    fi
fi

# Get failed test names (if any)
FAILED_TESTS=""
if [ -n "$FAILED" ] && [ "$FAILED" -gt 0 ]; then
    # Extract failed test names from output
    FAILED_TESTS=$(echo "$TEST_OUTPUT" | grep -A 5 -B 1 "FAIL\|✗" | grep "^\s*[^-]*test.*" | sed 's/.*--- \(.*\) ---.*/\1/' | grep -v "^\s*$" | head -10)
fi

# Create structured JSON output
echo -e "${YELLOW}Generating JSON output...${NC}" >&2
if [ "$COMPILATION_FAILED" = true ]; then
    # Create JSON for compilation errors
    cat > "$JSON_FILE" << EOF
{
  "status": "compilation_failed",
  "timestamp": "$TIMESTAMP",
  "command": "$0",
  "exit_code": $EXIT_CODE,
  "test_statistics": {
    "total": 0,
    "passed": 0,
    "failed": 0,
    "skipped": 0,
    "passing_percentage": 0
  },
  "security_tests": {
    "zip_slip_tests_found": false,
    "zip_slip_tests_passed": false,
    "path_security_tests_found": false,
    "path_security_tests_passed": false
  }
}
EOF
else
    # Create JSON for successful test runs
    cat > "$JSON_FILE" << EOF
{
  "status": "completed",
  "timestamp": "$TIMESTAMP",
  "command": "$0",
  "exit_code": $EXIT_CODE,
  "test_statistics": {
    "total": $TOTAL,
    "passed": ${PASSED:-0},
    "failed": ${FAILED:-0},
    "skipped": ${SKIPPED:-0},
    "passing_percentage": $PASSING_PERCENTAGE
  },
  "security_tests": {
    "zip_slip_tests_found": $ZIP_SLIP_TESTS_FOUND,
    "zip_slip_tests_passed": $ZIP_SLIP_TESTS_PASSED,
    "path_security_tests_found": $PATH_SECURITY_TESTS_FOUND,
    "path_security_tests_passed": $PATH_SECURITY_TESTS_PASSED
  }
}
EOF
fi

# Generate comprehensive report
echo -e "${YELLOW}Generating report: $OUTPUT_FILE${NC}" >&2

{
    echo "# Newton Test Results Report"
    echo "Generated: $TIMESTAMP"
    echo "Command: $0"
    echo "Output File: $OUTPUT_FILE"
    echo "JSON File: $JSON_FILE"
    echo ""

    echo "## Overall Status"
    if [ "$COMPILATION_FAILED" = true ]; then
        echo "❌ **COMPILATION FAILED** - Code does not compile, tests cannot run"
    elif [ "$EXIT_CODE" -eq 0 ]; then
        echo "✅ **PASSED** - All tests completed successfully"
    else
        echo "❌ **FAILED** - Some tests failed"
    fi
    echo ""

    echo "## Test Statistics"
    if [ "$COMPILATION_FAILED" = true ]; then
        echo "- **Status:** Compilation failed - no tests executed"
        echo "- **Total Tests:** N/A"
        echo "- **Passed:** N/A"
        echo "- **Failed:** N/A"
        echo "- **Skipped:** N/A"
        echo "- **Passing Rate:** N/A"
    else
        echo "- **Total Tests:** $TOTAL"
        echo "- **Passed:** $PASSED"
        echo "- **Failed:** $FAILED"
        echo "- **Skipped:** $SKIPPED"
        echo "- **Passing Rate:** ${PASSING_PERCENTAGE}%"
    fi
    echo ""

    # Progress bar visualization
    if [ "$COMPILATION_FAILED" = true ]; then
        echo "## Progress Visualization"
        echo "\`\`\`"
        echo "[░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░] COMPILATION FAILED"
        echo "\`\`\`"
        echo ""
    elif [ "$TOTAL" -gt 0 ]; then
        echo "## Progress Visualization"
        BAR_WIDTH=30
        FILLED=$((PASSED * BAR_WIDTH / TOTAL))
        EMPTY=$((BAR_WIDTH - FILLED))

        echo "\`\`\`"
        printf "["
        for ((i=0; i<FILLED; i++)); do printf "█"; done
        for ((i=0; i<EMPTY; i++)); do printf "░"; done
        printf "] %d%% (%d/%d)\n" "$PASSING_PERCENTAGE" "$PASSED" "$TOTAL"
        echo "\`\`\`"
        echo ""
    else
        echo "## Progress Visualization"
        echo "\`\`\`"
        echo "[░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░] No tests found"
        echo "\`\`\`"
        echo ""
    fi

    # Failed tests section
    if [ -n "$FAILED_TESTS" ] && [ "$FAILED" -gt 0 ]; then
        echo "## Failed Tests"
        echo ""
        echo "The following tests failed:"
        echo ""
        echo "\`\`\`"
        echo "$FAILED_TESTS"
        echo "\`\`\`"
        echo ""
    fi

    # Test duration (if available in summary)
    DURATION_LINE=$(echo "$TEST_OUTPUT" | grep "Summary.*\[" | head -1)
    if [ -n "$DURATION_LINE" ]; then
        DURATION=$(echo "$DURATION_LINE" | sed -n 's/.*\[\s*\([0-9.]*\)s\].*/\1/p')
        if [ -n "$DURATION" ]; then
            echo "## Performance"
            echo "- **Test Duration:** ${DURATION}s"
            echo ""
        fi
    fi

    echo "## Files"
    echo "- **Raw Test Output:** \`$JSON_FILE\`"
    echo "- **Markdown Report:** \`$OUTPUT_FILE\`"
    echo ""

    echo "## Raw Test Output"
    echo "Complete test output is saved in: \`$JSON_FILE\`"
    echo ""
    echo "You can analyze it with standard Unix tools:"
    echo "\`\`\`bash"
    echo "# Count total tests"
    echo "grep -c 'PASS\\|FAIL\\|SKIP' $JSON_FILE"
    echo ""
    echo "# Show failed tests"
    echo "grep -A 2 -B 2 'FAIL' $JSON_FILE"
    echo "\`\`\`"

} > "$OUTPUT_FILE"

# Console output summary
echo -e "${GREEN}Report generated successfully!${NC}" >&2
echo "" >&2

if [ "$COMPILATION_FAILED" = true ]; then
    echo "📊 Test Summary:" >&2
    echo "  Status: COMPILATION FAILED - no tests executed" >&2
    echo -e "${RED}❌ Code does not compile. Check $OUTPUT_FILE for compilation errors.${NC}" >&2
else
    echo "📊 Test Summary:" >&2
    echo "  Total: $TOTAL tests" >&2
    echo "  Passed: $PASSED (${PASSING_PERCENTAGE}%)" >&2
    echo "  Failed: $FAILED" >&2
    echo "  Skipped: $SKIPPED" >&2
    echo "" >&2

    if [ "$EXIT_CODE" -eq 0 ]; then
        echo -e "${GREEN}✅ All tests passed!${NC}" >&2
    else
        echo -e "${RED}❌ Some tests failed. Check $OUTPUT_FILE for details.${NC}" >&2
    fi
fi

echo "" >&2
echo "📁 Files created:" >&2
echo "  Markdown report: $OUTPUT_FILE" >&2
echo "  Raw output: $JSON_FILE" >&2

exit $EXIT_CODE