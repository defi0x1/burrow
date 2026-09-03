use crate::receipt::{Receipt, Verdict};
use std::time::Duration;

fn label_line(label: &str, first: &str) -> String {
    format!("  {:<10}{}", label, first)
}

fn cont_line(text: &str) -> String {
    format!("            {}", text)
}

pub fn print_header(id: &str, from_ref: &str, base_short: &str) {
    println!("burrow {}  ·  from {} ({})", id, from_ref, base_short);
    println!();
}

pub fn print_command(cmd: &[String]) {
    println!("  $ {}", shell_join(cmd));
}

fn shell_join(cmd: &[String]) -> String {
    cmd.iter()
        .map(|a| {
            if a.chars().any(|c| c.is_whitespace()) {
                format!("\"{}\"", a)
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn print_exit(exit_code: Option<i32>, timed_out: bool, duration: Duration) {
    println!();
    let code_str = match exit_code {
        Some(c) => c.to_string(),
        None => "signal".to_string(),
    };
    let suffix = if timed_out { " (timed out)" } else { "" };
    println!(
        "  exit {} · {}{}",
        code_str,
        human_duration(duration),
        suffix
    );
}

fn human_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else {
        format!("{}m{}s", secs / 60, secs % 60)
    }
}

/// Prints the CHANGED, GUARDS and verdict blocks. Shared by `run` and `show`.
pub fn print_result(receipt: &Receipt, id_for_hints: &str) {
    println!();
    print_changed(receipt);
    println!();
    print_guards(receipt);
    println!();
    print_verdict(receipt, id_for_hints);
}

fn print_changed(receipt: &Receipt) {
    let added: u64 = receipt.files.iter().map(|f| f.added).sum();
    let removed: u64 = receipt.files.iter().map(|f| f.removed).sum();
    println!(
        "{}",
        label_line(
            "CHANGED",
            &format!(
                "{} file{} · +{} \u{2212}{}",
                receipt.files_changed,
                if receipt.files_changed == 1 { "" } else { "s" },
                added,
                removed
            )
        )
    );
    let width = receipt
        .files
        .iter()
        .map(|f| f.path.chars().count())
        .max()
        .unwrap_or(0);
    for f in &receipt.files {
        println!(
            "    {:<width$}   +{} \u{2212}{}",
            f.path,
            f.added,
            f.removed,
            width = width
        );
    }
}

fn print_guards(receipt: &Receipt) {
    println!(
        "{}",
        label_line("GUARDS", &receipt.guard_summary.join(" · "))
    );
}

fn print_verdict(receipt: &Receipt, id: &str) {
    match receipt.verdict {
        Verdict::Ok => {
            println!(
                "{}",
                label_line("ok", &format!("burrow promote {} --branch <name>", id))
            );
            println!("{}", cont_line(&format!("burrow rm {}", id)));
        }
        Verdict::Blocked => {
            let mut lines = receipt.reasons.clone();
            if receipt.kept {
                lines.push(format!(
                    "the hole is kept so you can look: burrow show {}",
                    id
                ));
            } else {
                lines.push("the hole was discarded (--discard)".to_string());
            }
            let mut it = lines.iter();
            if let Some(first) = it.next() {
                println!("{}", label_line("blocked", first));
            } else {
                println!("{}", label_line("blocked", ""));
            }
            for rest in it {
                println!("{}", cont_line(rest));
            }
        }
    }
}

pub fn print_json(receipt: &Receipt) {
    match serde_json::to_string_pretty(receipt) {
        Ok(s) => println!("{}", s),
        Err(e) => eprintln!("failed to serialize receipt: {}", e),
    }
}
