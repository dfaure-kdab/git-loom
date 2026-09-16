use super::{filter_paths, selected_paths};
use crate::core::diff::DiffHunk;
use crate::core::repo;
use crate::core::test_helpers::TestRepo;
use crate::tui::hunk_selector::{FileEntry, HunkEntry, HunkOrigin};

/// A repo with changes to `a.rs` and `dir/b.rs`. It needs an upstream because
/// an argument that is not a path falls through to short-ID resolution.
fn repo_with_changes() -> TestRepo {
    let test_repo = TestRepo::new_with_remote();
    test_repo.write_file("a.rs", "one\n");
    std::fs::create_dir(test_repo.workdir().join("dir")).unwrap();
    test_repo.write_file("dir/b.rs", "one\n");
    test_repo.stage_files(&["a.rs", "dir/b.rs"]);
    test_repo.commit_staged("Add files");
    test_repo.write_file("a.rs", "changed\n");
    test_repo.write_file("dir/b.rs", "changed\n");
    test_repo
}

fn filter(test_repo: &TestRepo, args: &[&str]) -> anyhow::Result<Option<Vec<String>>> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    test_repo.in_dir(|| filter_paths(&repo::open_repo().unwrap(), &args))
}

#[test]
fn no_files_means_every_change() {
    assert_eq!(filter(&repo_with_changes(), &[]).unwrap(), None);
}

#[test]
fn zz_means_every_change_even_beside_a_path() {
    let test_repo = repo_with_changes();
    assert_eq!(filter(&test_repo, &["zz"]).unwrap(), None);
    assert_eq!(filter(&test_repo, &["a.rs", "zz"]).unwrap(), None);
}

#[test]
fn paths_resolve_to_themselves_including_below_the_root() {
    let filtered = filter(&repo_with_changes(), &["a.rs", "dir/b.rs"]).unwrap();
    assert_eq!(
        filtered,
        Some(vec!["a.rs".to_string(), "dir/b.rs".to_string()])
    );
}

#[test]
fn an_argument_naming_nothing_is_refused() {
    let err = filter(&repo_with_changes(), &["nope.rs"]).unwrap_err();
    assert!(err.to_string().contains("nope.rs"), "{err}");
}

/// The rationale for resolving once: a short ID names what is changed now.
#[test]
fn a_short_id_resolves_to_its_path() {
    let test_repo = repo_with_changes();
    let short_id = test_repo.in_dir(|| {
        let repo = repo::open_repo().unwrap();
        let info = repo::gather_repo_info(&repo, false, 1).unwrap();
        let allocator = crate::core::shortid::IdAllocator::new(info.collect_entities());
        allocator.get_file("a.rs").to_string()
    });
    assert_ne!(short_id, "a.rs", "the test needs a real short ID");
    assert_eq!(
        filter(&test_repo, &[short_id.as_str()]).unwrap(),
        Some(vec!["a.rs".to_string()])
    );
}

fn hunk(origin: HunkOrigin, selected: bool) -> HunkEntry {
    HunkEntry {
        hunk: DiffHunk {
            text: "@@ -1 +1 @@\n-a\n+b\n".to_string(),
            modified_lines: vec![1],
        },
        selected,
        origin,
    }
}

fn entry(path: &str, hunks: Vec<HunkEntry>) -> FileEntry {
    FileEntry {
        path: path.to_string(),
        hunks,
        index_status: 'M',
        worktree_status: 'M',
        binary: false,
    }
}

#[test]
fn a_file_with_no_selected_hunk_is_not_folded() {
    let entries = vec![entry(
        "a.rs",
        vec![
            hunk(HunkOrigin::Unstaged, false),
            hunk(HunkOrigin::Staged, false),
        ],
    )];
    assert!(selected_paths(&entries).is_empty());
}

/// A staged hunk left selected is a choice the picker offered and the user kept,
/// so its path belongs in the fold.
#[test]
fn a_kept_staged_hunk_is_folded() {
    let entries = vec![entry("a.rs", vec![hunk(HunkOrigin::Staged, true)])];
    assert_eq!(selected_paths(&entries), vec!["a.rs".to_string()]);
}

/// The whole point of the fix: a staged change the picker has no hunk for — a
/// mode-only one — never reaches the listing, so nobody can pick it and the
/// fold cannot name it (Spec 007).
#[cfg(unix)]
#[test]
fn a_mode_only_staged_change_is_not_listed() {
    use std::os::unix::fs::PermissionsExt;

    let test_repo = repo_with_changes();
    let script = test_repo.workdir().join("m.sh");
    std::fs::write(&script, "#!/bin/sh\n").unwrap();
    test_repo.stage_files(&["m.sh"]);
    test_repo.commit_staged("Add a script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    test_repo.stage_files(&["m.sh"]);

    let workdir = test_repo.workdir().to_path_buf();
    let entries = test_repo
        .in_dir(|| super::collect_file_entries(&test_repo.repo, &workdir, None))
        .unwrap();

    let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    assert!(!paths.contains(&"m.sh"), "{paths:?}");
    assert!(
        paths.contains(&"a.rs"),
        "the other changes are still listed"
    );
}

#[test]
fn only_the_picked_files_are_folded_and_order_is_kept() {
    let entries = vec![
        entry("a.rs", vec![hunk(HunkOrigin::Unstaged, true)]),
        entry("b.rs", vec![hunk(HunkOrigin::Unstaged, false)]),
        entry(
            "c.rs",
            vec![
                hunk(HunkOrigin::Staged, false),
                hunk(HunkOrigin::Unstaged, true),
            ],
        ),
    ];
    assert_eq!(
        selected_paths(&entries),
        vec!["a.rs".to_string(), "c.rs".to_string()]
    );
}
