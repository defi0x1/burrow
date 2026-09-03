mod cli;
mod guards;
mod receipt;
mod repo;
mod report;
mod run;
mod worktree;

use clap::Parser;
use cli::{Cli, Commands, PromoteArgs, RmArgs, RunArgs, ShowArgs};
use receipt::{BurrowInfo, FileChange, Limits, Receipt, Verdict};
use serde::Deserialize;
use std::time::Duration;
use worktree::Layout;

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Run(args) => cmd_run(args),
        Commands::Ls => cmd_ls(),
        Commands::Show(args) => cmd_show(args),
        Commands::Promote(args) => cmd_promote(args),
        Commands::Rm(args) => cmd_rm(args),
        Commands::Doctor => cmd_doctor(),
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("burrow: {}", e);
            std::process::exit(2);
        }
    }
}

#[derive(Deserialize, Default)]
struct BurrowJson {
    max_files: Option<usize>,
    max_bytes: Option<u64>,
    deny: Option<Vec<String>>,
}

fn load_burrow_json(repo_root: &std::path::Path) -> Result<BurrowJson, String> {
    let path = repo_root.join("burrow.json");
    if !path.exists() {
        return Ok(BurrowJson::default());
    }
    let s =
        std::fs::read_to_string(&path).map_err(|e| format!("failed to read burrow.json: {}", e))?;
    serde_json::from_str(&s).map_err(|e| format!("failed to parse burrow.json: {}", e))
}

fn repo_layout() -> Result<Layout, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = repo::find_repo_root(&cwd)?;
    let layout = Layout::new(root);
    layout.ensure_dirs()?;
    Ok(layout)
}

fn cmd_run(args: RunArgs) -> Result<i32, String> {
    let layout = repo_layout()?;
    let cfg = load_burrow_json(&layout.repo_root)?;

    let max_files = args
        .max_files
        .or(cfg.max_files)
        .unwrap_or(guards::DEFAULT_MAX_FILES);
    let max_bytes = args
        .max_bytes
        .or(cfg.max_bytes)
        .unwrap_or(guards::DEFAULT_MAX_BYTES);
    let mut denylist = cfg.deny.unwrap_or_else(guards::default_denylist);
    denylist.extend(args.deny.clone());

    let from_ref = args.from.clone().unwrap_or_else(|| "HEAD".to_string());
    let base_commit = repo::resolve_ref(&layout.repo_root, &from_ref)?;
    let base_short = repo::short_hash(&layout.repo_root, &base_commit)?;

    let id = worktree::new_id();
    let worktree_path = layout.worktree_path(&id);
    let meta_dir = layout.meta_dir(&id);
    std::fs::create_dir_all(&meta_dir).map_err(|e| e.to_string())?;

    repo::worktree_add(&layout.repo_root, &worktree_path, &base_commit)?;
    let lock = worktree::Lock::acquire(&meta_dir)?;

    let info = BurrowInfo {
        id: id.clone(),
        from_ref: from_ref.clone(),
        base_commit: base_commit.clone(),
        base_commit_short: base_short.clone(),
        command: args.command.clone(),
        started_at: worktree::now_rfc3339(),
        started_at_unix: worktree::now_unix(),
        pid: std::process::id(),
    };
    receipt::save_info(&meta_dir, &info)?;

    report::print_header(&id, &from_ref, &base_short);
    report::print_command(&args.command);

    let env: Vec<(&str, String)> = vec![
        ("BURROW_ID", id.clone()),
        ("BURROW_DIR", worktree_path.to_string_lossy().to_string()),
        ("BURROW_REF", from_ref.clone()),
    ];
    let timeout = args.timeout.map(Duration::from_secs);

    let outcome = run::run_command(&worktree_path, &args.command, &env, timeout);
    let outcome = match outcome {
        Ok(o) => o,
        Err(e) => {
            lock.release();
            return Err(e);
        }
    };

    report::print_exit(outcome.exit_code, outcome.timed_out, outcome.duration);

    let files = repo::diff_details(&worktree_path)?;
    let file_changes: Vec<FileChange> = files
        .iter()
        .map(|f| FileChange {
            path: f.path.clone(),
            added: f.added_lines,
            removed: f.removed_lines,
        })
        .collect();
    let bytes_added: u64 = files.iter().map(|f| f.added_bytes).sum();

    let guard_config = guards::GuardConfig {
        max_files,
        max_bytes,
        denylist: denylist.clone(),
    };
    let guard_report = guards::evaluate(&files, &guard_config)?;
    let verdict = if guard_report.blocked() {
        Verdict::Blocked
    } else {
        Verdict::Ok
    };

    let keep = !args.discard;

    let receipt = Receipt {
        id: id.clone(),
        from_ref: from_ref.clone(),
        base_commit: base_commit.clone(),
        base_commit_short: base_short.clone(),
        command: args.command.clone(),
        started_at: info.started_at.clone(),
        finished_at: worktree::now_rfc3339(),
        duration_secs: outcome.duration.as_secs(),
        exit_code: outcome.exit_code,
        timed_out: outcome.timed_out,
        files: file_changes,
        files_changed: files.len(),
        bytes_added,
        limits: Limits {
            max_files,
            max_bytes,
        },
        denylist,
        verdict: verdict.clone(),
        reasons: guard_report.reasons(),
        offending_paths: guard_report.offending_paths(),
        guard_summary: guard_report.summary_parts(),
        kept: keep,
    };
    receipt::save_receipt(&meta_dir, &receipt)?;
    lock.release();

    if !keep {
        repo::worktree_remove(&layout.repo_root, &worktree_path)?;
        let _ = std::fs::remove_dir_all(&meta_dir);
    }

    if args.json {
        report::print_json(&receipt);
    } else {
        report::print_result(&receipt, &id);
    }

    Ok(if matches!(verdict, Verdict::Blocked) {
        1
    } else {
        0
    })
}

fn cmd_ls() -> Result<i32, String> {
    let layout = repo_layout()?;
    let ids = layout.list_ids();

    if ids.is_empty() {
        println!("no burrows.");
        return Ok(0);
    }

    let mut rows: Vec<(String, String, String, String, String)> = Vec::new();
    for id in &ids {
        let meta_dir = layout.meta_dir(id);
        let info = receipt::load_info(&meta_dir);
        let (from_ref, started_unix) = match &info {
            Ok(i) => (i.from_ref.clone(), i.started_at_unix),
            Err(_) => ("?".to_string(), 0),
        };
        let age = human_age(worktree::now_unix().saturating_sub(started_unix));

        let (verdict, files) = if receipt::has_receipt(&meta_dir) {
            match receipt::load_receipt(&meta_dir) {
                Ok(r) => (
                    match r.verdict {
                        Verdict::Ok => "ok".to_string(),
                        Verdict::Blocked => "blocked".to_string(),
                    },
                    r.files_changed.to_string(),
                ),
                Err(_) => ("unknown".to_string(), "-".to_string()),
            }
        } else if worktree::is_locked(&meta_dir) {
            if worktree::is_stale(&meta_dir) {
                ("stale".to_string(), "-".to_string())
            } else {
                ("running".to_string(), "-".to_string())
            }
        } else {
            ("stale".to_string(), "-".to_string())
        };

        rows.push((id.clone(), from_ref, age, verdict, files));
    }

    let id_w = rows.iter().map(|r| r.0.len()).max().unwrap_or(2).max(2);
    let ref_w = rows.iter().map(|r| r.1.len()).max().unwrap_or(3).max(3);
    let age_w = rows.iter().map(|r| r.2.len()).max().unwrap_or(3).max(3);
    let verdict_w = rows.iter().map(|r| r.3.len()).max().unwrap_or(7).max(7);

    println!(
        "{:id_w$}  {:ref_w$}  {:age_w$}  {:verdict_w$}  FILES",
        "ID",
        "REF",
        "AGE",
        "VERDICT",
        id_w = id_w,
        ref_w = ref_w,
        age_w = age_w,
        verdict_w = verdict_w
    );
    for (id, from_ref, age, verdict, files) in &rows {
        println!(
            "{:id_w$}  {:ref_w$}  {:age_w$}  {:verdict_w$}  {}",
            id,
            from_ref,
            age,
            verdict,
            files,
            id_w = id_w,
            ref_w = ref_w,
            age_w = age_w,
            verdict_w = verdict_w
        );
    }

    Ok(0)
}

fn human_age(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

fn cmd_show(args: ShowArgs) -> Result<i32, String> {
    let layout = repo_layout()?;
    let meta_dir = layout.meta_dir(&args.id);
    if !meta_dir.exists() {
        return Err(format!("no such burrow: {}", args.id));
    }
    let info = receipt::load_info(&meta_dir)?;

    if !receipt::has_receipt(&meta_dir) {
        report::print_header(&info.id, &info.from_ref, &info.base_commit_short);
        report::print_command(&info.command);
        if worktree::is_locked(&meta_dir) && !worktree::is_stale(&meta_dir) {
            println!();
            println!("  still running (pid {})", info.pid);
        } else {
            println!();
            println!("  the process died before finishing. nothing to show yet.");
            println!("  burrow rm {} --stale removes holes like this.", args.id);
        }
        return Ok(0);
    }

    let r = receipt::load_receipt(&meta_dir)?;
    report::print_header(&r.id, &r.from_ref, &r.base_commit_short);
    report::print_command(&r.command);
    report::print_exit(
        r.exit_code,
        r.timed_out,
        Duration::from_secs(r.duration_secs),
    );
    report::print_result(&r, &r.id);

    Ok(if matches!(r.verdict, Verdict::Blocked) {
        1
    } else {
        0
    })
}

fn cmd_promote(args: PromoteArgs) -> Result<i32, String> {
    let layout = repo_layout()?;
    let meta_dir = layout.meta_dir(&args.id);
    if !meta_dir.exists() {
        return Err(format!("no such burrow: {}", args.id));
    }
    if !receipt::has_receipt(&meta_dir) {
        return Err(format!(
            "burrow {} hasn't finished yet, nothing to promote",
            args.id
        ));
    }
    let r = receipt::load_receipt(&meta_dir)?;

    if matches!(r.verdict, Verdict::Blocked) && !args.force {
        let reasons = if r.reasons.is_empty() {
            "a guard blocked this run".to_string()
        } else {
            r.reasons.join("; ")
        };
        return Err(format!(
            "burrow {} is blocked: {}. use --force to promote anyway",
            args.id, reasons
        ));
    }

    let worktree_path = layout.worktree_path(&args.id);
    if !worktree_path.exists() {
        return Err(format!(
            "burrow {} has no worktree left (it may have been discarded)",
            args.id
        ));
    }
    if !repo::has_changes(&worktree_path)? {
        return Err(format!(
            "burrow {} made no changes, nothing to promote",
            args.id
        ));
    }

    let message = format!("burrow: promote {}", args.id);
    let commit = repo::commit_all(&worktree_path, &message)?;

    if let Some(patch_path) = &args.patch {
        repo::write_patch(&worktree_path, &r.base_commit, patch_path)?;
        println!("wrote patch: {}", patch_path.display());
    } else {
        let branch = args
            .branch
            .clone()
            .unwrap_or_else(|| format!("burrow/{}", args.id));
        repo::create_branch(&layout.repo_root, &branch, &commit, args.force)?;
        println!(
            "created branch {} at {}",
            branch,
            &commit[..12.min(commit.len())]
        );
        println!("  git checkout {}", branch);
    }

    Ok(0)
}

fn cmd_rm(args: RmArgs) -> Result<i32, String> {
    let layout = repo_layout()?;

    let mode_count = args.id.is_some() as u8 + args.all as u8 + args.stale as u8;
    if mode_count != 1 {
        return Err("specify exactly one of: a burrow id, --all, or --stale".to_string());
    }

    if let Some(id) = &args.id {
        return rm_one_checked(&layout, id).map(|_| 0);
    }

    let ids = layout.list_ids();
    let mut removed = 0;
    let mut skipped = 0;
    for id in ids {
        let meta_dir = layout.meta_dir(&id);
        let active = worktree::is_locked(&meta_dir) && !worktree::is_stale(&meta_dir);

        if args.stale {
            let is_stale_lock = worktree::is_locked(&meta_dir) && worktree::is_stale(&meta_dir);
            if !is_stale_lock {
                continue;
            }
        } else if active {
            skipped += 1;
            continue;
        }

        rm_paths(&layout, &id)?;
        removed += 1;
    }

    if args.all {
        println!("removed {} burrow(s), skipped {} active", removed, skipped);
    } else {
        println!("removed {} stale burrow(s)", removed);
    }

    Ok(0)
}

fn rm_one_checked(layout: &Layout, id: &str) -> Result<(), String> {
    let meta_dir = layout.meta_dir(id);
    if !meta_dir.exists() {
        return Err(format!("no such burrow: {}", id));
    }
    if worktree::is_locked(&meta_dir) && !worktree::is_stale(&meta_dir) {
        return Err(format!(
            "burrow {} looks active, refusing to remove it while it's running",
            id
        ));
    }
    rm_paths(layout, id)?;
    println!("removed {}", id);
    Ok(())
}

fn rm_paths(layout: &Layout, id: &str) -> Result<(), String> {
    let worktree_path = layout.worktree_path(id);
    let meta_dir = layout.meta_dir(id);
    repo::worktree_remove(&layout.repo_root, &worktree_path)?;
    let _ = std::fs::remove_dir_all(&meta_dir);
    Ok(())
}

fn cmd_doctor() -> Result<i32, String> {
    let mut ok = true;

    let git_ok = repo::git_available();
    print_check(git_ok, "git is installed");
    ok &= git_ok;

    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = repo::find_repo_root(&cwd);
    let repo_ok = root.is_ok();
    print_check(repo_ok, "inside a git repository");
    ok &= repo_ok;

    if let Ok(root) = &root {
        let state_dir = root.join(".burrow");
        let writable = std::fs::create_dir_all(&state_dir).is_ok()
            && std::fs::write(state_dir.join(".doctor-check"), b"ok").is_ok();
        if writable {
            let _ = std::fs::remove_file(state_dir.join(".doctor-check"));
        }
        print_check(writable, "state directory (.burrow) is writable");
        ok &= writable;

        let worktree_ok = repo::worktree_support_available(root);
        print_check(worktree_ok, "git worktree is available");
        ok &= worktree_ok;
    } else {
        print_check(false, "state directory (.burrow) is writable");
        print_check(false, "git worktree is available");
        ok = false;
    }

    println!();
    if ok {
        println!("burrow is ready.");
        Ok(0)
    } else {
        println!("burrow found problems above.");
        Ok(1)
    }
}

fn print_check(ok: bool, label: &str) {
    let mark = if ok { "ok " } else { "FAIL" };
    println!("  [{}]  {}", mark, label);
}
