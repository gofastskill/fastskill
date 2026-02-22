# Style guide

_The following is a work-in-progress style guide for our user-facing messaging in the CLI output and
documentation_.

## Scope

This guide applies to:

- CLI output and user-facing strings
- Documentation (markdown files)
- Code comments (when they serve as user-facing documentation)

## General

1. Use of "e.g." and "i.e." should always be wrapped in commas, e.g., as shown here.
1. **Documentation**: Prefer "—" (Unicode em dash) wrapped in spaces, e.g., "hello — world" not "hello—world".
1. **CLI output**: Prefer punctuation that degrades well: `-` or `:` or `,` unless rendering across platforms has been validated.
1. Hyphenate compound words, e.g., use "platform-specific" not "platform specific".
1. Use backticks to escape: commands, code expressions, package names, and file paths.
1. **Documentation**: Prefer `[label](url)`. Use `<url>` only in reference documentation or when the URL itself is the meaningful value (for copy/paste).
1. **CLI**: If printing a URL as data, print the raw URL (or `<...>` if consistent delimiting is desired), but document this behavior.
1. If a message ends with a single relevant value, precede it with a colon, e.g.,
   `This is the value: value`. If the value is a literal, wrap it in backticks.
1. Markdown files should be wrapped at 100 characters.
1. In FastSkill documentation/examples, use space-separated values for command-line arguments, e.g.,
   `--resolution lowest`, not `--resolution=lowest`.

## Styling fastskill

Just fastskill, please.

1. Do not escape with backticks, e.g., `fastskill`, unless referring specifically to the `fastskill` executable.
1. Do not capitalize, e.g., "Fastskill", even at the beginning of a sentence.
1. Do not uppercase, e.g., "FASTSKILL", unless referring to an environment variable, e.g., `FASTSKILL_API_URL`.

## Terminology

1. Use "lockfile" not "lock file".
2. Use "pre-release", not "prerelease" (except in code, in which case: use `Prerelease`, not
   `PreRelease`; and `prerelease`, not `pre_release`).

### Glossary

- **set up** (verb) vs **setup** (noun)
- **command line** vs **command-line** (use hyphen when used as compound adjective)
- **configuration file** vs **config file** (prefer full form in documentation)
- **skill** vs **skills** (use appropriate singular/plural based on context)
- **registry** vs **repository** (registry for skill storage, repository for source code)

## Documentation

1. Use periods at the end of all sentences, including lists unless they enumerate single items.
1. Avoid language that patronizes the reader, e.g., "simply do this".
1. Only refer to "the user" in internal or contributor documentation.
1. Avoid "we" in favor of "fastskill" or imperative language.

### Sections

The documentation is divided into:

1. Guides
2. Concepts
3. Reference documentation

#### Guides

1. Should assume no previous knowledge about fastskill.
1. May assume basic knowledge of the domain.
1. Should refer to relevant concept documentation.
1. Should have a clear flow.
1. Should be followed by a clear call to action.
1. Should cover the basic behavior needed to get started.
1. Should not cover behavior in detail.
1. Should not enumerate all possibilities.
1. Should avoid linking to reference documentation unless not covered in a concept document.
1. May generally ignore platform-specific behavior.
1. Should be written from second-person point of view.
1. Should use the imperative voice.

#### Concepts

1. Should cover behavior in detail.
1. Should not enumerate all possibilities.
1. Should cover most common configuration.
1. Should refer to the relevant reference documentation.
1. Should discuss platform-specific behavior.
1. Should be written from the third-person point of view, not second-person (i.e., avoid "you").
1. Should not use the imperative voice.

#### Reference documentation

1. Should enumerate all options.
1. Should generally be generated from documentation in the code.
1. Should be written from the third-person point of view, not second-person (i.e., avoid "you").
1. Should not use the imperative voice.

### Code blocks

1. All code blocks should have a language marker.
1. When using `console` syntax, use `$` to indicate commands — everything else is output.
1. Never use the `bash` syntax when displaying command output.
1. Prefer `console` with `$` prefixed commands over `bash`.
1. Command output should rarely be included — it's hard to keep up-to-date.
1. Use `title` for example files, e.g., `pyproject.toml`, `Dockerfile`, or `example.py`.

## CLI

1. CLI text intentionally uses "headline style" (terse and without trailing periods for single-sentence messages). Use periods only when a message spans multiple sentences.
1. May use the second-person point of view, e.g., "Did you mean...?".

### Colors and style

1. All CLI output must be interpretable and understandable _without_ the use of color and other
   styling. (For example: even if a command is rendered in green, wrap it in backticks.)
1. `NO_COLOR` must be respected when using any colors or styling.
1. `FASTSKILL_NO_PROGRESS` must be respected when using progress-styling like bars or spinners.
1. Disable spinners and interactive elements when stdout is not a TTY; keep machine-readable output stable.
1. Respect `TERM=dumb` or similar fallback for limited terminal capabilities.
1. Ensure colors are not the only signal; use prefix icons or labels (e.g., `[✓]` for success, `[✗]` for error) alongside colors.
1. In general, use:
   - Green for success.
   - Red for error.
   - Yellow for warning.
   - Cyan for hints.
   - Cyan for file paths.
   - Cyan for important user-facing literals (e.g., a package name in a message).
   - Green for commands.

#### Detailed color palette

Use ANSI color codes or named colors for consistency:

- **Success**: Bright green (`\x1b[32m` or `ansi::Green`)
- **Error**: Bright red (`\x1b[31m` or `ansi::Red`)
- **Warning**: Bright yellow (`\x1b[33m` or `ansi::Yellow`)
- **Info/Hints**: Bright cyan (`\x1b[36m` or `ansi::Cyan`)
- **File paths**: Cyan (same as info)
- **Commands**: Green (same as success)
- **Emphasis**: Bright white or bold (`\x1b[1m`)

#### Accessibility considerations

1. Ensure sufficient color contrast (4.5:1 ratio minimum)
1. Use colorblind-friendly combinations (avoid red/green alone)
1. Always provide alternative indicators (icons, prefixes, text labels)
1. Test color output in various terminal themes

**Colorblind-friendly combinations:**

```rust
// Good: Use both color and icon
println!("{} {}", "✓".green(), "Success message".green());

// Better: Use color + icon + text
println!("{} {} {}", "✓".green(), "SUCCESS:".green().bold(), "Operation completed");
```

#### When NOT to use colors

- Machine-readable output (JSON, XML, logs)
- Output redirected to files or pipes
- When `NO_COLOR` environment variable is set
- In error messages that may be parsed by scripts

#### NO_COLOR implementation

```rust
fn should_use_colors() -> bool {
    // NO_COLOR takes precedence over TTY detection
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }

    // Check if output is a TTY
    is_tty()
}
```

#### Icon and emoji usage

1. Use simple Unicode symbols that work across platforms
1. Provide text fallbacks for limited terminals
1. Prefer consistent icon set throughout the application

**Recommended icons:**

- `✓` or `✔` - Success/check
- `✗` or `✕` - Error/failure
- `⚠` - Warning
- `ℹ` - Information
- `⠋` - Spinner (when supported)
- `→` - Arrow/progress indicator

**Icon + color examples:**

```rust
// Success with icon
println!("{} Skill installed successfully", "✓".green());

// Warning with icon
println!("{} {}", "⚠".yellow(), "Using deprecated feature".yellow());

// Error with icon
println!("{} {}", "✗".red(), "Installation failed".red());
```

### Logging

1. The `--verbose` flag enables logs at all levels. `RUST_LOG` overrides this to provide fine-grained control over log levels and module-specific filtering, e.g., `RUST_LOG=fastskill=info` for module-specific control.
1. All logging should be to stderr.

#### Logging vs user messages boundaries

1. Use `tracing` for debug information and development logs that require `--verbose`
1. Use direct output (`println!`, `eprintln!`) for user-facing messages that should always be visible
1. User-facing messages should not require `--verbose` flag
1. Debug logs should use appropriate tracing levels (`debug!`, `info!`, `warn!`, `error!`)
1. Avoid expensive operations in user-facing message formatting

**When to use tracing:**

```rust
// Debug information - requires --verbose
tracing::debug!("Processing skill: {}", skill_id);
tracing::info!("Connecting to registry at {}", url);

// Performance metrics - requires --verbose
tracing::debug!(elapsed = ?start.elapsed(), "Skill installation completed");
```

**When to use direct output:**

```rust
// User-facing messages - always visible
println!("✓ Skill '{}' installed successfully", skill_id);
eprintln!("error: Failed to connect to registry");

// Progress and status - always visible
println!("Installing skills...");
eprintln!("⠋ Searching for dependencies...");
```

**Performance considerations:**

```rust
// Good: Direct output for user messages
println!("Found {} skills", count);

// Avoid: Expensive formatting in user messages
// Bad: println!("Found {} skills: {}", count, expensive_list.join(", "));
```

**--verbose flag behavior:**

- Without `--verbose`: Show only user-facing messages
- With `--verbose`: Show user-facing messages + all tracing logs
- `RUST_LOG` provides fine-grained control: `RUST_LOG=fastskill=debug`

### Output

1. Text can be written to stdout if it is "data" that could be piped to another program.

#### Progress indicators and spinners

1. Use spinners for indeterminate operations (e.g., "Installing skills...")
1. Use progress bars for determinate operations with known completion (e.g., downloading files)
1. Use simple status messages for quick operations (< 1 second)
1. Spinner format: `[spinner] <action> <target>` (e.g., `⠋ Installing requests...`)
1. Progress bar format: `[XX%] <action> <target> (ETA: Xs)` (e.g., `[45%] Downloading data (ETA: 12s)`)
1. Always detect TTY and disable interactive indicators when stdout is not a TTY
1. Respect `FASTSKILL_NO_PROGRESS` environment variable to disable progress indicators
1. Provide meaningful fallback messages when progress indicators are disabled
1. Update progress frequently but not excessively (target: 5-10 updates per second max)

**Good examples:**

```
⠋ Searching for skills...
[23%] Installing requests (ETA: 8s)
✓ Published skill successfully
```

**TTY-aware implementation:**

```rust
if is_tty && !no_progress {
    // Show interactive spinner
    show_spinner("Installing skills...");
} else {
    // Show simple status
    println!("Installing skills...");
}
```

**Environment variable usage:**

```bash
# Disable all progress indicators
FASTSKILL_NO_PROGRESS=1 fastskill install requests
```

#### Structured data output

1. Use JSON for machine-readable output and API responses
1. Use XML only when specifically requested or for legacy compatibility
1. Use table format for human-readable listings and summaries
1. Always pretty-print JSON output by default (2-space indentation)
1. Use consistent JSON structure with top-level objects, not arrays
1. Include error information in structured output using standard error format
1. Provide `--json`, `--xml`, `--table` flags for output format selection
1. Default to human-readable format unless machine output is requested

**JSON standards:**

```json
{
  "data": [...],
  "meta": {
    "count": 5,
    "version": "1.0"
  }
}
```

**Error format in JSON:**

```json
{
  "error": {
    "code": "FS001",
    "message": "Skill not found",
    "details": "The skill 'web-scraper' was not found in the registry"
  }
}
```

**Table formatting guidelines:**

- Left-align text columns, right-align numeric columns
- Use consistent column widths and separators
- Include headers unless output is very brief
- Wrap long content at column boundaries
- Use box-drawing characters for borders when appropriate

**Output format selection:**

```
fastskill list                    # Human-readable table (default)
fastskill list --json            # JSON format
fastskill search web --xml       # XML format
fastskill show requests --table  # Explicit table format
```

**Machine-readable vs human-readable:**

- Machine output: JSON/XML with stable schemas, minimal localization
- Human output: Tables with colors, localized messages, formatting

#### Version and branding display

1. Display version as `fastskill <version>` (lowercase, no "FastSkill" branding)
1. Show version information on startup unless explicitly suppressed
1. Use consistent branding across all user-facing text
1. Include version in help output and error messages when relevant

**Version display format:**

```bash
# Startup message
fastskill 1.2.3

# Version command
fastskill --version
fastskill 1.2.3

# In help text
fastskill install --help
# Shows: fastskill-install 1.2.3
```

**Startup message standards:**

- Show version on first run or when significant changes occur
- Keep startup messages brief and non-intrusive
- Allow suppression with flags like `--quiet` or configuration
- Include brief description on first run

**Example startup output:**

```
FastSkill 1.2.3
Package manager and operational toolkit for AI agent skills

Run 'fastskill --help' to get started
```

**Branding consistency:**

- Use "fastskill" (lowercase) in commands and binary name
- Use "FastSkill" (title case) only in marketing/description contexts
- Maintain consistent terminology across documentation and UI
- Avoid version-specific branding changes unless major rebranding

#### Output formatting utilities

1. Use text wrapping at 80-100 columns for readability
1. Maintain consistent indentation (2 spaces for nested content, 4 for lists)
1. Use consistent list formatting (bullets, numbers, indentation)
1. Format multi-line messages with proper alignment
1. Provide utility functions for common formatting patterns

**Text wrapping standards:**

```rust
// Wrap at 80 columns by default
const MAX_LINE_LENGTH: usize = 80;

// Utility function for wrapping text
fn wrap_text(text: &str, width: usize) -> String {
    // Implementation using textwrap crate or similar
    textwrap::wrap(text, width).join("\n")
}
```

**Indentation standards:**

- **Top-level content**: No indentation
- **Nested content**: 2 spaces
- **List items**: 2 spaces for bullets, content indented to align
- **Error details**: 2 spaces from error prefix
- **Hints**: Aligned with error message start

**Example indentation:**

```
error: Main error message here
       Additional details about the error
       span multiple lines with consistent
       indentation.

hint: First suggestion for resolution
hint: Second suggestion if first doesn't work
```

**List formatting:**

- Use `-` for unordered lists
- Use `1.` `2.` `3.` for ordered lists
- Align content consistently
- Keep list items under 80-100 columns

**Good list formatting:**

```
Available skills:
  - web-scraper    Extract data from websites
  - data-analyzer  Process and analyze datasets
  - file-parser    Parse various file formats

Installation steps:
  1. Authenticate with registry
  2. Download skill dependencies
  3. Install skill and verify integrity
```

**Multi-line message formatting:**

```
Skill information:
  Name: web-scraper
  Version: 1.2.3
  Description: Extract structured data from
               websites using various techniques
               including CSS selectors and
               XPath expressions.

  Dependencies:
    - requests >= 2.25.0
    - beautifulsoup4 >= 4.9.0
    - lxml >= 4.6.0
```

**Common formatting utilities:**

```rust
// Indented blocks
fn indent(text: &str, spaces: usize) -> String {
    text.lines()
        .map(|line| format!("{}{}", " ".repeat(spaces), line))
        .collect::<Vec<_>>()
        .join("\n")
}

// Key-value formatting
fn format_key_value(key: &str, value: &str, max_key_width: usize) -> String {
    format!("{:<width$} {}", key, value, width = max_key_width)
}

// Error formatting with consistent indentation
fn format_error_with_details(error: &str, details: &[&str]) -> String {
    let mut result = error.to_string();
    for detail in details {
        result.push_str(&format!("\n  {}", detail));
    }
    result
}
```

### Warnings

1. `warn_user` and `warn_user_once` are shown without the `--verbose` flag.
   - These methods should be preferred over tracing warnings when the warning is actionable.
   - Deprecation warnings should use these methods.
1. Deprecation warnings must be actionable.

### Hints

1. Errors may be followed by hints suggesting a solution.
1. Hints should be separated from errors by a blank newline.
1. Hints should be stylized as `hint: <content>`.
1. Always provide hints for actionable errors; optional for informational errors.
1. When multiple solutions exist, format as separate hint lines or numbered hints.
1. Be specific in hints: include exact commands, file paths, or configuration options when applicable.
1. Focus hints on the most likely solutions first.

**Good hint examples:**

```
error: Skill 'web-scraper' not found

hint: Run 'fastskill search web scraper' to find similar skills
hint: Check available skills with 'fastskill list'
```

**Multiple hints formatting:**

```
error: Failed to connect to registry

hint: Check your internet connection
hint: Verify FASTSKILL_API_URL is set correctly
hint: Run 'fastskill auth login' to refresh authentication
```

**Integration with error template:**

Always follow the standard error format:
```
error: <summary>
[optional details]

hint: <primary solution>
hint: <alternative solution>
```

### Message quality

Principles for high-quality CLI messages:

- **Actionable**: Errors should explain what failed, why (if known), and what to do next.
- **Stable**: Avoid including volatile output like timestamps or temporary file paths.
- **Structured**: Use consistent prefixes (`error:`, `warning:`, `hint:`), indentation, and line wrapping at 80-100 columns.
- **No blame**: Use neutral language; avoid phrases like "you did X wrong".
- **Line width**: CLI output and hint blocks should wrap at 80-100 columns for readability.
- **Standard error format**: `error: <summary>` then optional details; then blank line; then hints.
- **Error codes**: Include short identifiers for searchable, documented errors.

#### Error codes system

1. Use format `FS` + 3-digit number (e.g., `FS001`, `FS002`) for FastSkill error codes
1. Document all error codes in a central registry with descriptions and recovery suggestions
1. Include error codes in user-facing errors, especially for common or recoverable issues
1. Provide lookup mechanism for error code details and troubleshooting

**Error code format:**

- `FS001-FS099`: General/system errors
- `FS100-FS199`: Authentication and authorization errors
- `FS200-FS299`: Skill management errors
- `FS300-FS399`: Registry errors
- `FS400-FS499`: Network and connectivity errors
- `FS500-FS599`: Validation errors

**When to include error codes:**

- All documented, user-recoverable errors
- Common error conditions that users are likely to encounter
- Errors that may require support assistance
- Skip codes for transient or internal errors

**Format options:**

```
error: Skill not found [FS201]
error [FS201]: Skill not found
```

### TTY detection and non-interactive behavior

1. Detect TTY using `isatty()` or equivalent for stdout/stderr
1. Automatically disable interactive elements (spinners, colors) when not in TTY
1. Respect `NO_COLOR` environment variable independently of TTY detection
1. Provide meaningful fallback messages for non-interactive environments
1. Handle pipe detection for appropriate output formatting

**TTY detection implementation:**

```rust
use std::io::IsTerminal;

fn is_tty() -> bool {
    std::io::stdout().is_terminal()
}

fn is_interactive() -> bool {
    std::io::stdout().is_terminal() && std::io::stderr().is_terminal()
}
```

**Behavior when stdout is not a TTY:**

- Disable spinners and progress bars
- Disable ANSI colors and styling
- Use simple text output without interactive elements
- Ensure machine-readable format for pipes
- Keep essential status messages

**Behavior when stderr is not a TTY:**

- Disable colored error output
- Use plain text error messages
- Ensure errors are still visible in logs
- Maintain error message structure for parsing

**Pipe detection patterns:**

```rust
// Detect if output is being piped
fn is_piped() -> bool {
    !std::io::stdout().is_terminal()
}

// Adjust output based on destination
if is_piped() {
    // Machine-readable output (JSON, simple text)
    println!("{}", serde_json::to_string(&result)?);
} else {
    // Human-readable output (tables, colors)
    print_colored_table(&results);
}
```

**TTY-aware code examples:**

```rust
// Progress indicator with TTY detection
if is_tty() && !no_progress {
    show_spinner("Installing skills...");
} else {
    println!("Installing skills...");
}

// Color usage with TTY detection
let message = "Operation completed";
if should_use_colors() {
    println!("{}", message.green());
} else {
    println!("✓ {}", message);
}
```

**Common TTY detection patterns:**

```rust
// Combined check for interactive features
fn enable_interactive_features() -> bool {
    is_tty() && std::env::var("NO_COLOR").is_err() && std::env::var("FASTSKILL_NO_PROGRESS").is_err()
}

// Conditional output based on environment
match (is_tty(), is_piped()) {
    (true, false) => show_interactive_output(),
    (false, true) => show_machine_output(),
    _ => show_plain_output(),
}
```

**Error code registry example:**

```markdown
## Error Code Reference

### FS001: Configuration Error
**Description:** Invalid or missing configuration file.
**Recovery:** Check the `[tool.fastskill]` section in `skill-project.toml` for syntax errors and required fields.

### FS101: Authentication Required
**Description:** Registry access requires authentication.
**Recovery:** Run `fastskill auth login` to authenticate.

### FS201: Skill Not Found
**Description:** Requested skill does not exist in registry.
**Recovery:** Use `fastskill search` to find similar skills.
```

#### Error message template

```
error: <summary>
[optional details]

hint: <actionable suggestion>
```

Use this format for consistent error presentation. Include error codes for documented, searchable errors.

#### Error message examples

**Simple error with hint:**

```
error: Skill 'web-scraper' not found

hint: Run 'fastskill search web scraper' to find similar skills
hint: Check available skills with 'fastskill list'
```

**Complex error with details and code:**

```
error: Failed to authenticate with registry [FS101]

The registry at https://registry.example.com requires authentication, but no valid token was found.

hint: Run 'fastskill auth login' to authenticate
hint: Set FASTSKILL_API_TOKEN environment variable
hint: Check that your token hasn't expired
```

**Error with context and multiple hints:**

```
error: Cannot install skill 'data-analyzer' due to dependency conflict [FS203]

Skill 'data-analyzer' requires 'pandas>=1.0.0', but 'pandas=0.25.3' is installed.
Conflicting skill: 'legacy-analyzer' (installed via requirements.txt)

hint: Update pandas to version 1.0.0 or higher
hint: Remove conflicting skill with 'fastskill remove legacy-analyzer'
hint: Use 'fastskill show data-analyzer' to see full dependency requirements
```

**Warning example:**

```
warning: Using deprecated skill 'old-parser'

This skill will be removed in FastSkill 2.0. Consider migrating to 'new-parser'.

hint: Run 'fastskill search parser' to find alternatives
hint: Update your skill requirements to use 'new-parser'
```

**Good vs bad examples:**

**Good - Actionable and clear:**

```
error: Network connection failed

hint: Check your internet connection
hint: Verify FASTSKILL_API_URL is accessible
```

**Bad - Unclear and unhelpful:**

```
error: Something went wrong

hint: Try again
```

**Good - Specific and recoverable:**

```
error: Permission denied writing to '/opt/fastskill/skills/'

hint: Run with sudo if installing system-wide
hint: Use FASTSKILL_SKILLS_DIR to specify a writable directory
hint: Check file permissions on the target directory
```

**Bad - Too technical, not user-focused:**

```
error: EACCES: permission denied, mkdir '/opt/fastskill/skills/'

hint: chmod +w /opt/fastskill/skills/
```

**Common patterns:**

- **Authentication errors**: Always suggest login commands and token setup
- **Network errors**: Suggest checking connectivity and retrying
- **Permission errors**: Suggest alternatives (sudo, different directories, permissions)
- **Dependency conflicts**: Show what's conflicting and how to resolve
- **Not found errors**: Suggest search commands and listing alternatives

**Anti-patterns to avoid:**

- Generic "try again" hints without specific guidance
- Technical jargon in user-facing messages
- Blaming the user ("you didn't set up X correctly")
- Overwhelming with too many hints (limit to 2-3 most relevant)
- Including volatile information (temporary file paths, timestamps)

### Exit codes and error severity

1. Use standard exit codes: `0` for success, `1` for general errors, `2` for misuse/invalid arguments
1. Reserve exit codes `3-125` for specific error conditions
1. Avoid exit codes `126-255` (shell reserved)
1. Exit immediately for fatal errors, continue processing for non-fatal issues when possible
1. Document exit codes in command help and error messages

**Exit code conventions:**

- `0`: Success, command completed normally
- `1`: General error (file not found, network failure, etc.)
- `2`: Command misuse (invalid arguments, missing required flags)

**Error severity levels:**

- **Fatal**: Exit immediately (invalid arguments, authentication failures)
- **Error**: Complete current operation, then exit (file system errors, network timeouts)
- **Warning**: Log but continue (deprecated features, performance issues)
- **Info**: Informational messages, no impact on exit code

**Exit code documentation:**

```rust
// In clap command definitions
#[command(
    about = "Install skills from registry",
    long_about = "Install skills from registry.

Exit codes:
    0 - Success
    1 - Installation failed
    2 - Invalid arguments"
)]
```

**When to exit vs continue:**

- Invalid arguments: Exit immediately with code 2
- Network failures: Retry with backoff, exit with code 1 if persistent
- File permission errors: Exit with code 1
- Missing dependencies: Continue if optional, exit if required

## Enforcement

Recommended tooling and automated checks to enforce these style guidelines:

- **Markdown**: Use prettier/markdownlint for line wrapping (100 chars), trailing spaces, and heading order.
- **Terminology**: Use vale/codespell/custom dictionary to enforce preferred terms like "lockfile" and "pre-release".
- **Code blocks**: Require language markers; forbid `bash` syntax for output (use `console` instead).
- **CLI strings**: Message lints for no trailing periods on single-sentence messages, no "simply", no "we".

## Internationalization

Even if not localizing yet, write with future translation in mind:

- Avoid string concatenation that assumes English word order.
- Keep user-visible literals (paths, skill names) as placeholders.
- Avoid idioms, jokes, and culturally-specific references in errors.

#### String externalization patterns

1. Use named constants or functions for user-visible strings
1. Avoid inline string literals in UI code
1. Group related strings in modules or constants
1. Use lazy_static or similar for string externalization

**Good patterns:**

```rust
// Define constants for user strings
const MSG_SKILL_INSTALLED: &str = "Skill '{}' installed successfully";
const MSG_SKILL_NOT_FOUND: &str = "Skill '{}' not found";

// Use in code
println!("{}", format!(MSG_SKILL_INSTALLED, skill_name));
eprintln!("{}", format!(MSG_SKILL_NOT_FOUND, skill_name));
```

**Avoid inline strings:**

```rust
// Bad: Inline user-visible strings
println!("Skill '{}' installed successfully", skill_name);

// Good: Externalized strings
println!("{}", messages::skill_installed(skill_name));
```

#### Placeholder naming conventions

1. Use descriptive placeholder names that indicate content type
1. Follow consistent naming patterns: `{skill_id}`, `{path}`, `{count}`
1. Use snake_case for multi-word placeholders
1. Include type hints when helpful: `{file_path}`, `{skill_name}`

**Placeholder examples:**

```rust
// Good placeholders
"Skill '{skill_name}' installed successfully"
"Found {count} skills in '{registry_name}'"
"Failed to read file '{file_path}'"

// Bad placeholders (too generic)
"Skill '{}' installed successfully"
"Found {} skills in '{}'"
```

#### Pluralization considerations

1. Design messages to handle both singular and plural forms
1. Use count-based logic for proper pluralization
1. Consider languages with different plural rules (not just singular/plural)

**Pluralization patterns:**

```rust
fn format_skill_count(count: usize) -> String {
    match count {
        0 => "No skills found".to_string(),
        1 => "Found 1 skill".to_string(),
        n => format!("Found {} skills", n),
    }
}
```

#### Date/time formatting standards

1. Use ISO 8601 format for machine-readable timestamps
1. Use locale-appropriate formats for human-readable dates
1. Include timezone information when relevant
1. Be consistent across all date/time displays

**Date formatting:**

```rust
// Machine-readable (logs, JSON)
let iso_timestamp = chrono::Utc::now().to_rfc3339();

// Human-readable (UI)
let human_date = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");

// Relative time for user messages
let relative = timeago::format(published_at, chrono::Utc::now());
println!("Published {}", relative);
```

#### Number formatting

1. Use appropriate digit grouping for large numbers
1. Handle decimal places consistently
1. Consider locale-specific number formatting
1. Be consistent across similar contexts

**Number formatting examples:**

```rust
// File sizes
format_bytes(size) => "1.2 MB", "45.6 KB"

// Counts
format_count(n) => "1,234 skills", "42 results"

// Percentages
format_percentage(ratio) => "85.3%"
```

#### Examples of i18n-ready vs non-ready code

**i18n-ready:**

```rust
// Externalized strings with named placeholders
const MSG_INSTALL_SUCCESS: &str = "Skill '{skill_name}' installed successfully from {registry_name}";
const MSG_SEARCH_RESULTS: &str = "Found {count} skills matching '{query}'";

println!("{}", format!(MSG_INSTALL_SUCCESS,
    skill_name = skill.name,
    registry_name = registry.name));
```

**Not i18n-ready:**

```rust
// String concatenation (assumes English word order)
println!("Successfully installed {} from {}", skill.name, registry.name);

// Inline literals
println!("Found {} skills matching '{}'", count, query);

// Culturally specific idioms
println!("Skill '{}' is a piece of cake to install", skill_name); // Idiom
println!("That's all folks!", skill_name); // Cultural reference
```
