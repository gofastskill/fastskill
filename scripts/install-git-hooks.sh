#!/bin/bash

# Git Hooks Installation Script
# Install git hooks from .githooks/ to .git/hooks/

set -e

echo "🔗 Installing Git Hooks"
echo "======================"

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ]; then
    echo "❌ Error: Run this script from the project root"
    exit 1
fi

echo "✅ Repository structure verified"

# Check if pre-commit is already set up
if [ -f ".pre-commit-config.yaml" ] && command -v pre-commit >/dev/null 2>&1; then
    echo ""
    echo "📋 Pre-commit framework detected!"
    echo "Installing pre-commit hooks..."
    pre-commit install
    echo "✅ Pre-commit hooks installed"
    echo ""
    echo "Available pre-commit hooks:"
    echo "  • pre-commit: Runs formatting, linting, and unit tests before commits"
    echo "    - trailing-whitespace, end-of-file-fixer, check-yaml"
    echo "    - check-added-large-files, check-merge-conflict, debug-statements"
    echo "    - fmt, cargo-check, clippy, test"
else
    echo ""
    echo "📋 Installing traditional git hooks from .githooks/..."

    # Check if .githooks directory exists
    if [ ! -d ".githooks" ]; then
        echo "❌ Error: .githooks directory not found"
        exit 1
    fi

    # Create .git/hooks directory if it doesn't exist
    if [ ! -d ".git/hooks" ]; then
        echo "📁 Creating .git/hooks directory..."
        mkdir -p .git/hooks
        echo "✅ .git/hooks directory created"
    fi

    # Install hooks
    hooks_installed=0

    for hook_file in .githooks/*; do
        if [ -f "$hook_file" ]; then
            hook_name=$(basename "$hook_file")

            # Skip utility scripts that aren't git hooks
            case "$hook_name" in
                validate-commit-type.sh)
                    echo "  Installing $hook_name (utility script)..."
                    # Make executable in place - don't copy to .git/hooks/
                    chmod +x ".githooks/$hook_name"
                    echo "  ✅ $hook_name installed as utility script"
                    ((hooks_installed++))
                    continue
                    ;;
            esac

            echo "  Installing $hook_name..."

            # Copy hook to .git/hooks/
            cp "$hook_file" ".git/hooks/$hook_name"

            # Make it executable
            chmod +x ".git/hooks/$hook_name"

            echo "  ✅ $hook_name installed"
            ((hooks_installed++))
        fi
    done

    echo ""
    echo "Summary:"
    echo "  ✅ Hooks installed: $hooks_installed"
    echo ""
    echo "Available hooks:"
    if [ -f ".git/hooks/pre-commit" ]; then
        echo "  • pre-commit: Runs formatting, linting, and unit tests before commits"
    fi
    if [ -f ".git/hooks/pre-push" ]; then
        echo "  • pre-push: Runs full test suite and documentation build before pushes"
    fi
    if [ -f ".git/hooks/commit-msg" ]; then
        echo "  • commit-msg: Validates conventional commit message format and type-file matching"
    fi
fi

echo ""
echo "🎉 Git hooks installation completed!"
echo ""

exit 0
