use std::path::{Path, PathBuf};
use std::process::Command;

pub struct FileDiff {
    pub path: String,
    pub added_lines: u64,
    pub removed_lines: u64,
    pub added_bytes: u64,
    pub binary: bool,
    /// Content of each added line, for secret scanning. Empty for binary files.
    pub added_line_samples: Vec<String>,
}

fn run_git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git {}: {}", args.join(" "), e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git {} failed: {}", args.join(" "), stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Find the top-level directory of the git repo containing `start`.
pub fn find_repo_root(start: &Path) -> Result<PathBuf, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(start)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("failed to run git: {}", e))?;
    if !out.status.success() {
        return Err(
            "not a git repository (or any parent up to mount point). burrow needs a repo to work in."
                .to_string(),
        );
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(PathBuf::from(s))
}

/// Resolve a ref to a full commit hash.
pub fn resolve_ref(repo_root: &Path, refname: &str) -> Result<String, String> {
    run_git(repo_root, &["rev-parse", "--verify", refname]).map(|s| s.trim().to_string())
}

pub fn short_hash(repo_root: &Path, commit: &str) -> Result<String, String> {
    run_git(repo_root, &["rev-parse", "--short", commit]).map(|s| s.trim().to_string())
}

pub fn worktree_add(repo_root: &Path, worktree_path: &Path, refname: &str) -> Result<(), String> {
    run_git(
        repo_root,
        &[
            "worktree",
            "add",
            "--detach",
            worktree_path.to_str().ok_or("invalid path")?,
            refname,
        ],
    )
    .map(|_| ())
}

pub fn worktree_remove(repo_root: &Path, worktree_path: &Path) -> Result<(), String> {
    let path_str = worktree_path.to_str().ok_or("invalid path")?;
    // The directory may already be gone (e.g. the command deleted it). Prune first
    // so `worktree remove` does not choke on a missing checkout.
    let _ = run_git(repo_root, &["worktree", "prune"]);
    if !worktree_path.exists() {
        return Ok(());
    }
    run_git(repo_root, &["worktree", "remove", "--force", path_str]).map(|_| ())
}

pub fn worktree_support_available(repo_root: &Path) -> bool {
    run_git(repo_root, &["worktree", "list"]).is_ok()
}

/// True if the worktree has any uncommitted change (tracked or untracked).
pub fn has_changes(worktree_path: &Path) -> Result<bool, String> {
    let out = run_git(
        worktree_path,
        &["status", "--porcelain", "--untracked-files=all"],
    )?;
    Ok(!out.trim().is_empty())
}

/// Diff every changed file (tracked and untracked) against HEAD.
pub fn diff_details(worktree_path: &Path) -> Result<Vec<FileDiff>, String> {
    let mut files = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let diff_text = run_git(worktree_path, &["diff", "HEAD", "--no-color"])?;
    parse_unified_diff(&diff_text, &mut files, &mut seen);

    let status = run_git(
        worktree_path,
        &["status", "--porcelain", "--untracked-files=all"],
    )?;
    for line in status.lines() {
        if let Some(path) = line.strip_prefix("?? ") {
            let path = path.trim();
            if seen.contains(path) {
                continue;
            }
            let full = worktree_path.join(path);
            let meta = std::fs::metadata(&full);
            if let Ok(meta) = meta {
                if meta.is_file() {
                    match std::fs::read(&full) {
                        Ok(bytes) => {
                            if is_binary(&bytes) {
                                files.push(FileDiff {
                                    path: path.to_string(),
                                    added_lines: 0,
                                    removed_lines: 0,
                                    added_bytes: bytes.len() as u64,
                                    binary: true,
                                    added_line_samples: Vec::new(),
                                });
                            } else {
                                let text = String::from_utf8_lossy(&bytes);
                                let lines: Vec<String> =
                                    text.lines().map(|l| l.to_string()).collect();
                                files.push(FileDiff {
                                    path: path.to_string(),
                                    added_lines: lines.len() as u64,
                                    removed_lines: 0,
                                    added_bytes: bytes.len() as u64,
                                    binary: false,
                                    added_line_samples: lines,
                                });
                            }
                        }
                        Err(_) => continue,
                    }
                    seen.insert(path.to_string());
                }
            }
        }
    }

    Ok(files)
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|&b| b == 0)
}

fn strip_ab_prefix(p: &str) -> String {
    let p = p.trim();
    if let Some(rest) = p.strip_prefix("a/") {
        return rest.to_string();
    }
    if let Some(rest) = p.strip_prefix("b/") {
        return rest.to_string();
    }
    p.to_string()
}

fn parse_unified_diff(
    text: &str,
    files: &mut Vec<FileDiff>,
    seen: &mut std::collections::HashSet<String>,
) {
    let mut cur: Option<FileDiff> = None;
    let flush = |cur: &mut Option<FileDiff>,
                 files: &mut Vec<FileDiff>,
                 seen: &mut std::collections::HashSet<String>| {
        if let Some(f) = cur.take() {
            if !f.path.is_empty() {
                seen.insert(f.path.clone());
                files.push(f);
            }
        }
    };

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            flush(&mut cur, files, seen);
            let mut f = FileDiff {
                path: String::new(),
                added_lines: 0,
                removed_lines: 0,
                added_bytes: 0,
                binary: false,
                added_line_samples: Vec::new(),
            };
            // "a/path b/path" -- take the b/ side if we can find it, else best effort.
            if let Some(idx) = rest.find(" b/") {
                f.path = rest[idx + 3..].to_string();
            }
            cur = Some(f);
        } else if let Some(rest) = line.strip_prefix("Binary files ") {
            if let Some(f) = cur.as_mut() {
                f.binary = true;
                if f.path.is_empty() {
                    if let Some(idx) = rest.find(" and ") {
                        f.path = strip_ab_prefix(&rest[..idx]);
                    }
                }
            }
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            if rest.trim() != "/dev/null" {
                if let Some(f) = cur.as_mut() {
                    f.path = strip_ab_prefix(rest);
                }
            }
        } else if let Some(rest) = line.strip_prefix("--- ") {
            if let Some(f) = cur.as_mut() {
                if f.path.is_empty() && rest.trim() != "/dev/null" {
                    f.path = strip_ab_prefix(rest);
                }
            }
        } else if let Some(content) = line.strip_prefix('+') {
            if let Some(f) = cur.as_mut() {
                f.added_lines += 1;
                f.added_bytes += content.len() as u64;
                f.added_line_samples.push(content.to_string());
            }
        } else if line.starts_with('-') {
            if let Some(f) = cur.as_mut() {
                f.removed_lines += 1;
            }
        }
    }
    flush(&mut cur, files, seen);
}

/// Commit everything currently in the worktree.
pub fn commit_all(worktree_path: &Path, message: &str) -> Result<String, String> {
    run_git(worktree_path, &["add", "-A"])?;
    run_git(worktree_path, &["commit", "-m", message, "--quiet"])?;
    run_git(worktree_path, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string())
}

pub fn create_branch(
    repo_root: &Path,
    branch: &str,
    commit: &str,
    force: bool,
) -> Result<(), String> {
    let mut args = vec!["branch"];
    if force {
        args.push("-f");
    }
    args.push(branch);
    args.push(commit);
    run_git(repo_root, &args).map(|_| ())
}

pub fn write_patch(worktree_path: &Path, base: &str, out_path: &Path) -> Result<(), String> {
    let patch = run_git(
        worktree_path,
        &["format-patch", "--stdout", &format!("{}..HEAD", base)],
    )?;
    std::fs::write(out_path, patch).map_err(|e| format!("failed to write patch file: {}", e))
}
