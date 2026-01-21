#!/bin/bash

# Generic Commit Type Validator
# Validates that commit types match actual file changes

set -e

# Input validation
if [ $# -ne 1 ]; then
    echo "❌ Usage: $0 <commit-message>"
    exit 1
fi

commit_msg="$1"

# Extract commit type using regex
if ! echo "$commit_msg" | grep -qE "^(feat|fix|docs|style|refactor|test|chore|perf|ci|build|revert)(\(.+\))?: .{1,}"; then
    echo "❌ Invalid commit message format"
    exit 1
fi

commit_type=$(echo "$commit_msg" | sed -E 's/^([a-z]+).*/\1/')

echo "🔍 Validating commit type: $commit_type"

# Get staged files (what will be committed)
changed_files=$(git diff --cached --name-only)

if [ -z "$changed_files" ]; then
    echo "⚠️  No files staged for commit"
    exit 0
fi

echo "📁 Changed files:"
echo "$changed_files" | sed 's/^/  • /'
echo ""

# Generic validation function
validate_file_pattern() {
    local pattern="$1"
    local error_msg="$2"

    if ! echo "$changed_files" | grep -qE "$pattern"; then
        echo "❌ $error_msg"
        echo "💡 Changed files don't match expected pattern for '$commit_type' commits"
        return 1
    fi
    return 0
}

# Commit type specific validations
case "$commit_type" in
    docs)
        # Must change documentation files
        validate_file_pattern '\.(md|rst|txt)$|^docs/|^README|^CHANGELOG' \
            "docs commit must modify documentation files (.md, .rst, .txt, docs/, README, CHANGELOG)"
        ;;

    test)
        # Must change test files or test-related code
        validate_file_pattern '\.(test|spec)\.|tests/' \
            "test commit must modify test files or test directories"
        ;;

    ci)
        # Must change CI/CD configuration
        validate_file_pattern '\.github/|\.gitlab-ci|\.travis|Jenkinsfile|azure-pipelines|\.circleci|scripts/ci' \
            "ci commit must modify CI/CD configuration files (.github/, Jenkinsfile, etc.)"
        ;;

    feat|fix|refactor)
        # Must change source code (not just docs/tests)
        if ! echo "$changed_files" | grep -qE '\.(rs|py|js|ts|go|java|cpp|cxx|cc|c\+\+)$'; then
            echo "❌ $commit_type commit must modify source code files"
            echo "💡 Expected: .rs, .py, .js, .ts, .go, .java, .cpp, .cxx, .cc files"
            exit 1
        fi

        # For feat commits, should not be ONLY documentation or tests
        if [ "$commit_type" = "feat" ]; then
            if echo "$changed_files" | grep -qE '^docs/|README|\.md$' && ! echo "$changed_files" | grep -qE '\.(rs|py|js|ts|go|java|cpp|cxx|cc|c\+\+)$'; then
                echo "❌ feat commit should not be only documentation changes"
                echo "💡 Use 'docs:' for documentation-only changes"
                exit 1
            fi
        fi
        ;;

    style)
        # Can be source files or documentation (formatting)
        if ! echo "$changed_files" | grep -qE '\.(rs|py|js|ts|go|java|cpp|cxx|cc|c\+\+|md|rst|txt)$|^rustfmt\.toml$|\.prettierrc|\.eslintrc'; then
            echo "❌ style commit should modify code files or formatting config"
            exit 1
        fi
        ;;

    perf)
        # Must change source code (performance improvements)
        validate_file_pattern '\.(rs|py|js|ts|go|java|cpp|cxx|cc|c\+\+)$' \
            "perf commit must modify source code files for performance improvements"
        ;;

    build)
        # Must change build configuration
        validate_file_pattern '(Cargo\.toml|pyproject\.toml|package\.json|Dockerfile|\.dockerignore|Makefile|build\.gradle|\.mk)$|^scripts/build|^scripts/deploy' \
            "build commit must modify build configuration or scripts"
        ;;

    chore)
        # Most permissive - can be anything except source code changes
        # Good for version bumps, dependency updates, config changes
        if echo "$changed_files" | grep -qE '\.(rs|py|js|ts|go|java|cpp|cxx|cc|c\+\+)$' && ! echo "$changed_files" | grep -qE '^src/'; then
            echo "⚠️  chore commit includes source code changes - consider using feat/fix/refactor instead"
            echo "💡 This warning doesn't block the commit"
        fi
        ;;

    revert)
        # Can revert any files - no specific validation needed
        echo "✅ revert commit - validation skipped"
        ;;

    *)
        echo "⚠️  Unknown commit type '$commit_type' - no specific validation"
        ;;
esac

echo "✅ Commit type '$commit_type' validation passed"
exit 0