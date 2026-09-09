use crate::core::test_helpers::TestRepo;
use crate::core::weave;
use crate::trace;

/// Regression (#159): `continue_rebase` must capture git's output and route it
/// to the trace, instead of running with inherited stdio and leaking git's
/// "Successfully rebased" / "Updated refs" messages to the terminal. Before the
/// fix it logged nothing at all, so asserting the trace records the step proves
/// the output is now captured (you can only log stderr you captured).
#[test]
fn continue_rebase_captures_output_to_trace() {
    let test_repo = TestRepo::new();
    let c1 = test_repo.commit("first", "a.txt");
    test_repo.commit("second", "b.txt");
    let workdir = test_repo.workdir();

    // Pause a rebase at the first commit so there is something to continue.
    weave::start_edit_rebase(&test_repo.repo, &workdir, c1).unwrap();

    // The trace logger is thread-local and cargo reuses threads across tests;
    // clear any logger a prior test leaked so our init reliably takes effect.
    let _ = trace::finalize();
    let git_dir = test_repo.repo.path().to_path_buf();
    trace::init(&git_dir, "git loom fold");
    let outcome = super::continue_rebase(&workdir).unwrap();
    let log_path = trace::finalize().expect("trace should have recorded an entry");

    assert!(matches!(outcome, super::RebaseOutcome::Completed));
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        content.contains("[git] rebase --continue"),
        "trace should record the continue step, got:\n{content}"
    );
}

/// A rebase whose todo still has an `edit` step ahead of it is not over when
/// `git rebase --continue` exits 0: it merely advanced to that next step.
/// Reporting `Completed` there would let the caller finish off a command while
/// the repository sits detached mid-rebase.
#[test]
fn continue_rebase_reports_paused_at_next_edit() {
    let test_repo = TestRepo::new();
    let base = test_repo.commit("base", "base.txt");
    let c1 = test_repo.commit("first", "a.txt");
    let c2 = test_repo.commit("second", "b.txt");
    let c3 = test_repo.commit("third", "c.txt");
    let workdir = test_repo.workdir();
    let git_dir = test_repo.repo.path().to_path_buf();

    // Two `edit` steps: the rebase stops at the first, and continuing from it
    // stops at the second.
    let todo = format!("label onto\n\nreset onto\nedit {c1}\nedit {c2}\npick {c3}\n");
    assert_eq!(
        weave::run_rebase(&workdir, Some(&base.to_string()), &todo).unwrap(),
        super::RebaseOutcome::Paused,
        "the rebase stops at the first `edit`, it has not completed"
    );

    assert_eq!(
        super::continue_rebase(&workdir).unwrap(),
        super::RebaseOutcome::Paused,
        "the second `edit` is still ahead — the rebase is not over"
    );

    // Only once the last `edit` is passed does it actually finish.
    assert_eq!(
        super::continue_rebase(&workdir).unwrap(),
        super::RebaseOutcome::Completed
    );
    assert!(!super::rebase_is_in_progress(&git_dir));
}

/// `git rebase --continue` with no rebase in progress is a caller bug, not a
/// conflict: reporting `Stopped` would send the user off to resolve conflicts
/// that do not exist.
#[test]
fn continue_rebase_without_a_rebase_is_an_error() {
    let test_repo = TestRepo::new();
    test_repo.commit("first", "a.txt");
    let workdir = test_repo.workdir();

    let err = super::continue_rebase(&workdir).unwrap_err();
    assert!(
        err.to_string().contains("git rebase failed"),
        "expected the rebase failure itself, got: {err}"
    );
}

/// A command can fail before its rebase ever starts — the worktree check, the
/// git-dir lookup, a missing loom binary. There is nothing to abort then, so
/// the cleanup must still run: skipping it strands the temp branch, saved
/// patch or state file the caller was about to remove.
#[test]
fn cleanup_runs_when_there_was_no_rebase_to_abort() {
    let test_repo = TestRepo::new();
    test_repo.commit("first", "a.txt");
    let workdir = test_repo.workdir();
    assert!(!super::rebase_is_in_progress(test_repo.repo.path()));

    let mut cleaned = false;
    let err =
        super::rebase_abort_then_cleanup(&workdir, anyhow::anyhow!("boom"), || cleaned = true);

    assert!(cleaned, "nothing was running, so the cleanup must happen");
    assert_eq!(
        err.to_string(),
        "boom",
        "the command's own failure is what the user needs to see"
    );
}

/// With a rebase actually running, the abort has to happen before the cleanup,
/// and the caller's error is still the one reported.
#[test]
fn a_live_rebase_is_aborted_before_the_cleanup_runs() {
    let test_repo = TestRepo::new();
    let base = test_repo.commit("base", "base.txt");
    let c1 = test_repo.commit("first", "a.txt");
    let workdir = test_repo.workdir();

    let todo = format!("label onto\n\nreset onto\nedit {c1}\n");
    weave::run_rebase(&workdir, Some(&base.to_string()), &todo).unwrap();
    assert!(super::rebase_is_in_progress(test_repo.repo.path()));

    let mut cleaned = false;
    let err =
        super::rebase_abort_then_cleanup(&workdir, anyhow::anyhow!("boom"), || cleaned = true);

    assert!(cleaned, "the abort worked, so the cleanup must follow");
    assert!(
        !super::rebase_is_in_progress(test_repo.repo.path()),
        "the rebase should be gone"
    );
    assert_eq!(err.to_string(), "boom");
}

/// When the abort fails the rebase is still running, so the cleanup is skipped
/// — and the reported error must still carry the original failure, not replace
/// it with the hint.
#[test]
fn a_failed_abort_skips_the_cleanup_and_keeps_the_cause() {
    let test_repo = TestRepo::new();
    let base = test_repo.commit("base", "base.txt");
    let c1 = test_repo.commit("first", "a.txt");
    let workdir = test_repo.workdir();

    let todo = format!("label onto\n\nreset onto\nedit {c1}\n");
    weave::run_rebase(&workdir, Some(&base.to_string()), &todo).unwrap();

    // A held index.lock makes the abort fail, as a concurrent git process would.
    let lock = test_repo.repo.path().join("index.lock");
    std::fs::write(&lock, b"").unwrap();

    let mut cleaned = false;
    let err =
        super::rebase_abort_then_cleanup(&workdir, anyhow::anyhow!("boom"), || cleaned = true);

    assert!(
        !cleaned,
        "cleaning up on top of a live rebase is what this guards against"
    );
    let msg = err.to_string();
    assert!(msg.contains("boom"), "the cause must survive, got: {msg}");
    assert!(msg.contains("left mid-rebase"), "{msg}");

    std::fs::remove_file(&lock).unwrap();
    super::rebase_abort(&workdir).unwrap();
}

/// Build a repo where `rerere` has recorded a resolution for a conflict, then
/// replay that same conflict in a rebase. With `rerere.autoUpdate` on, git
/// stages the recorded resolution and the stop leaves a clean index — which
/// must not be mistaken for a rebase that broke down.
#[test]
fn rerere_resolved_stop_is_still_a_conflict() {
    let test_repo = TestRepo::new();
    test_repo.set_config("rerere.enabled", "true");
    test_repo.set_config("rerere.autoUpdate", "true");
    let workdir = test_repo.workdir();

    test_repo.write_file("f.txt", "base\n");
    test_repo.stage_files(&["f.txt"]);
    test_repo.commit_staged("base");
    let base = test_repo.head_oid().to_string();

    test_repo.write_file("f.txt", "onto side\n");
    test_repo.stage_files(&["f.txt"]);
    test_repo.commit_staged("onto side");
    let onto = test_repo.head_oid().to_string();

    // The same conflicting topic twice: the first rebase records the
    // resolution, the second one has rerere replay it.
    let conflict_on = |topic: &str| {
        test_repo.create_branch_at(topic, &base);
        test_repo.switch_branch(topic);
        test_repo.write_file("f.txt", "topic side\n");
        test_repo.stage_files(&["f.txt"]);
        test_repo.commit_staged("topic side");
        crate::git::run_git(&workdir, &["rebase", &onto]).unwrap_err();
        assert!(
            super::rebase_is_in_progress(test_repo.repo.path()),
            "the conflict must be what stopped the rebase"
        );
    };

    conflict_on("topic1");
    test_repo.write_file("f.txt", "resolved\n");
    crate::git::run_git(&workdir, &["add", "f.txt"]).unwrap();
    assert_eq!(
        super::continue_rebase(&workdir).unwrap(),
        super::RebaseOutcome::Completed
    );

    conflict_on("topic2");

    assert!(
        super::rebase_is_in_progress(test_repo.repo.path()),
        "the replayed conflict still stops the rebase"
    );
    assert_eq!(
        test_repo.read_file("f.txt"),
        "resolved\n",
        "rerere should have replayed the recorded resolution"
    );
    assert!(
        !super::has_unmerged_paths(&workdir),
        "rerere staged its resolution, so nothing is left unmerged"
    );
    assert!(
        super::auto_merge_id(&workdir).is_some(),
        "the stop must still count as a conflict"
    );

    let err = super::abort_after_failure(&workdir).to_string();
    assert!(
        err.contains("Rebase failed with conflicts"),
        "a stop rerere resolved is still a conflict to report: {err}"
    );
}

/// The reftable backend keeps refs in `.git/reftable/`, so `AUTO_MERGE` is no
/// file under the git dir there — reading it must go through git.
#[test]
fn auto_merge_id_works_on_a_reftable_repo() {
    let dir = tempfile::tempdir().unwrap();
    let workdir = dir.path().to_path_buf();
    let init = std::process::Command::new("git")
        .current_dir(&workdir)
        .args(["init", "--ref-format=reftable"])
        .output()
        .unwrap();
    if !init.status.success() {
        eprintln!("skipping: this git has no reftable backend");
        return;
    }
    for (key, value) in [("user.name", "Test"), ("user.email", "test@example.com")] {
        crate::git::run_git(&workdir, &["config", key, value]).unwrap();
    }

    let write = |content: &str| std::fs::write(workdir.join("f.txt"), content).unwrap();
    let commit = |message: &str| {
        crate::git::run_git(&workdir, &["add", "f.txt"]).unwrap();
        crate::git::run_git(&workdir, &["commit", "-m", message]).unwrap();
    };
    write("base\n");
    commit("base");
    crate::git::run_git(&workdir, &["branch", "topic"]).unwrap();
    write("onto side\n");
    commit("onto side");
    let onto = crate::git::run_git_stdout(&workdir, &["rev-parse", "HEAD"]).unwrap();
    crate::git::run_git(&workdir, &["switch", "topic"]).unwrap();
    write("topic side\n");
    commit("topic side");
    crate::git::run_git(&workdir, &["rebase", onto.trim()]).unwrap_err();

    assert!(
        !workdir.join(".git/AUTO_MERGE").exists(),
        "reftable keeps no AUTO_MERGE file — that is the point of this test"
    );
    assert!(
        super::auto_merge_id(&workdir).is_some(),
        "the conflict must be recognized on a reftable repo too"
    );
}

/// An untracked file in the way of a picked commit stops the rebase with a
/// clean index and no conflict — with `rerere` enabled too, which is what made
/// `MERGE_RR` useless as a signal: git writes it for any sequencer pick.
#[test]
fn untracked_file_stop_is_not_a_conflict() {
    let test_repo = TestRepo::new();
    test_repo.set_config("rerere.enabled", "true");
    let workdir = test_repo.workdir();

    test_repo.commit("base", "a.txt");
    let base = test_repo.head_oid().to_string();
    test_repo.write_file("foo.txt", "committed\n");
    test_repo.stage_files(&["foo.txt"]);
    crate::git::run_git(&workdir, &["commit", "-m", "add foo.txt"]).unwrap();
    let add_foo = test_repo.head_oid().to_string();
    crate::git::run_git(&workdir, &["rm", "-q", "foo.txt"]).unwrap();
    crate::git::run_git(&workdir, &["commit", "-m", "delete foo.txt"]).unwrap();
    let delete_foo = test_repo.head_oid().to_string();

    // Replaying "add foo.txt" on top of the deletion cannot write the file:
    // the user has an untracked one there.
    test_repo.write_file("foo.txt", "untracked\n");
    crate::git::run_git(
        &workdir,
        &["rebase", "--onto", &delete_foo, &base, &add_foo],
    )
    .unwrap_err();

    assert!(
        super::rebase_is_in_progress(test_repo.repo.path()),
        "the blocked pick stops the rebase"
    );
    assert!(!super::has_unmerged_paths(&workdir));
    assert!(
        super::auto_merge_id(&workdir).is_none(),
        "an untracked file in the way is not a conflict"
    );
}

#[test]
fn rebase_outcome_classifies_all_four_cases() {
    use super::{RebaseOutcome, rebase_outcome};
    let tmp = tempfile::tempdir().unwrap();
    let git_dir = tmp.path();
    let fail = || Err(anyhow::anyhow!("boom"));

    assert_eq!(
        rebase_outcome(git_dir, Ok(())).unwrap(),
        RebaseOutcome::Completed
    );
    assert!(rebase_outcome(git_dir, fail()).is_err());

    std::fs::create_dir(git_dir.join("rebase-merge")).unwrap();
    assert_eq!(
        rebase_outcome(git_dir, Ok(())).unwrap(),
        RebaseOutcome::Paused
    );
    assert_eq!(
        rebase_outcome(git_dir, fail()).unwrap(),
        RebaseOutcome::Stopped
    );
}
