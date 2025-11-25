# Test Skill for Registry Publishing

This is a **fake test skill** created specifically for integration testing of the production registry publishing workflow.

## ⚠️ WARNING

**DO NOT USE THIS SKILL IN PRODUCTION**

This skill is intended **ONLY** for testing purposes. It should never be used in actual production environments.

## Purpose

This test skill is used to verify:
- Skill packaging functionality
- Publishing to production registry API
- Registry index updates
- Skill download and installation from registry
- End-to-end registry workflow

## Structure

- `SKILL.md` - Skill metadata and documentation
- `skill-info.toml` - Skill configuration file
- `main.py` - Fake Python script (for testing purposes only)

## Usage in Tests

This skill is used by integration tests in `integration_registry_publish_test.rs`. The tests are disabled by default and must be run manually with the `--ignored` flag.

## Version

Current version: 1.0.0

To test version updates, modify the version in both `SKILL.md` and `skill-info.toml`.

