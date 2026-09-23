use std::path::Path;
use std::process::{Command, Output};

use compass_history::{ExtractionFingerprint, HistoryStore, PublishRequest, Repository};
use compass_pr_intelligence::{GateState, PullRequestReport, RiskBand};

const SYNTHETIC_ENGINE_IDENTITY: &str = "historical-engine";

fn git(root: &Path, arguments: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned().into())
    }
}

fn run(root: &Path, arguments: &[&str]) -> Result<Output, Box<dyn std::error::Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_compass"))
        .args(arguments)
        .current_dir(root)
        .output()?)
}

fn initialize(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    git(root, &["init", "--quiet", "--initial-branch=main"])?;
    git(root, &["config", "user.name", "Compass Test"])?;
    git(root, &["config", "user.email", "compass@example.invalid"])?;
    std::fs::write(root.join("lib.rs"), "pub fn shared() -> u8 { 1 }\n")?;
    git(root, &["add", "lib.rs"])?;
    git(root, &["commit", "--quiet", "-m", "base"])?;
    Ok(())
}

fn build_history(
    root: &Path,
    commit: &str,
    profile_from: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = vec!["history", "build", commit];
    if let Some(source) = profile_from {
        arguments.extend(["--profile-from", source]);
    } else {
        arguments.push("--code-only");
    }
    let output = run(root, &arguments)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "history build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}

fn missing_revision(output: &Output) -> Result<String, Box<dyn std::error::Error>> {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let revision = stderr
        .split_once("revision ")
        .and_then(|(_, rest)| rest.split_once(" is not materialized"))
        .map(|(revision, _)| revision.to_owned())
        .ok_or_else(|| format!("missing explicit materialization error: {stderr}"))?;
    Ok(revision)
}

fn publish_historical_base(root: &Path, commit: &str) -> Result<(), Box<dyn std::error::Error>> {
    build_history(root, commit, None)?;
    let repository = Repository::discover(root)?;
    let commit = repository.resolve(commit)?;
    let history = HistoryStore::open_existing(&repository)?.ok_or("seeded history store")?;
    let current = history.preferred(&commit)?.ok_or("seeded realization")?;
    let completed = history.artifacts(&current.id)?;
    let mut historical_profile = current.version.build_profile;
    historical_profile.insert("compass_version", SYNTHETIC_ENGINE_IDENTITY)?;
    history.publish(PublishRequest {
        commit: commit.clone(),
        parents: repository.parents(&commit)?,
        profile: historical_profile,
        fingerprint: "a".repeat(64).parse::<ExtractionFingerprint>()?,
        artifacts: completed.artifacts,
        completion: completed.completion,
        make_preferred: true,
    })?;
    Ok(())
}

#[test]
fn local_review_requires_explicit_materialization() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    initialize(directory.path())?;
    let base = git(directory.path(), &["rev-parse", "HEAD"])?;
    git(directory.path(), &["checkout", "--quiet", "-b", "feature"])?;
    std::fs::write(directory.path().join("feature.rs"), "pub fn feature() {}\n")?;
    git(directory.path(), &["add", "feature.rs"])?;
    git(directory.path(), &["commit", "--quiet", "-m", "feature"])?;
    let head = git(directory.path(), &["rev-parse", "HEAD"])?;

    let output = run(
        directory.path(),
        &[
            "review", "--base", &base, "--head", &head, "--format", "json",
        ],
    )?;
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(missing_revision(&output)?, base);
    let repository = Repository::discover(directory.path())?;
    assert!(HistoryStore::open_existing(&repository)?.is_none());
    Ok(())
}

#[test]
fn local_review_writes_round_trippable_exact_report() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    initialize(directory.path())?;
    git(directory.path(), &["checkout", "--quiet", "-b", "feature"])?;
    std::fs::write(
        directory.path().join("feature.rs"),
        "pub fn feature() -> u8 { 2 }\n",
    )?;
    std::fs::write(
        directory.path().join("package.json"),
        "{\"dependencies\":{\"fixture-package\":\"1.0.0\"}}\n",
    )?;
    git(directory.path(), &["add", "feature.rs", "package.json"])?;
    git(directory.path(), &["commit", "--quiet", "-m", "feature"])?;
    let head = git(directory.path(), &["rev-parse", "HEAD"])?;
    git(directory.path(), &["checkout", "--quiet", "main"])?;
    std::fs::write(
        directory.path().join("main.rs"),
        "pub fn target() -> u8 { 3 }\n",
    )?;
    git(directory.path(), &["add", "main.rs"])?;
    git(directory.path(), &["commit", "--quiet", "-m", "target"])?;
    let base = git(directory.path(), &["rev-parse", "HEAD"])?;

    build_history(directory.path(), &base, None)?;
    let probe = run(
        directory.path(),
        &[
            "review", "--base", &base, "--head", &head, "--format", "json",
        ],
    )?;
    assert_eq!(probe.status.code(), Some(1));
    let comparison = missing_revision(&probe)?;
    build_history(directory.path(), &comparison, Some(&base))?;

    let report_path = directory.path().join("review.json");
    let output = run(
        directory.path(),
        &[
            "review",
            "--base",
            &base,
            "--head",
            &head,
            "--repo",
            "crabbuild/fixture",
            "--pull-request-number",
            "42",
            "--format",
            "json",
            "--output",
            report_path.to_str().ok_or("report path")?,
        ],
    )?;
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = PullRequestReport::from_json(&std::fs::read(report_path)?)?;
    assert_eq!(report.identity.pull_request_number, Some(42));
    assert_eq!(report.identity.repository.owner, "crabbuild");
    assert_eq!(report.identity.revisions.target_head, base);
    assert_eq!(report.identity.revisions.pull_request_head, head);
    assert_eq!(
        report.identity.revisions.merge_result.object_id(),
        Some(comparison.as_str())
    );

    let preserved_path = directory.path().join("bounded-review.md");
    std::fs::write(&preserved_path, "preserve-me")?;
    let bounded = run(
        directory.path(),
        &[
            "review",
            "--base",
            &base,
            "--head",
            &head,
            "--format",
            "markdown",
            "--max-output-bytes",
            "1",
            "--output",
            preserved_path.to_str().ok_or("bounded report path")?,
        ],
    )?;
    assert_eq!(bounded.status.code(), Some(1));
    assert!(bounded.stdout.is_empty());
    assert!(String::from_utf8_lossy(&bounded.stderr).contains("PR review output is"));
    assert_eq!(std::fs::read_to_string(preserved_path)?, "preserve-me");
    Ok(())
}

#[test]
fn local_review_rejects_incompatible_engine_profiles() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    initialize(directory.path())?;
    let base = git(directory.path(), &["rev-parse", "HEAD"])?;
    git(directory.path(), &["checkout", "--quiet", "-b", "feature"])?;
    std::fs::write(
        directory.path().join("lib.rs"),
        "pub fn shared() -> u8 { 2 }\n",
    )?;
    git(directory.path(), &["add", "lib.rs"])?;
    git(directory.path(), &["commit", "--quiet", "-m", "feature"])?;
    let head = git(directory.path(), &["rev-parse", "HEAD"])?;
    publish_historical_base(directory.path(), &base)?;

    let probe = run(
        directory.path(),
        &[
            "review", "--base", &base, "--head", &head, "--format", "json",
        ],
    )?;
    assert_eq!(probe.status.code(), Some(1));
    let comparison = missing_revision(&probe)?;
    build_history(directory.path(), &comparison, None)?;

    let output = run(
        directory.path(),
        &[
            "review", "--base", &base, "--head", &head, "--format", "json",
        ],
    )?;
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("realizations were produced by incompatible graph engines"),
        "stderr={stderr} stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
    Ok(())
}

#[test]
fn conflicted_review_is_unavailable_without_false_clean_gate()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    initialize(directory.path())?;
    git(directory.path(), &["checkout", "--quiet", "-b", "feature"])?;
    std::fs::write(
        directory.path().join("lib.rs"),
        "pub fn shared() -> u8 { 2 }\n",
    )?;
    git(directory.path(), &["add", "lib.rs"])?;
    git(directory.path(), &["commit", "--quiet", "-m", "feature"])?;
    let head = git(directory.path(), &["rev-parse", "HEAD"])?;
    git(directory.path(), &["checkout", "--quiet", "main"])?;
    std::fs::write(
        directory.path().join("lib.rs"),
        "pub fn shared() -> u8 { 3 }\n",
    )?;
    git(directory.path(), &["add", "lib.rs"])?;
    git(directory.path(), &["commit", "--quiet", "-m", "target"])?;
    let base = git(directory.path(), &["rev-parse", "HEAD"])?;
    build_history(directory.path(), &base, None)?;
    build_history(directory.path(), &head, Some(&base))?;

    let output = run(
        directory.path(),
        &[
            "review", "--base", &base, "--head", &head, "--format", "json",
        ],
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = PullRequestReport::from_json(&output.stdout)?;
    assert_eq!(report.advisory_risk.band, RiskBand::Unavailable);
    assert!(
        report
            .gates
            .iter()
            .all(|gate| gate.state != GateState::Fail)
    );
    assert!(
        report
            .gates
            .iter()
            .any(|gate| gate.state == GateState::Indeterminate)
    );
    Ok(())
}

#[test]
fn review_usage_errors_have_stable_exit_and_streams() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    initialize(directory.path())?;
    let output = run(directory.path(), &["review", "--unknown"])?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown option --unknown"));
    Ok(())
}
