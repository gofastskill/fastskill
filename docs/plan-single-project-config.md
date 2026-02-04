# Implementation Plan: Single Project Config and Mandatory skills_directory

## Goal

1. **Detection**: If `skill-project.toml` exists and is project-level with valid config, use it. If not found (or not project-level when consumer config is needed), fail and tell user to run `fastskill init`. No fallbacks to `.claude/skills` or walk-up for skills dir.
2. **Mandatory skills_directory**: In project-level `skill-project.toml`, `[tool.fastskill]` and `skills_directory` are required.
3. **Single source for paths**: One loader loads and validates the project file and returns a single struct; all code that needs project root, project file path, or skills directory uses that loader. No hardcoded `.claude/skills` or `.claude` in logic.
4. **Init**: For project-level init, user must specify skills location (`--skills-dir`); write `[tool.fastskill]` with `skills_directory`. No coding-agent option for now (deferred).

---

## Phase 1: Core loader and types

### 1.1 Add `project_config` module in core

- **New file**: `src/core/project_config.rs`
  - Define `ProjectConfig` struct:
    - `project_root: PathBuf` (directory containing `skill-project.toml`)
    - `project_file_path: PathBuf` (path to `skill-project.toml`)
    - `skills_directory: PathBuf` (resolved, absolute or relative to project root)
  - Define `load_project_config(start_path: &Path) -> Result<ProjectConfig, String>`:
    1. Call `project::resolve_project_file(start_path)`.
    2. If `!found`, return error: "skill-project.toml not found in this directory or any parent. Create it at the top level of your workspace (e.g. run 'fastskill init' there), then run this command again."
    3. If `found` and `context != ProjectContext::Project`, return error: "skill-project.toml here is for a skill (same directory as SKILL.md). Run install/add/list/update from the project root that has [dependencies] and [tool.fastskill] with skills_directory."
    4. Load `SkillProjectToml::load_from_file(&project_file_path)`.
    5. Validate: require `project.tool.as_ref().and_then(|t| t.fastskill.as_ref()).and_then(|f| f.skills_directory.as_ref()).is_some()`. If missing, return error: "project-level skill-project.toml requires [tool.fastskill] with skills_directory. Run 'fastskill init --skills-dir <path>' at project root or add it manually."
    6. Resolve `skills_directory`: if relative, join with `project_file_path.parent().unwrap_or(Path::new("."))`; optionally `canonicalize` or keep as-is per existing behavior.
    7. Return `ProjectConfig { project_root, project_file_path, skills_directory }`.
  - Export from `src/core/mod.rs`: `pub mod project_config;` and re-export `ProjectConfig`, `load_project_config` (or keep loader in CLI and call from core; see below).

- **Placement**: Loader can live in `core` so both CLI and HTTP use it. Error type: use `String` in core; CLI wraps in `CliError::Config(...)` where it calls the loader.

### 1.2 Manifest: require skills_directory for project context

- **File**: `src/core/manifest.rs`
  - In `validate_for_context`, for `ProjectContext::Project`: add check that `self.tool.as_ref().and_then(|t| t.fastskill.as_ref()).map(|f| f.skills_directory.is_some()).unwrap_or(false)` is true. If not, return Err with message: "Project-level skill-project.toml requires [tool.fastskill] with skills_directory. Run 'fastskill init --skills-dir <path>' or add [tool.fastskill] with skills_directory = \"...\"."
  - Keep `FastSkillToolConfig.skills_directory` as `Option<PathBuf>` in the struct (for backward compatibility and skill-level manifests that may omit it); validation enforces presence when context is Project.

---

## Phase 2: CLI uses single loader; remove fallbacks

### 2.1 Config: replace `resolve_skills_storage_directory` with loader

- **File**: `src/cli/config.rs`
  - Remove `walk_up_for_skills_dir` and all logic that uses `.claude/skills` (Priority 2 and 3).
  - Change `resolve_skills_storage_directory()` to:
    1. Call `fastskill::core::project_config::load_project_config(&current_dir)` (or equivalent; if loader returns a custom error type, map to `CliError::Config`).
    2. On Err, return `Err(CliError::Config(message))`.
    3. On Ok(config), return `Ok(config.skills_directory)`.
  - Change `get_skill_search_locations_for_display()` to use the loader: call `load_project_config`; on Ok return `vec![(config.skills_directory, "project".to_string())]`; on Err return the same Err (caller already uses this for error display). Remove hardcoded `home.join(".claude/skills")` for "global" unless product decision is to keep a global fallback (if keep, document it; otherwise remove).
  - `create_service_config`: unchanged in signature; it already calls `resolve_skills_storage_directory()`, so it will get the new behavior (fail when no project file or missing skills_directory).

### 2.2 Commands that need project file path and/or skills dir

- **Files**: `src/cli/commands/install.rs`, `update.rs`, `list.rs`, `add.rs`, `remove.rs`, and `src/cli/utils/manifest_utils.rs`
  - Where they currently call `resolve_project_file(&current_dir)` and then later `resolve_skills_storage_directory()` or `create_service_config()`: optionally refactor to call `load_project_config()` once and pass `config.project_file_path` and use `config.skills_directory` (or keep calling `resolve_skills_storage_directory()` and `resolve_project_file()` separately; the important part is that `resolve_skills_storage_directory` no longer has fallbacks and requires project file + skills_directory).
  - Ensure they do not rely on "no project file" path: install/update/list already fail when `!project_file_result.found` with `manifest_required_message()`; keep that. Add/remove/manifest_utils already fail when project file not found; keep that.

### 2.3 Remove hardcoded paths in CLI

- **File**: `src/cli/commands/show.rs`
  - Remove default `PathBuf::from(".claude")` for config base path. Use project root or project file path from loader; if show is allowed without a project file (e.g. global show), define behavior explicitly (e.g. require project file and use loader, or keep a single fallback only for show; prefer requiring project file for consistency).

- **File**: `src/cli/commands/install.rs`
  - Remove `fs::create_dir_all(".claude")`; create the directory from resolved `skills_directory` (service config already has `skill_storage_path`; ensure the code that creates the dir uses that path, not `.claude`). Update tests that assert on ".claude" or create `.claude`.

- **File**: `src/cli/commands/update.rs`
  - Remove `fs::remove_dir_all(".claude")` in tests; use temp dir or resolved path from config.

- **File**: `src/cli/utils/manifest_utils.rs`
  - Remove comment "Ensure .claude directory exists". Ensure parent of lock file exists (use project root or project file parent from context); if this code path only runs when project file exists, parent is already available.

- **File**: `src/cli/cli.rs`
  - Update long_about and `--repositories` help text: remove or reword references to ".claude/skills" and ".claude/repositories.toml"; point to skill-project.toml and that skills directory comes from [tool.fastskill].skills_directory.

- **File**: `src/cli/commands/mod.rs`
  - Update install command description: remove ".claude/skills/"; say "Install skills from skill-project.toml [dependencies] into the skills directory configured in [tool.fastskill].skills_directory."

---

## Phase 3: Init requires skills dir for project-level

### 3.1 Init args and behavior

- **File**: `src/cli/commands/init.rs`
  - Add `--skills-dir <path>` argument (required when context is project-level; optional when skill-level).
  - Before building `SkillProjectToml`, detect context (skill-level if SKILL.md in current dir, else project-level).
  - **Project-level** (no SKILL.md in current dir):
    - Require `--skills-dir` (or prompt when not `--yes`). If missing and not `--yes`, return Err: "Project-level init requires --skills-dir <path>."
    - Set `tool: Some(ToolSection { fastskill: Some(FastSkillToolConfig { skills_directory: Some(PathBuf::from(args.skills_dir)), embedding: None, repositories: None }) })` in the created `SkillProjectToml` (use actual arg name and type).
    - Write real `[tool.fastskill]` with `skills_directory = "..."` in the generated file; do not leave it only in comments.
  - **Skill-level** (SKILL.md present):
    - Do not require `--skills-dir`; keep current behavior (optional commented [tool.fastskill]).
  - Remove or update the appended commented block so it does not suggest skills_directory is optional for project-level; for project-level the generated file must have uncommented [tool.fastskill] with skills_directory.

### 3.2 Init validation after write

- After writing the file, `validate_for_context(context)` is already called; with the new validation in manifest (Phase 1.2), project-level manifest will require skills_directory, so the init-created project-level file will pass only if we wrote [tool.fastskill].skills_directory.

---

## Phase 4: HTTP server uses same source

### 4.1 Server and manifest handler

- **File**: `src/http/server.rs`
  - Replace `resolve_project_file_path()` implementation: use `core::project_config::load_project_config(&current_dir)`. If Err, the server may still start but project-dependent routes could return 500 or 404; preferable: at server build time, call loader and store `ProjectConfig` (or at least project_file_path and skills_directory) in state so handlers use it. Alternatively, keep resolving per-request in handlers; then ensure handlers call the same loader and fail consistently.
  - Recommended: when creating the server, call `load_project_config(env::current_dir())`; if Err, either fail server startup with a clear message or store the error and return it from /api/project and other project endpoints. Store `ProjectConfig` (or its fields) in `AppState` so handlers do not re-resolve; single load at startup.

- **File**: `src/http/handlers/manifest.rs`
  - Use skills directory and project path from state (set from `ProjectConfig` in server). Replace hardcoded `.cursor` for rules output path only if you have a config field for it (deferred per plan); otherwise leave as-is.
  - For GET /api/project and similar: get `skills_directory` from loaded project config (or from SkillProjectToml loaded from state's project_file_path); remove fallback to string ".claude/skills". Use the value from the loaded config.

---

## Phase 5: Config file and other references

### 5.1 Config file loader

- **File**: `src/cli/config_file.rs`
  - `load_config_from_skill_project`: already uses `resolve_project_file`; when it loads and returns `FastSkillConfig`, it gets `skills_directory` from the manifest. No change needed for loading; the manifest now requires it for project-level, and the single loader (project_config) will have already validated it before any consumer uses it.

### 5.2 Default in config_file

- **File**: `src/cli/config_file.rs`
  - Comment "Default: \".claude/skills\"" on `skills_directory`: update to "Required in project-level skill-project.toml; no default."

### 5.3 Error messages

- **File**: `src/cli/error.rs`
  - `manifest_required_message()` and `manifest_required_for_add_message()`: ensure they say "run 'fastskill init' there" (already do). No change required unless you want to add "use --skills-dir for project-level".

---

## Phase 6: Tests and docs

### 6.1 Tests

- Update tests that create a project without [tool.fastskill] or without skills_directory: they should now fail or create a valid project with skills_directory.
- Update tests that rely on `.claude/skills` or `resolve_skills_storage_directory` returning a default: they should create a skill-project.toml with [tool.fastskill].skills_directory or expect an error.
- Add tests for `load_project_config`: not found; found but skill-level; found project-level but missing skills_directory; found project-level with skills_directory (success).

### 6.2 Docs and comments

- README, webdocs, and in-code comments: replace "default: .claude/skills" with "required in project-level skill-project.toml; set via fastskill init --skills-dir or [tool.fastskill].skills_directory."

---

## File checklist (summary)

| Area | File | Change |
|------|------|--------|
| Core | `src/core/project_config.rs` | **New**: ProjectConfig, load_project_config() |
| Core | `src/core/mod.rs` | Add mod project_config; re-export |
| Core | `src/core/manifest.rs` | validate_for_context(Project): require [tool.fastskill].skills_directory |
| CLI | `src/cli/config.rs` | resolve_skills_storage_directory + get_skill_search_locations use loader; remove walk_up and .claude/skills fallbacks |
| CLI | `src/cli/commands/init.rs` | Add --skills-dir; project-level: require it, write [tool.fastskill].skills_directory |
| CLI | `src/cli/commands/show.rs` | Remove default .claude; use loader or require project |
| CLI | `src/cli/commands/install.rs` | Remove create_dir_all(".claude"); use service config path; tests |
| CLI | `src/cli/commands/update.rs` | Tests: no remove_dir_all(".claude") |
| CLI | `src/cli/utils/manifest_utils.rs` | Remove .claude comment; use project root for lock parent if needed |
| CLI | `src/cli/cli.rs` | Help text: no .claude/skills default |
| CLI | `src/cli/commands/mod.rs` | Install description: skills dir from config |
| CLI | `src/cli/config_file.rs` | Comment: skills_directory required, no default |
| HTTP | `src/http/server.rs` | Use load_project_config; store in state; fail or propagate |
| HTTP | `src/http/handlers/manifest.rs` | Use config from state for skills_directory; no ".claude/skills" fallback |
| Tests | Various | Expect project file + skills_directory or Err; no .claude default |

---

## Order of implementation

1. Phase 1.1 and 1.2 (core loader + manifest validation).
2. Phase 2.1 (config.rs: single loader, remove fallbacks).
3. Phase 2.2 and 2.3 (commands and hardcoded path removal).
4. Phase 3 (init --skills-dir and project-level write).
5. Phase 4 (HTTP server and manifest handler).
6. Phase 5 and 6 (config_file, errors, tests, docs).
