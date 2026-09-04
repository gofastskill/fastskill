//! `analyze` must never report success without producing an analysis.
//!
//! CONTEXT.md ("Vector index") states the precondition: the index is *"only
//! meaningful when an Embedding provider is configured ... every consumer
//! (`search --local`, `analyze`) inherits the same provider precondition"*.
//! An **unmet** precondition is an error, not a silent exit 0. `reindex` is the
//! single sanctioned *skip* — it has nothing to report when there is nothing to
//! index — and it says so explicitly ("Reindex skipped: ...").
//!
//! Before this suite, all three `analyze` subcommands printed
//!
//! ```text
//! Note: semantic analysis requires an embedding provider. Results may be limited to structural analysis.
//! ```
//!
//! and exited **0** having performed no analysis at all: no structural analysis
//! existed behind that promise, and no result of any kind was printed. A caller
//! — human or CI — reads "exit 0, no duplicates found" on skills the command
//! never looked at. That is the worst failure shape for an analysis command, so
//! the gate is asserted here per subcommand.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use fastskill_core::{VectorIndexService, VectorIndexServiceImpl};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use tempfile::TempDir;

// --------------------------------------------------------------------------
// Fixtures
// --------------------------------------------------------------------------

/// A project manifest with **no** `[tool.fastskill.embedding]` section, i.e. no
/// Embedding provider. `ServiceConfig::embedding` is populated solely from this
/// section (`fastskill-cli/src/config.rs`), so its absence is what makes
/// `FastSkillService::vector_index_service()` return `None` — the state under
/// test.
const PROJECT_TOML_NO_PROVIDER: &str = r#"[dependencies]

[tool.fastskill]
skills_directory = ".claude/skills"
"#;

/// The same manifest **with** an Embedding provider configured.
const PROJECT_TOML_WITH_PROVIDER: &str = r#"[dependencies]

[tool.fastskill]
skills_directory = ".claude/skills"

[tool.fastskill.embedding]
openai_base_url = "https://api.openai.com/v1"
embedding_model = "text-embedding-3-small"
"#;

/// The two skills carry a **byte-identical** description. Any analysis worth
/// the name reports them; a command that returns "nothing" here is not
/// answering the question it was asked.
const TWIN_DESCRIPTION: &str = "A skill that processes text documents and extracts data.";

const TWINS: [&str; 2] = ["twin-a", "twin-b"];

fn workspace_with_twin_skills(project_toml: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let skills_dir = temp.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(temp.path().join("skill-project.toml"), project_toml).unwrap();

    for id in TWINS {
        let skill_dir = skills_dir.join(id);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!(
                "---\nname: {id}\ndescription: {TWIN_DESCRIPTION}\nversion: 1.0.0\n---\n\n# {id}\n"
            ),
        )
        .unwrap();
    }

    (temp, skills_dir)
}

/// Run `fastskill <args>` in `working_dir`.
///
/// Deliberately not `snapshot_helpers::run_fastskill_command_with_env`: that
/// helper can only *set* variables, and these tests must run with
/// `OPENAI_API_KEY` explicitly **removed** rather than at the mercy of the
/// developer's shell.
fn run_fastskill(args: &[&str], working_dir: &Path) -> Output {
    std::process::Command::new(super::snapshot_helpers::get_binary_path())
        .args(args)
        .current_dir(working_dir)
        .env_remove("OPENAI_API_KEY")
        .output()
        .expect("failed to execute fastskill")
}

// --------------------------------------------------------------------------
// No Embedding provider -> hard failure, never a silent success
// --------------------------------------------------------------------------

fn assert_analyze_refuses_without_provider(subcommand: &str) {
    let (temp, _skills_dir) = workspace_with_twin_skills(PROJECT_TOML_NO_PROVIDER);
    let output = run_fastskill(&["analyze", subcommand], temp.path());

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // The load-bearing assertion: an unmet precondition is a failure. Exit 0
    // here is indistinguishable, to any caller, from "analysed, found nothing".
    assert!(
        !output.status.success(),
        "`analyze {subcommand}` without an embedding provider must exit non-zero \
         (it performs no analysis, so exit 0 reads as a clean result);\n\
         status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status,
    );

    // ...and it must not fail mutely either.
    assert!(
        !(stdout.trim().is_empty() && stderr.trim().is_empty()),
        "`analyze {subcommand}` must explain why it refused; both streams were empty"
    );

    assert!(
        stderr.contains("requires an embedding provider"),
        "`analyze {subcommand}` must name the unmet precondition on stderr, got:\n{stderr}"
    );
    assert!(
        stderr.contains("doctor"),
        "`analyze {subcommand}` must point at `fastskill doctor` for setup guidance, got:\n{stderr}"
    );

    // No promise the code does not keep. The removed wording claimed results
    // would be "limited to structural analysis"; no structural analysis exists,
    // and that unfulfilled promise is what made the silent exit 0 look
    // deliberate.
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        !combined.contains("structural analysis"),
        "`analyze {subcommand}` must not promise a structural-analysis fallback that does not \
         exist, got:\n{combined}"
    );
}

#[test]
fn test_analyze_duplicates_fails_without_embedding_provider() {
    assert_analyze_refuses_without_provider("duplicates");
}

#[test]
fn test_analyze_matrix_fails_without_embedding_provider() {
    assert_analyze_refuses_without_provider("matrix");
}

#[test]
fn test_analyze_cluster_fails_without_embedding_provider() {
    assert_analyze_refuses_without_provider("cluster");
}

// --------------------------------------------------------------------------
// Embedding provider configured + index built -> real output
// --------------------------------------------------------------------------

/// Index the two twin skills with (near-)identical unit vectors, so a correct
/// analysis reports them as a ~1.0 pair. Writing the vectors directly keeps the
/// test off a live embedding endpoint while still exercising the whole CLI
/// path.
async fn index_twins(skills_dir: &Path) {
    let index = VectorIndexServiceImpl::with_default_path(skills_dir);
    let embeddings = [vec![1.0, 0.0, 0.0, 0.0], vec![1.0, 0.0, 0.0, 0.0]];
    for (id, embedding) in TWINS.iter().zip(embeddings) {
        index
            .add_or_update_skill(
                id,
                skills_dir.join(id),
                serde_json::json!({ "name": id, "description": TWIN_DESCRIPTION }),
                embedding,
                "test_hash",
            )
            .await
            .unwrap();
    }
}

/// The positive half of the contract: with the precondition **met**, all three
/// subcommands still produce real output. Without this, "make it fail" could be
/// satisfied by a gate that never opens.
///
/// `cluster` and `duplicates` also have richer fixtures in
/// `analyze_cluster_tests.rs`; `matrix` had no wired coverage at all before
/// this test.
#[tokio::test]
async fn test_analyze_produces_output_with_provider_and_index() {
    let (temp, skills_dir) = workspace_with_twin_skills(PROJECT_TOML_WITH_PROVIDER);
    index_twins(&skills_dir).await;

    // matrix
    let output = run_fastskill(&["analyze", "matrix"], temp.path());
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        output.status.success(),
        "`analyze matrix` with a provider and an index must succeed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("Similarity Matrix"),
        "`analyze matrix` must print the matrix, got:\n{stdout}"
    );
    for id in TWINS {
        assert!(
            stdout.contains(id),
            "`analyze matrix` must list `{id}`, got:\n{stdout}"
        );
    }

    // duplicates — the twins are a critical-severity pair (similarity 1.0).
    let output = run_fastskill(&["analyze", "duplicates"], temp.path());
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        output.status.success(),
        "`analyze duplicates` with a provider and an index must succeed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for id in TWINS {
        assert!(
            stdout.contains(id),
            "`analyze duplicates` must report the identically-described pair, missing `{id}` in:\n{stdout}"
        );
    }

    // cluster
    let output = run_fastskill(&["analyze", "cluster", "-k", "1"], temp.path());
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        output.status.success(),
        "`analyze cluster` with a provider and an index must succeed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for id in TWINS {
        assert!(
            stdout.contains(id),
            "`analyze cluster` must place `{id}` in a cluster, got:\n{stdout}"
        );
    }
}
