# Troubleshooting

FastSkill 0.9.229

Source: https://docs.gofastskill.com/troubleshooting

Release revision: 68975dfaca15508bc5cf2fb761d6218405c954ea

Documentation revision: 68975dfaca15508bc5cf2fb761d6218405c954ea



# Troubleshooting

Common issues and fixes when operating FastSkill. Run `fastskill cli doctor` first — it checks the
skills directory, `skill-project.toml`, embedding configuration, `OPENAI_API_KEY`, and auth token.

## Lock File Issues

FastSkill uses two lock files to track installed skills:

* **Project lock**: `skills.lock` in your project root
* **Global lock**: `~/.config/fastskill/global-skills.lock` (Linux/macOS) or `%APPDATA%\fastskill\global-skills.lock` (Windows)

### Lock File Is Held by Another Process

**Error**: `Lock file is held by another process: /path/to/skills.lock`

**Cause**: Another `fastskill` command is currently running and has acquired an exclusive lock on the lock file.

**Solutions**:

1. **Wait for the other command to complete**:
   * Check if another terminal has a running `fastskill` command
   * Let it finish, then retry your command

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

***

### Corrupted Lock File

**Symptoms**:

* `Parse error: ...` when running commands
* `skills.lock` contains invalid TOML
* Commands fail with deserialization errors

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
fastskill project install
```

This will re-resolve all dependencies and create a fresh lock file.

#### Option 2: Restore from Version Control (Project Lock)

If `skills.lock` is committed to git:

```bash
# Restore the last good version
git checkout skills.lock

# Verify it works
fastskill project install --lock
```

#### Option 3: Manually Fix TOML Syntax

If you know what's wrong, you can manually edit the file:

1. Open `skills.lock` in a text editor
2. Fix TOML syntax errors (check quotes, brackets, commas)
3. Ensure all required fields are present
4. Save and test with `fastskill project install --lock`

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
fastskill skill add --global <skill-name>
```

***

### Global Lock Location Issues

**Error**: `Global config directory unavailable: ...`

**Cause**: The system cannot determine the standard config directory for your platform.

**Diagnosis**:

The global lock should be at:

* **Linux/macOS**: `~/.config/fastskill/global-skills.lock`
* **Windows**: `%APPDATA%\fastskill\global-skills.lock`

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

3. **Set XDG\_CONFIG\_HOME explicitly** (advanced):

   ```bash
   # Linux/macOS
   export XDG_CONFIG_HOME="$HOME/.config"
   fastskill skill add --global <skill>
   ```

   ```powershell
   # Windows PowerShell
   $env:XDG_CONFIG_HOME = "$HOME\.config"
   fastskill skill add --global <skill>
   ```

***

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
   fastskill project install --lock
   ```

4. **Don't manually edit lock files** unless you understand the TOML schema

5. **Regenerate rather than fix** if the lock becomes corrupted

6. **Keep FastSkill up to date** to get lock file improvements and bug fixes

***

### Getting Help

If you encounter lock file issues not covered here:

1. Check lock file format: `head -20 skills.lock`
2. Validate TOML syntax with a validator
3. Try regenerating with `rm skills.lock && fastskill project install`
4. Report the issue at: [https://github.com/yourusername/fastskill/issues](https://github.com/yourusername/fastskill/issues)

Include:

* FastSkill version (`fastskill --version`)
* Lock file version (`grep '^version = ' skills.lock`)
* Full error message
* Lock file content (redact sensitive URLs if needed)

