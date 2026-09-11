use super::resolve_loom_exe;

/// An installed binary is itself: nothing above it is a `deps` directory. The
/// path need not exist — this branch never looks at the filesystem.
#[test]
fn an_installed_binary_resolves_to_itself() {
    let exe = std::path::Path::new("/usr/local/bin/git-loom");

    assert_eq!(resolve_loom_exe(exe).unwrap(), exe);
}

/// A test harness resolves to the real binary one level up.
#[test]
fn a_test_harness_resolves_to_the_built_binary() {
    let dir = tempfile::tempdir().unwrap();
    let deps = dir.path().join("deps");
    std::fs::create_dir(&deps).unwrap();
    let built = dir
        .path()
        .join(format!("git-loom{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&built, "").unwrap();

    let resolved = resolve_loom_exe(&deps.join("git_loom-0123456789abcdef")).unwrap();

    assert_eq!(resolved, built);
}

/// Without that binary the harness must not be offered in its place: git would
/// run it as the rebase sequence editor and get an argument parser instead.
#[test]
fn a_test_harness_without_the_built_binary_errors() {
    let dir = tempfile::tempdir().unwrap();
    let deps = dir.path().join("deps");
    std::fs::create_dir(&deps).unwrap();
    let harness = deps.join("git_loom-0123456789abcdef");

    let err = resolve_loom_exe(&harness).expect_err("the built binary is missing");

    let msg = err.to_string();
    assert!(msg.contains("git-loom"), "unexpected error: {msg}");
    assert!(msg.contains("cargo build"), "unexpected error: {msg}");
    assert!(
        !msg.contains("git_loom-0123456789abcdef"),
        "the harness path is not the one to build: {msg}"
    );
}

/// Paths with nothing above them to inspect: a root, which has no parent at
/// all, and a bare name, whose parent is empty rather than absent.
#[test]
fn a_path_with_no_directory_above_it_resolves_to_itself() {
    for exe in [std::path::Path::new("/"), std::path::Path::new("git-loom")] {
        assert_eq!(resolve_loom_exe(exe).unwrap(), exe, "for {}", exe.display());
    }
}
