use super::snapshot_helpers::run_fastskill_command;
use fastskill_evals::{effective_checks, load_checks, load_suite, CheckDefinition};
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn example_root() -> PathBuf {
    repo_root().join("skills/eval-authoring/examples/invoice-extraction")
}

#[test]
fn shipped_authoring_example_validates_and_exercises_every_asset() {
    let example = example_root();
    let project: toml::Value =
        toml::from_str(&fs::read_to_string(example.join("skill-project.toml")).unwrap()).unwrap();
    assert_eq!(
        project["metadata"]["id"].as_str(),
        Some("invoice-extraction")
    );
    let result = run_fastskill_command(&["eval", "validate", "--json"], Some(&example));
    assert!(result.success, "stderr: {}", result.stderr);
    let json: serde_json::Value = serde_json::from_str(&result.stdout).unwrap();
    assert_eq!(json["valid"], true);
    assert_eq!(json["case_count"], 3);
    assert_eq!(json["check_count"], 1);
    assert_eq!(json["judges"][0], "extraction-correctness");

    let suite = load_suite(&example.join("evals/prompts.csv")).unwrap();
    assert_eq!(
        suite
            .cases
            .iter()
            .filter(|case| case.should_trigger)
            .count(),
        2
    );
    assert_eq!(
        suite
            .cases
            .iter()
            .filter(|case| !case.should_trigger)
            .count(),
        1
    );
    let checks = load_checks(&example.join("evals/checks.toml")).unwrap();
    assert_eq!(checks.len(), 1);
    for case in &suite.cases {
        let effective = effective_checks(&checks, &case.id, case.should_trigger);
        let trigger = effective
            .iter()
            .find_map(|check| match check {
                CheckDefinition::SkillInvoked { expected, .. } => Some(*expected),
                _ => None,
            })
            .expect("every case must get a trigger expectation");
        assert_eq!(trigger, case.should_trigger);
        if case.should_trigger {
            assert!(
                case.extra
                    .get("expected")
                    .is_some_and(|value| value.contains("invoice_number")),
                "positive case {} needs judge-visible expected facts",
                case.id
            );
        }
    }

    let basic = fs::read_to_string(example.join("fixtures/basic.txt")).unwrap();
    assert!(basic.contains("INV-1042") && basic.contains("USD") && basic.contains("123.45"));
    let missing = fs::read_to_string(example.join("fixtures/missing-number.txt")).unwrap();
    assert!(!missing.contains("Invoice number:"));
    assert!(missing.contains("EUR") && missing.contains("78.90"));
    let prompt = fs::read_to_string(example.join("evals/judge-prompt.md")).unwrap();
    assert!(prompt.contains("{{rubric}}") && prompt.contains("{{output_contract}}"));
    assert!(prompt.contains("{{case.expected}}"));
    let notes = fs::read_to_string(example.join("evals/review-notes.md")).unwrap();
    assert!(notes.contains("Unsupported:") && notes.contains("not been executed"));
}

#[test]
fn contradictory_explicit_trigger_check_fails_for_the_intended_reason() {
    let source = example_root();
    let temp = tempfile::tempdir().unwrap();
    copy_tree(&source, temp.path());
    fs::write(
        temp.path().join("evals/checks.toml"),
        "[[check]]\nname = \"skill_invoked\"\nexpected = false\ncases = [\"invoice-basic\"]\n",
    )
    .unwrap();
    let result = run_fastskill_command(&["eval", "validate"], Some(temp.path()));
    assert!(!result.success);
    let evidence = format!("{}{}", result.stdout, result.stderr);
    assert!(evidence.contains("EVAL_CHECKS_INVALID"), "{evidence}");
    assert!(evidence.contains("invoice-basic"), "{evidence}");
}

#[test]
fn skill_references_and_local_links_are_complete() {
    let root = repo_root();
    let skill_root = root.join("skills/eval-authoring");
    let skill = fs::read_to_string(skill_root.join("SKILL.md")).unwrap();
    assert!(skill.starts_with("---\nname: eval-authoring\n"));
    for relative in [
        "references/semantics.md",
        "references/examples.md",
        "references/handoff.md",
        "examples/invoice-extraction/SKILL.md",
    ] {
        assert!(skill_root.join(relative).is_file(), "missing {relative}");
    }
    for relative in [
        "webdocs/evals-quality/setup.mdx",
        "webdocs/cli-reference/eval-command.mdx",
    ] {
        assert!(root.join(relative).is_file(), "missing {relative}");
    }
    assert!(skill.contains("FastSkill `0.9.230`"));
    assert!(skill.contains("outcome") && skill.contains("adherence"));
}

#[test]
fn purpose_built_authoring_fixtures_cover_required_branches() {
    let fixtures = repo_root().join("tests/fixtures/eval-authoring");
    let existing = fixtures.join("existing-suite");
    let validation = run_fastskill_command(&["eval", "validate", "--json"], Some(&existing));
    assert!(validation.success, "stderr: {}", validation.stderr);
    let validation_json: serde_json::Value = serde_json::from_str(&validation.stdout).unwrap();
    assert_eq!(validation_json["valid"], true);
    assert_eq!(validation_json["case_count"], 1);
    for relative in [
        "invoice-no-evals/SKILL.md",
        "invoice-no-evals/fixtures/basic.txt",
        "invoice-no-evals/fixtures/missing-number.txt",
        "invoice-no-evals/fixtures/ambiguous-date.txt",
        "invoice-no-evals/fixtures/multiple-currencies.txt",
        "defective-total/SKILL.md",
        "defective-total/ground-truth.md",
        "unnecessary-step/SKILL.md",
        "unnecessary-step/scripted-answers.md",
        "unsupported-order/SKILL.md",
        "existing-suite/evals/user-note.md",
        "environment-variants.md",
        "scripted-workflow.md",
    ] {
        assert!(fixtures.join(relative).is_file(), "missing {relative}");
    }

    let ground_truth =
        fs::read_to_string(fixtures.join("defective-total/ground-truth.md")).unwrap();
    assert!(ground_truth.contains("17.00") && ground_truth.contains("15.00"));
    let answers =
        fs::read_to_string(fixtures.join("unnecessary-step/scripted-answers.md")).unwrap();
    assert!(answers.contains("not mandatory") && answers.contains("outcome correctness wins"));
    let unsupported = fs::read_to_string(fixtures.join("unsupported-order/SKILL.md")).unwrap();
    assert!(unsupported.contains("exact tool-call order"));
    let note = fs::read_to_string(fixtures.join("existing-suite/evals/user-note.md")).unwrap();
    assert_eq!(
        note,
        "# User note\n\nKeep this unrelated note byte-for-byte when refining the suite.\n"
    );
    let variants = fs::read_to_string(fixtures.join("environment-variants.md")).unwrap();
    for branch in [
        "incompatible CLI",
        "missing runtime",
        "missing judge credential",
        "invalid reference",
        "unavailable observation",
    ] {
        assert!(
            variants.contains(branch),
            "missing controlled branch {branch}"
        );
    }
    let workflow = fs::read_to_string(fixtures.join("scripted-workflow.md")).unwrap();
    for step in 1..=8 {
        assert!(workflow.contains(&format!("{step}. ")), "missing WF{step}");
    }
}

fn copy_tree(source: &Path, target: &Path) {
    for entry in walkdir::WalkDir::new(source) {
        let entry = entry.unwrap();
        let relative = entry.path().strip_prefix(source).unwrap();
        let destination = target.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&destination).unwrap();
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
}
