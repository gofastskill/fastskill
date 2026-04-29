# Troubleshooting FastSkill Documentation

## 403 Errors for CDN Icons

If you're seeing 403 (Forbidden) errors for SVG icons from CloudFront CDN like:

```text
GET https://d3gk2c5xim1je2.cloudfront.net/v7.1.0/regular/cpu.svg 403 (Forbidden)
GET https://d3gk2c5xim1je2.cloudfront.net/v7.1.0/regular/refresh-cw.svg 403 (Forbidden)
```

### What This Means

These errors occur when Mintlify tries to load icon SVGs from their CDN. This is usually not a critical issue because:

1. **Icons Are Optional**: Mintlify will gracefully handle missing icons by showing fallbacks or omitting them
2. **Common Causes**:

   - Network/firewall blocking CloudFront CDN access
   - Development environment accessing CDN
   - CORS restrictions (less likely)
   - Mintlify CDN temporarily unavailable

### Solutions

#### Option 1: Use Standard Mintlify Icons (Recommended)

The icons used in the documentation (`cpu`, `refresh-cw`, `plug`, `zap`, `search`, `shield`) are valid Mintlify icon names. The 403 errors are likely environmental and won't affect production.

#### Option 2: Use Local SVG Icons (If Needed)

If CDN access is permanently blocked, you can use local SVG icons instead:

1. Replace icon names with file paths:

   ```markdown
   <Card title="Framework Agnostic" icon="/icons/cpu.svg" href="/architecture">
   ```

2. Store SVG icons in `/icons/` directory:

   ```bash
   mkdir -p webdocs/icons
   ```

3. Download or create the SVG files locally

#### Option 3: Use Icon Library Alternatives

You can also use emoji or remove icons:

```markdown
<Card title="Framework Agnostic" href="/architecture">
```

### Verification

To verify icons are correct, check:

- [Mintlify Icon Reference](https://mintlify.com/docs/components/icon)
- Icon names match exactly (case-sensitive)
- Documentation builds successfully despite 403 errors

### Production Notes

These 403 errors typically:

- **Don't affect documentation rendering** in production
- **Don't block deployment** via Mintlify
- **Are environment-specific** and may not occur in production

If icons are critical, consider hosting your own icon set or using Mintlify's icon system which may have different CDN access in production.

---

## Lock File Issues

FastSkill uses two lock files to track installed skills:
- **Project lock**: `skills.lock` in your project root
- **Global lock**: `~/.config/fastskill/global-skills.lock` (Linux/macOS) or `%APPDATA%\fastskill\global-skills.lock` (Windows)

### Lock File Is Held by Another Process

**Error**: `Lock file is held by another process: /path/to/skills.lock`

**Cause**: Another `fastskill` command is currently running and has acquired an exclusive lock on the lock file.

**Solutions**:

1. **Wait for the other command to complete**:
   - Check if another terminal has a running `fastskill` command
   - Let it finish, then retry your command

2. **Kill the conflicting process**:
   ```bash
   # Find fastskill processes
   ps aux | grep fastskill

   # Kill the process (use the PID from above)
   kill <PID>
   ```

3. **Remove stale lock guard file** (if no process is actually running):
   ```bash
   # For project lock
   rm skills.lock.lock

   # For global lock (Linux/macOS)
   rm ~/.config/fastskill/global-skills.lock.lock

   # For global lock (Windows)
   del %APPDATA%\fastskill\global-skills.lock.lock
   ```

**Prevention**: Avoid running multiple `fastskill` commands simultaneously in the same project or with the same global scope.

---

### Corrupted Lock File

**Symptoms**:
- `Parse error: ...` when running commands
- `skills.lock` contains invalid TOML
- Commands fail with deserialization errors

**Cause**: The lock file was manually edited incorrectly, interrupted during write, or corrupted by a bug.

**Solutions**:

#### Option 1: Regenerate from Manifest (Project Lock)

The safest way to recover is to delete the corrupted lock and regenerate it:

```bash
# Back up the corrupted lock (optional)
cp skills.lock skills.lock.backup

# Remove the corrupted lock
rm skills.lock

# Regenerate from skill-project.toml
fastskill install
```

This will re-resolve all dependencies and create a fresh lock file.

#### Option 2: Restore from Version Control (Project Lock)

If `skills.lock` is committed to git:

```bash
# Restore the last good version
git checkout skills.lock

# Verify it works
fastskill install --lock
```

#### Option 3: Manually Fix TOML Syntax

If you know what's wrong, you can manually edit the file:

1. Open `skills.lock` in a text editor
2. Fix TOML syntax errors (check quotes, brackets, commas)
3. Ensure all required fields are present
4. Save and test with `fastskill install --lock`

**Validation**: Use a TOML validator to check syntax:

```bash
# If you have Python installed
python3 -c "import tomllib; tomllib.load(open('skills.lock', 'rb'))"
```

#### Option 4: Clear Global Lock

For corrupted global lock:

```bash
# Back up (optional)
cp ~/.config/fastskill/global-skills.lock ~/.config/fastskill/global-skills.lock.backup

# Remove corrupted global lock
rm ~/.config/fastskill/global-skills.lock

# Reinstall global skills manually
fastskill add --global <skill-name>
```

---

### Global Lock Location Issues

**Error**: `Global config directory unavailable: ...`

**Cause**: The system cannot determine the standard config directory for your platform.

**Diagnosis**:

The global lock should be at:
- **Linux/macOS**: `~/.config/fastskill/global-skills.lock`
- **Windows**: `%APPDATA%\fastskill\global-skills.lock`

Check if the directory exists:

```bash
# Linux/macOS
ls -la ~/.config/fastskill/

# Windows (PowerShell)
dir $env:APPDATA\fastskill\
```

**Solutions**:

1. **Create the directory manually**:
   ```bash
   # Linux/macOS
   mkdir -p ~/.config/fastskill

   # Windows (PowerShell)
   New-Item -ItemType Directory -Path "$env:APPDATA\fastskill" -Force
   ```

2. **Check permissions**:
   ```bash
   # Linux/macOS - ensure you own the directory
   ls -ld ~/.config/fastskill
   # Should show your username as owner

   # Fix permissions if needed
   chmod 755 ~/.config/fastskill
   ```

3. **Set XDG_CONFIG_HOME explicitly** (Linux/macOS advanced):
   ```bash
   export XDG_CONFIG_HOME="$HOME/.config"
   fastskill add --global <skill>
   ```

---

### Lock File Mismatch After Migration

**Symptom**: After upgrading from FastSkill ≤ 0.9.111, skills seem out of sync or lock file looks different.

**Cause**: Lock file was automatically migrated from v1.0.0 to v2.0.0 format, removing timestamp fields.

**What Changed**:
- `metadata.generated_at` removed
- `[[skills]].fetched_at` removed
- `metadata.version` changed from `"1.0.0"` to `"2.0"`
- Skill entries are now sorted alphabetically by ID

**This is expected and correct**. The migration preserves all important data (versions, checksums, dependencies).

**Verification**:

1. Check the lock file version:
   ```bash
   grep '^version = ' skills.lock
   # Should output: version = "2.0"
   ```

2. Verify no timestamps:
   ```bash
   grep -E 'generated_at|fetched_at' skills.lock
   # Should output nothing
   ```

3. Reconcile with manifest:
   ```bash
   fastskill install
   ```

If you see unexpected changes in `git diff skills.lock`, this is likely due to the migration. The new format is deterministic and won't cause spurious diffs in future operations.

---

### Skills.lock Not Deterministic (Pre-v2.0)

**Symptom**: Every time you run `fastskill install` or `fastskill add`, `skills.lock` shows changes in git even though no dependencies changed.

**Cause**: You're using an old version of FastSkill (≤ 0.9.111) with lock file format v1.0.0.

**Solution**: Upgrade to the latest FastSkill version and let the lock file migrate:

```bash
# Upgrade fastskill
cargo install fastskill-cli --force

# Or build from source
cd fastskill
cargo build --release

# Run any command to trigger migration
fastskill install

# Verify migration
grep '^version = ' skills.lock
# Should output: version = "2.0"
```

After migration, the lock file will be deterministic and suitable for committing to git.

---

### Best Practices

To avoid lock file issues:

1. **Commit project lock to git**:
   ```bash
   git add skills.lock
   git commit -m "Add skill lock file"
   ```

2. **Don't commit global lock**:
   ```bash
   # Add to .gitignore
   echo "global-skills.lock" >> ~/.gitignore
   ```

3. **Use `--lock` in CI** for reproducible builds:
   ```bash
   fastskill install --lock
   ```

4. **Don't manually edit lock files** unless you understand the TOML schema

5. **Regenerate rather than fix** if the lock becomes corrupted

6. **Keep FastSkill up to date** to get lock file improvements and bug fixes

---

### Getting Help

If you encounter lock file issues not covered here:

1. Check lock file format: `head -20 skills.lock`
2. Validate TOML syntax with a validator
3. Try regenerating with `rm skills.lock && fastskill install`
4. Report the issue at: https://github.com/yourusername/fastskill/issues

Include:
- FastSkill version (`fastskill --version`)
- Lock file version (`grep '^version = ' skills.lock`)
- Full error message
- Lock file content (redact sensitive URLs if needed)
