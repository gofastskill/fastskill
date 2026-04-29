# Two-Scope Lock Model

**Version**: 2.0
**Last Updated**: 2026-04-29

## Overview

FastSkill uses two separate lock files to track installed skills at different scopes:

1. **Project Lock** (`skills.lock`) — Tracks skills installed in a specific project
2. **Global Lock** (`global-skills.lock`) — Tracks skills installed globally for the current user

This separation ensures that global and project skills remain isolated, improves team collaboration through deterministic lock files, and enables proper dependency tracking at both scopes.

---

## Lock File Locations

### Project Lock

**Path**: `<project_root>/skills.lock`

**Purpose**: Records all skills installed in the current project, including transitive dependencies. This file should be committed to version control.

**Created by**:
- `fastskill init` (empty)
- `fastskill install`
- `fastskill add <skill>` (without `--global`)

**Format version**: `2.0`

### Global Lock

**Path**:
- Linux/macOS: `~/.config/fastskill/global-skills.lock`
- Windows: `%APPDATA%\fastskill\global-skills.lock`

**Purpose**: Records all globally installed skills for the current user. This file is NOT committed to version control and includes operational metadata like install timestamps.

**Created by**:
- `fastskill add --global <skill>`

**Format version**: `1.0`

---

## File Formats

### Project Lock Format (skills.lock)

The project lock uses a **deterministic format** with no timestamps, ensuring byte-identical output for the same dependency set. This makes it suitable for version control and team workflows.

```toml
[metadata]
version = "2.0"
fastskill_version = "0.9.112"

[[skills]]
id = "my-skill"
name = "My Skill"
version = "1.2.0"
source = { type = "source", name = "default", skill = "my-skill", version = "1.2.0" }
checksum = "sha256:abc123..."
dependencies = ["dependency-skill"]
groups = ["dev"]
editable = false
depth = 0
parent_skill = "parent-skill-id"  # Optional, only for transitive deps
```

**Key characteristics**:
- **No timestamps**: No `generated_at` in metadata, no `fetched_at` in skill entries
- **Sorted entries**: Skills are always sorted alphabetically by `id`
- **Deterministic**: Running `install` twice produces identical output
- **Version 2.0**: Format version indicates deterministic schema

**Fields**:
- `id`: Unique skill identifier
- `name`: Human-readable skill name
- `version`: Installed version
- `source`: Where the skill was fetched from (registry, git, local path, etc.)
- `checksum`: SHA-256 hash of skill content (when available)
- `dependencies`: List of skill IDs this skill depends on
- `groups`: Optional groups this skill belongs to (e.g., "dev", "prod")
- `editable`: Whether the skill was installed in editable mode
- `depth`: Dependency depth (0 = direct dependency, 1+ = transitive)
- `parent_skill`: For transitive deps, which direct dependency required this

### Global Lock Format (global-skills.lock)

The global lock includes **operational metadata** to track when skills were installed, checked for updates, and last updated.

```toml
[metadata]
version = "1.0"
fastskill_version = "0.9.112"

[[skills]]
id = "my-global-skill"
name = "My Global Skill"
version = "2.0.0"
source = { type = "git", url = "https://github.com/org/repo.git", branch = "main" }
checksum = "sha256:def456..."
installed_at = "2026-04-28T10:00:00Z"
last_checked_at = "2026-04-28T12:00:00Z"
last_updated_at = "2026-04-28T10:00:00Z"
dependencies = []
groups = []
```

**Key characteristics**:
- **Includes timestamps**: `installed_at`, `last_checked_at`, `last_updated_at`
- **User-specific**: Not shared across users or machines
- **Not committed**: Should NOT be in version control
- **Sorted entries**: Skills sorted alphabetically by `id` for readability

**Additional fields** (vs project lock):
- `installed_at`: When the skill was first installed globally
- `last_checked_at`: When `update --check --global` last ran for this skill
- `last_updated_at`: When the skill was last updated to a new version

---

## Command Behavior by Scope

| Command | Project Lock | Global Lock |
|---------|-------------|-------------|
| `fastskill init` | Created (empty) | Not touched |
| `fastskill add <skill>` | Written | Not touched |
| `fastskill add --global <skill>` | Not touched | Written |
| `fastskill install` | Written | Not touched |
| `fastskill install --lock` | Read (pinned versions) | Not touched |
| `fastskill remove <skill>` | Written (entry removed) | Not touched |
| `fastskill remove --global <skill>` | Not touched | Written (entry removed) |
| `fastskill update` | Written (versions bumped) | Not touched |
| `fastskill update --global` | Not touched | Written (versions bumped) |
| `fastskill update --check` | Read-only | Not touched |
| `fastskill update --check --global` | Not touched | Read + Write (`last_checked_at`) |

**Important**: `fastskill install` is **always project-scoped**. It reads `skill-project.toml` and writes `skills.lock`. It never touches the global lock.

---

## Migration from Version 1.0.0

### What Changed

Lock files created with FastSkill version ≤ 0.9.111 used format version `1.0.0` and included volatile timestamp fields:

- `metadata.generated_at` — Timestamp when lock was last written
- `[[skills]].fetched_at` — Timestamp when skill was installed

These fields caused non-deterministic diffs in version control, even when no actual dependencies changed.

### Automatic Migration

When you load a v1.0.0 lock file and perform any operation that writes the lock (add, install, remove, update), the file is **automatically migrated** to v2.0.0 format:

1. `generated_at` is removed from metadata
2. `fetched_at` is removed from all skill entries
3. `metadata.version` is updated to `"2.0"`
4. Entries are sorted by `id`
5. All other data is preserved

**No manual intervention is required**. The migration happens transparently.

### Compatibility

- **Reading v1.0.0 locks**: Fully supported. Extra fields are ignored during deserialization.
- **`install --lock` with v1.0.0**: Fully supported. Pinned versions are respected, file is migrated on write.
- **Backward compatibility**: Code using the `SkillsLock` type alias continues to work (type alias points to `ProjectSkillsLock`).

### Verification

After migration, verify the lock file:

```bash
# Check version
grep '^version = ' skills.lock
# Should output: version = "2.0"

# Verify no timestamps
grep -E 'generated_at|fetched_at' skills.lock
# Should output nothing
```

---

## CI and Team Workflows

### Committing Lock Files

**Project lock** (`skills.lock`):
- ✅ **Should be committed** to version control
- ✅ Deterministic format ensures no spurious diffs
- ✅ Enables reproducible builds in CI
- ✅ Team members get identical dependency versions

**Global lock** (`global-skills.lock`):
- ❌ **Should NOT be committed** to version control
- ❌ User-specific, machine-specific
- ❌ Contains timestamps that change on every check

### Reproducible CI Builds

To ensure reproducible CI builds:

1. Commit `skill-project.toml` and `skills.lock` to git
2. In CI, run `fastskill install --lock` to install pinned versions
3. Lock file content will be byte-identical across runs
4. No unexpected dependency updates during CI

Example CI workflow:

```yaml
- name: Install FastSkill dependencies
  run: fastskill install --lock

- name: Verify lock file unchanged
  run: git diff --exit-code skills.lock
```

### Updating Dependencies

To update project dependencies:

```bash
# Check for available updates (doesn't modify lock)
fastskill update --check

# Update all skills to latest compatible versions
fastskill update

# Review changes
git diff skills.lock

# Commit updated lock
git add skills.lock
git commit -m "Update skill dependencies"
```

---

## Global Skill Workflows

### Installing Global Skills

Global skills are available across all projects for the current user:

```bash
# Install a skill globally
fastskill add --global my-utility-skill

# The skill is available in all projects
cd ~/any-project
fastskill list --global  # Shows my-utility-skill
```

Global installation:
- ✅ Updates `~/.config/fastskill/global-skills.lock`
- ✅ Installs to `~/.config/fastskill/skills/`
- ❌ Does NOT modify `skill-project.toml` or `skills.lock`

### Managing Global Skills

```bash
# List globally installed skills
fastskill list --global

# Check for updates to global skills
fastskill update --check --global

# Update all global skills
fastskill update --global

# Remove a global skill
fastskill remove --global my-utility-skill
```

### Global vs Project Skills

If a skill is installed both globally and in a project:
- Project version takes precedence within that project
- Global version is available in projects that don't have it locally
- Locks remain separate and don't conflict

---

## Troubleshooting

### Lock File Issues

See [TROUBLESHOOTING.md](../../webdocs/TROUBLESHOOTING.md#lock-file-issues) for detailed solutions to:
- Concurrent write conflicts (`FileLocked` error)
- Corrupted lock files
- Global lock location issues
- Migration problems

### Common Issues

**"Lock file is held by another process"**
- Another `fastskill` command is running
- Wait for it to complete or kill the process
- Remove `skills.lock.lock` or `global-skills.lock.lock` if stale

**"Global config directory unavailable"**
- Platform doesn't support standard config directories
- Check `~/.config/fastskill/` (Linux/macOS) or `%APPDATA%\fastskill\` (Windows)
- Ensure directory permissions allow writes

**Skills out of sync after migration**
- Run `fastskill install` to reconcile
- Compare `skill-project.toml` with `skills.lock`
- Use `fastskill update --check` to verify versions

---

## Technical Details

### Concurrency Protection

Lock files are protected against concurrent writes using advisory file locks (`fs2` crate):

- Lock guard file: `skills.lock.lock` or `global-skills.lock.lock`
- Retry logic: 3 attempts with 200ms intervals
- Failure mode: Returns `LockError::FileLocked` if lock cannot be acquired

### Entry Sorting

Project lock entries are **always sorted alphabetically by skill ID** before writing. This ensures:
- Deterministic output regardless of installation order
- Clean git diffs (new skills appear in sorted position, not at end)
- Easy manual inspection

### Checksum Verification

When available, skills include a SHA-256 checksum in the lock file. This is used to verify integrity during `install --lock` operations.

Not all source types provide checksums (e.g., editable local paths), so `checksum` is optional.

---

## API Reference

For programmatic access to lock files:

```rust
use fastskill_core::core::lock::{
    ProjectSkillsLock,
    GlobalSkillsLock,
    project_lock_path,
    global_lock_path,
};

// Load project lock
let project_lock = ProjectSkillsLock::load_from_file(
    &project_lock_path(&project_file_path)
)?;

// Load global lock
let global_lock = GlobalSkillsLock::load_from_file(
    &global_lock_path()?
)?;
```

See `crates/fastskill-core/src/core/lock.rs` for full API documentation.

---

## Summary

The two-scope lock model provides:

✅ **Deterministic project locks** for reliable team workflows and CI
✅ **Proper global skill tracking** with operational metadata
✅ **Automatic migration** from v1.0.0 format
✅ **Scope isolation** preventing global/project conflicts
✅ **Concurrency protection** against corrupted writes

For most users, the lock files work transparently. Commit `skills.lock` to git, and don't commit `global-skills.lock`.
