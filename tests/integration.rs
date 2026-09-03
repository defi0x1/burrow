// Integration tests against real git repositories in temp directories.
// No mocking: every test shells out to the actual `burrow` binary and to `git`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_burrow"))
}

fn temp_dir(label: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "burrow-test-{}-{}-{}-{}",
        std::process::id(),
        label,
        nanos,
        n
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git command failed to run")
}

/// A fresh repo with one commit, so `HEAD` always resolves.
fn init_repo() -> PathBuf {
    let dir = temp_dir("repo");
    assert!(git(&dir, &["init", "-q"]).status.success());
    assert!(git(&dir, &["config", "user.email", "test@example.com"])
        .status
        .success());
    assert!(git(&dir, &["config", "user.name", "Test"]).status.success());
    std::fs::write(dir.join("README.md"), "hello\n").unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
    assert!(git(&dir, &["add", "-A"]).status.success());
    assert!(git(&dir, &["commit", "-q", "-m", "init"]).status.success());
    dir
}

fn burrow(repo: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(repo)
        .args(args)
        .output()
        .expect("failed to run burrow binary")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

fn extract_id(ls_output: &str) -> String {
    // Skip the header row, take the id from the first data row.
    ls_output
        .lines()
        .nth(1)
        .and_then(|l| l.split_whitespace().next())
        .expect("no burrow id found in ls output")
        .to_string()
}

#[test]
fn doctor_fails_outside_a_repo() {
    let dir = temp_dir("nonrepo");
    let out = burrow(&dir, &["doctor"]);
    assert!(!out.status.success(), "doctor should fail outside a repo");
    assert!(stdout(&out).contains("FAIL"));
}

#[test]
fn doctor_passes_inside_a_repo() {
    let repo = init_repo();
    let out = burrow(&repo, &["doctor"]);
    assert!(out.status.success(), "stdout: {}", stdout(&out));
    assert!(stdout(&out).contains("burrow is ready."));
}

#[test]
fn run_refuses_outside_a_repo() {
    let dir = temp_dir("norun");
    let out = burrow(&dir, &["run", "--", "sh", "-c", "echo hi"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("not a git repository"));
}

#[test]
fn run_ok_reports_changed_files_and_keeps_worktree_by_default() {
    let repo = init_repo();
    let out = burrow(&repo, &["run", "--", "sh", "-c", "echo new > new_file.txt"]);
    assert!(
        out.status.success(),
        "stdout: {} stderr: {}",
        stdout(&out),
        stderr(&out)
    );
    let s = stdout(&out);
    assert!(s.contains("CHANGED   1 file"));
    assert!(s.contains("new_file.txt"));
    assert!(s.contains("denylist clean"));
    assert!(s.contains("no secrets"));
    assert!(s.contains("ok        burrow promote"));

    // Default retention keeps the hole around for inspection.
    let ls = burrow(&repo, &["ls"]);
    assert!(stdout(&ls).contains("ok"));
    let burrows_dir = repo.join(".burrow").join("burrows");
    let entries: Vec<_> = std::fs::read_dir(&burrows_dir).unwrap().collect();
    assert_eq!(entries.len(), 1, "worktree should still be on disk");
}

#[test]
fn run_discard_removes_the_worktree() {
    let repo = init_repo();
    let out = burrow(
        &repo,
        &["run", "--discard", "--", "sh", "-c", "echo x > f.txt"],
    );
    assert!(out.status.success());
    let ls = burrow(&repo, &["ls"]);
    assert!(stdout(&ls).contains("no burrows."));
    let burrows_dir = repo.join(".burrow").join("burrows");
    let entries: Vec<_> = std::fs::read_dir(&burrows_dir)
        .map(|d| d.collect::<Vec<_>>())
        .unwrap_or_default();
    assert!(entries.is_empty());
}

#[test]
fn run_blocked_by_denylist() {
    let repo = init_repo();
    let out = burrow(
        &repo,
        &[
            "run",
            "--discard",
            "--",
            "sh",
            "-c",
            "mkdir -p .github/workflows && echo x > .github/workflows/ci.yml",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let s = stdout(&out);
    assert!(s.contains("denylist HIT"));
    assert!(s.contains("blocked"));
    assert!(s.contains(".github/workflows/ci.yml (denylist)"));
}

#[test]
fn run_blocked_by_max_files() {
    let repo = init_repo();
    let out = burrow(
        &repo,
        &[
            "run",
            "--discard",
            "--max-files",
            "1",
            "--",
            "sh",
            "-c",
            "echo a > a.txt && echo b > b.txt && echo c > c.txt",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let s = stdout(&out);
    assert!(s.contains("files 3/1"));
    assert!(s.contains("more file"));
}

#[test]
fn run_blocked_by_max_bytes() {
    let repo = init_repo();
    let out = burrow(
        &repo,
        &[
            "run",
            "--discard",
            "--max-bytes",
            "4",
            "--",
            "sh",
            "-c",
            "echo this-is-more-than-four-bytes > big.txt",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("more bytes than the limit"));
}

#[test]
fn run_blocked_by_secret_detection() {
    let repo = init_repo();
    let out = burrow(
        &repo,
        &[
            "run",
            "--discard",
            "--",
            "sh",
            "-c",
            "echo 'aws_key: AKIAABCDEFGHIJKLMNOP' > creds.txt",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("possible AWS access key"));
}

#[test]
fn run_custom_deny_glob_via_flag() {
    let repo = init_repo();
    let out = burrow(
        &repo,
        &[
            "run",
            "--discard",
            "--deny",
            "*.secret",
            "--",
            "sh",
            "-c",
            "echo x > oops.secret",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("oops.secret (denylist)"));
}

#[test]
fn env_vars_are_passed_to_the_command() {
    let repo = init_repo();
    let out = burrow(
        &repo,
        &[
            "run",
            "--discard",
            "--",
            "sh",
            "-c",
            "echo \"id=$BURROW_ID ref=$BURROW_REF dir=$BURROW_DIR\" > env_report.txt; cat env_report.txt >&2",
        ],
    );
    let err = stderr(&out);
    assert!(err.contains("ref=HEAD"), "stderr: {}", err);
    assert!(
        err.contains("dir=") && err.contains(".burrow/burrows/"),
        "stderr: {}",
        err
    );
}

#[test]
fn command_runs_with_cwd_set_to_the_worktree_not_the_repo() {
    let repo = init_repo();
    let out = burrow(&repo, &["run", "--discard", "--", "pwd"]);
    let s = stdout(&out);
    assert!(s.contains(".burrow/burrows/"), "stdout: {}", s);
}

#[test]
fn ls_shows_active_burrows() {
    let repo = init_repo();
    burrow(&repo, &["run", "--", "sh", "-c", "echo x > f.txt"]);
    let ls = burrow(&repo, &["ls"]);
    let s = stdout(&ls);
    assert!(s.contains("ID"));
    assert!(s.contains("VERDICT"));
    assert!(s.contains("ok"));
}

#[test]
fn show_prints_the_full_receipt() {
    let repo = init_repo();
    let run_out = burrow(&repo, &["run", "--", "sh", "-c", "echo x > f.txt"]);
    assert!(run_out.status.success());
    let ls = burrow(&repo, &["ls"]);
    let id = extract_id(&stdout(&ls));

    let show = burrow(&repo, &["show", &id]);
    assert!(show.status.success());
    let s = stdout(&show);
    assert!(s.contains(&id));
    assert!(s.contains("CHANGED"));
    assert!(s.contains("GUARDS"));
}

#[test]
fn show_unknown_id_errors() {
    let repo = init_repo();
    let out = burrow(&repo, &["show", "no-such-burrow"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no such burrow"));
}

#[test]
fn promote_creates_a_branch_from_the_burrows_changes() {
    let repo = init_repo();
    burrow(&repo, &["run", "--", "sh", "-c", "echo new > promoted.txt"]);
    let ls = burrow(&repo, &["ls"]);
    let id = extract_id(&stdout(&ls));

    let promote = burrow(&repo, &["promote", &id, "--branch", "feature/x"]);
    assert!(
        promote.status.success(),
        "stdout: {} stderr: {}",
        stdout(&promote),
        stderr(&promote)
    );

    let branches = git(&repo, &["branch", "--list", "feature/x"]);
    assert!(stdout(&branches).contains("feature/x"));

    // The branch's commit must actually contain the file, and the main repo
    // working tree must be untouched (still on the original branch/state).
    let show = git(&repo, &["show", "feature/x:promoted.txt"]);
    assert!(show.status.success());
    assert_eq!(stdout(&show).trim(), "new");
    assert!(!repo.join("promoted.txt").exists());
}

#[test]
fn promote_refuses_a_blocked_burrow_without_force() {
    let repo = init_repo();
    burrow(
        &repo,
        &[
            "run",
            "--",
            "sh",
            "-c",
            "mkdir -p .github/workflows && echo x > .github/workflows/ci.yml",
        ],
    );
    let ls = burrow(&repo, &["ls"]);
    let id = extract_id(&stdout(&ls));

    let promote = burrow(&repo, &["promote", &id]);
    assert!(!promote.status.success());
    assert!(stderr(&promote).contains("blocked"));

    let promote_forced = burrow(&repo, &["promote", &id, "--branch", "forced", "--force"]);
    assert!(promote_forced.status.success());
}

#[test]
fn promote_with_patch_writes_a_patch_file() {
    let repo = init_repo();
    burrow(&repo, &["run", "--", "sh", "-c", "echo x > patched.txt"]);
    let ls = burrow(&repo, &["ls"]);
    let id = extract_id(&stdout(&ls));

    let patch_path = temp_dir("patch").join("out.patch");
    let promote = burrow(
        &repo,
        &["promote", &id, "--patch", patch_path.to_str().unwrap()],
    );
    assert!(promote.status.success(), "stderr: {}", stderr(&promote));
    let patch = std::fs::read_to_string(&patch_path).unwrap();
    assert!(patch.contains("patched.txt"));
    assert!(patch.contains("diff --git"));
}

#[test]
fn rm_removes_a_burrow() {
    let repo = init_repo();
    burrow(&repo, &["run", "--", "sh", "-c", "echo x > f.txt"]);
    let ls = burrow(&repo, &["ls"]);
    let id = extract_id(&stdout(&ls));

    let rm = burrow(&repo, &["rm", &id]);
    assert!(rm.status.success());

    let ls_after = burrow(&repo, &["ls"]);
    assert!(stdout(&ls_after).contains("no burrows."));
}

#[test]
fn rm_all_skips_nothing_when_nothing_is_running() {
    let repo = init_repo();
    burrow(&repo, &["run", "--", "sh", "-c", "echo x > a.txt"]);
    burrow(&repo, &["run", "--", "sh", "-c", "echo y > b.txt"]);

    let rm = burrow(&repo, &["rm", "--all"]);
    assert!(rm.status.success());
    assert!(stdout(&rm).contains("removed 2"));

    let ls = burrow(&repo, &["ls"]);
    assert!(stdout(&ls).contains("no burrows."));
}

#[test]
fn burrow_json_supplies_defaults_that_flags_override() {
    let repo = init_repo();
    std::fs::write(repo.join("burrow.json"), r#"{"max_files": 1}"#).unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "add config"]);

    // Config alone blocks at 2 files.
    let out = burrow(
        &repo,
        &[
            "run",
            "--discard",
            "--",
            "sh",
            "-c",
            "echo a > a.txt && echo b > b.txt",
        ],
    );
    assert_eq!(out.status.code(), Some(1));

    // A flag overrides the config default.
    let out2 = burrow(
        &repo,
        &[
            "run",
            "--discard",
            "--max-files",
            "5",
            "--",
            "sh",
            "-c",
            "echo a > a.txt && echo b > b.txt",
        ],
    );
    assert_eq!(out2.status.code(), Some(0));
}

#[test]
fn run_from_a_non_head_ref_works() {
    let repo = init_repo();
    git(&repo, &["branch", "topic"]);
    std::fs::write(repo.join("README.md"), "changed on main\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "main-only change"]);

    let out = burrow(
        &repo,
        &[
            "run",
            "--discard",
            "--from",
            "topic",
            "--",
            "sh",
            "-c",
            "echo x > f.txt",
        ],
    );
    assert!(out.status.success());
    assert!(stdout(&out).contains("from topic"));
}
