use std::path::Path;

use anyhow::Result;

use super::run_git_stdout;

/// Flags forced on every diff loom reads back, for display or otherwise.
///
/// `--no-color` rather than `-c color.ui=false`: an explicit `color.diff=always`
/// beats `color.ui`. An external diff driver prints whatever it likes, which is
/// not a patch.
const DISPLAY: &[&str] = &["--no-color", "--no-ext-diff"];

/// Flags forced on every diff loom parses or hands back to `git apply`.
///
/// A textconv filter rewrites a binary file's patch into text `git apply`
/// cannot apply, `diff.context=0` produces hunks it refuses without
/// `--unidiff-zero`, and `diff.submodule=diff` inlines a submodule's patch.
const REPLAY: &[&str] = &[
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--unified=3",
    "--submodule=short",
];

/// Run `git diff` with the flags that keep its output replayable, and return it.
fn diff_stdout(workdir: &Path, args: &[&str]) -> Result<String> {
    patch_stdout(workdir, "diff", REPLAY, args)
}

fn patch_stdout(workdir: &Path, command: &str, flags: &[&str], args: &[&str]) -> Result<String> {
    let mut full = vec![command];
    full.extend(flags);
    full.extend(args);
    run_git_stdout(workdir, &full)
}

/// Get the diff for a single commit (its changes relative to its parent).
///
/// Wraps `git diff <oid>^..<oid>`.
pub fn diff_commit(workdir: &Path, oid: &str) -> Result<String> {
    diff_stdout(workdir, &[&format!("{}^..{}", oid, oid)])
}

/// Get the diff for a single file within a commit (relative to its parent).
///
/// Wraps `git diff <oid>^..<oid> -- <path>`.
pub fn diff_commit_file(workdir: &Path, oid: &str, path: &str) -> Result<String> {
    diff_stdout(workdir, &[&format!("{}^..{}", oid, oid), "--", path])
}

/// Get the staged (cached) diff for specific files.
///
/// Wraps `git diff --cached -- <files>`. Returns an empty string if the
/// files have no staged changes.
pub fn diff_cached_files(workdir: &Path, files: &[&str]) -> Result<String> {
    let mut args = vec!["--cached", "--"];
    args.extend(files);
    diff_stdout(workdir, &args)
}

/// Get the diff of all tracked files against HEAD (name-only).
///
/// Wraps `git diff HEAD --name-only`. Returns one filename per line.
pub fn diff_head_name_only(workdir: &Path) -> Result<String> {
    diff_stdout(workdir, &["HEAD", "--name-only"])
}

/// Get the unified diff for a single file against HEAD.
///
/// Wraps `git diff HEAD -- <path>`.
pub fn diff_head_file(workdir: &Path, path: &str) -> Result<String> {
    diff_stdout(workdir, &["HEAD", "--", path])
}

/// Check whether a file is binary (has working-tree changes vs HEAD that git cannot diff as text).
///
/// Uses `git diff --numstat HEAD -- <path>`: binary files are reported with `-\t-` instead of
/// numeric insertion/deletion counts. This is locale-independent, unlike the "Binary files"
/// string in the standard diff output.
pub fn diff_head_file_is_binary(workdir: &Path, path: &str) -> Result<bool> {
    let out = diff_stdout(workdir, &["--numstat", "HEAD", "--", path])?;
    Ok(out.starts_with("-\t"))
}

/// Get the unified diff for a single file (unstaged changes only: index → worktree).
///
/// Wraps `git diff -- <path>`.
pub fn diff_file(workdir: &Path, path: &str) -> Result<String> {
    diff_stdout(workdir, &["--", path])
}

/// Get the staged diff for a single file (HEAD → index).
///
/// Wraps `git diff --cached -- <path>`.
pub fn diff_cached_file(workdir: &Path, path: &str) -> Result<String> {
    diff_stdout(workdir, &["--cached", "--", path])
}

/// Check whether a file's unstaged changes are binary.
///
/// Uses `git diff --numstat -- <path>`.
pub fn diff_file_is_binary(workdir: &Path, path: &str) -> Result<bool> {
    let out = diff_stdout(workdir, &["--numstat", "--", path])?;
    Ok(out.starts_with("-\t"))
}

/// Check whether a file's staged changes are binary.
///
/// Uses `git diff --cached --numstat -- <path>`.
pub fn diff_cached_file_is_binary(workdir: &Path, path: &str) -> Result<bool> {
    let out = diff_stdout(workdir, &["--cached", "--numstat", "--", path])?;
    Ok(out.starts_with("-\t"))
}

/// Check whether a file's changes within a commit are binary.
///
/// Uses `git diff --numstat <oid>^..<oid> -- <path>`.
pub fn diff_commit_file_is_binary(workdir: &Path, oid: &str, path: &str) -> Result<bool> {
    let out = diff_stdout(
        workdir,
        &["--numstat", &format!("{}^..{}", oid, oid), "--", path],
    )?;
    Ok(out.starts_with("-\t"))
}

/// List files changed in a commit with their status character (A/M/D/R/etc.).
///
/// Wraps `git diff --name-status <oid>^..<oid>`. Returns `(status_char, path)` pairs.
pub fn diff_commit_name_status(workdir: &Path, oid: &str) -> Result<Vec<(char, String)>> {
    let out = diff_stdout(workdir, &["--name-status", &format!("{}^..{}", oid, oid)])?;
    let mut result = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut fields = line.splitn(3, '\t');
        let status_field = fields.next().unwrap_or("");
        let path1 = fields.next().unwrap_or("").trim();
        let path2 = fields.next().map(str::trim);
        if status_field.is_empty() || path1.is_empty() {
            continue;
        }
        let status = status_field.chars().next().unwrap_or('M');
        // For rename/copy entries (R/C), git outputs "old-path\tnew-path"; use the destination.
        let path = match (status, path2) {
            ('R' | 'C', Some(dest)) => dest.to_string(),
            _ => path1.to_string(),
        };
        result.push((status, path));
    }
    Ok(result)
}

/// Get the full unified diff of all working-tree changes against HEAD.
///
/// Wraps `git diff HEAD`.
pub fn diff_head(workdir: &Path) -> Result<String> {
    diff_stdout(workdir, &["HEAD"])
}

/// Get the unified diff for specific files against HEAD.
///
/// Wraps `git diff HEAD -- <files>`.
pub fn diff_head_files(workdir: &Path, files: &[&str]) -> Result<String> {
    let mut args = vec!["HEAD", "--"];
    args.extend(files);
    diff_stdout(workdir, &args)
}

/// Get the working-tree diff against HEAD, for display.
///
/// Wraps `git diff HEAD`. Unlike [`diff_head`], keeps the user's textconv
/// filters: the output is read, never applied.
pub fn diff_head_display(workdir: &Path) -> Result<String> {
    patch_stdout(workdir, "diff", DISPLAY, &["HEAD"])
}

/// Get the working-tree diff of one file against HEAD, for display.
///
/// Wraps `git diff HEAD -- <path>`, keeping the user's textconv filters.
pub fn diff_head_file_display(workdir: &Path, path: &str) -> Result<String> {
    patch_stdout(workdir, "diff", DISPLAY, &["HEAD", "--", path])
}

/// Get the diff between two commits, for display.
///
/// Wraps `git diff <base> <tip>`.
pub fn diff_range(workdir: &Path, base: &str, tip: &str) -> Result<String> {
    patch_stdout(workdir, "diff", DISPLAY, &[base, tip])
}

/// Get a commit's summary and patch, for display.
///
/// Wraps `git show --stat --patch <oid>`.
pub fn show_commit_patch(workdir: &Path, oid: &str) -> Result<String> {
    patch_stdout(workdir, "show", DISPLAY, &["--stat", "--patch", oid])
}

/// Get the patch a commit applied to one file, without the commit header, for
/// display.
///
/// Wraps `git show --format= <oid> -- <path>`.
pub fn show_commit_file(workdir: &Path, oid: &str, path: &str) -> Result<String> {
    patch_stdout(workdir, "show", DISPLAY, &["--format=", oid, "--", path])
}

#[cfg(test)]
#[path = "git_diff_test.rs"]
mod tests;
