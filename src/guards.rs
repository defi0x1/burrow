use crate::repo::FileDiff;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::collections::BTreeSet;

pub const DEFAULT_MAX_FILES: usize = 10;
pub const DEFAULT_MAX_BYTES: u64 = 64 * 1024;

pub fn default_denylist() -> Vec<String> {
    vec![
        ".env*".to_string(),
        "**/*.pem".to_string(),
        "**/id_rsa*".to_string(),
        "**/secrets/**".to_string(),
        ".github/workflows/**".to_string(),
        "**/package-lock.json".to_string(),
        "**/yarn.lock".to_string(),
        "**/pnpm-lock.yaml".to_string(),
        "**/Cargo.lock".to_string(),
        "**/Gemfile.lock".to_string(),
        "**/poetry.lock".to_string(),
        "**/composer.lock".to_string(),
        "**/migrations/**".to_string(),
    ]
}

pub struct GuardOutcome {
    pub name: &'static str,
    pub blocked: bool,
    /// Fragment for the GUARDS summary line, e.g. "files 3/10" or "denylist clean".
    pub summary: String,
    /// Human reason lines shown when blocked.
    pub reasons: Vec<String>,
    pub paths: Vec<String>,
}

pub struct GuardReport {
    pub outcomes: Vec<GuardOutcome>,
}

impl GuardReport {
    pub fn blocked(&self) -> bool {
        self.outcomes.iter().any(|o| o.blocked)
    }

    pub fn reasons(&self) -> Vec<String> {
        self.outcomes
            .iter()
            .filter(|o| o.blocked)
            .flat_map(|o| o.reasons.clone())
            .collect()
    }

    pub fn offending_paths(&self) -> Vec<String> {
        let mut set = BTreeSet::new();
        for o in &self.outcomes {
            for p in &o.paths {
                set.insert(p.clone());
            }
        }
        set.into_iter().collect()
    }

    /// The GUARDS summary line, in the project's fixed display order
    /// (files, bytes, denylist, secrets). When blocked, only the guards
    /// that actually blocked are shown.
    pub fn summary_parts(&self) -> Vec<String> {
        let order = ["max_files", "max_bytes", "denylist", "secrets"];
        let blocked = self.blocked();
        order
            .iter()
            .filter_map(|name| {
                let o = self.outcomes.iter().find(|o| o.name == *name)?;
                if !blocked || o.blocked {
                    Some(o.summary.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}

fn build_globset(patterns: &[String]) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        let glob = Glob::new(p).map_err(|e| format!("invalid denylist glob {:?}: {}", p, e))?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|e| format!("failed to build denylist matcher: {}", e))
}

fn eval_denylist(files: &[FileDiff], patterns: &[String]) -> Result<GuardOutcome, String> {
    let set = build_globset(patterns)?;
    let hits: Vec<String> = files
        .iter()
        .filter(|f| set.is_match(&f.path))
        .map(|f| f.path.clone())
        .collect();
    let blocked = !hits.is_empty();
    Ok(GuardOutcome {
        name: "denylist",
        blocked,
        summary: if blocked {
            "denylist HIT".to_string()
        } else {
            "denylist clean".to_string()
        },
        reasons: hits
            .iter()
            .map(|p| format!("changed {} (denylist)", p))
            .collect(),
        paths: hits,
    })
}

fn eval_max_files(files: &[FileDiff], max_files: usize) -> GuardOutcome {
    let count = files.len();
    let blocked = count > max_files;
    GuardOutcome {
        name: "max_files",
        blocked,
        summary: format!("files {}/{}", count, max_files),
        reasons: if blocked {
            vec![format!(
                "{} more file{} than the limit allows",
                count - max_files,
                if count - max_files == 1 { "" } else { "s" }
            )]
        } else {
            Vec::new()
        },
        paths: Vec::new(),
    }
}

fn eval_max_bytes(files: &[FileDiff], max_bytes: u64) -> GuardOutcome {
    let total: u64 = files.iter().map(|f| f.added_bytes).sum();
    let blocked = total > max_bytes;
    GuardOutcome {
        name: "max_bytes",
        blocked,
        summary: format!("bytes {}/{}", human_bytes(total), human_bytes(max_bytes)),
        reasons: if blocked {
            vec![format!(
                "{} more bytes than the limit allows",
                human_bytes(total - max_bytes)
            )]
        } else {
            Vec::new()
        },
        paths: Vec::new(),
    }
}

const SECRET_KEYWORDS: &[&str] = &["token", "secret", "key", "password", "api_key"];

fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut counts = std::collections::HashMap::new();
    for c in s.chars() {
        *counts.entry(c).or_insert(0u32) += 1;
    }
    let len = s.chars().count() as f64;
    counts
        .values()
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum()
}

/// Pull candidate "value" tokens out of a line: runs of characters that look
/// like an assigned value (after `=`, `:`, or inside quotes).
fn candidate_values(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in line.split(['=', ':']).skip(1) {
        let trimmed = part
            .trim()
            .trim_matches(|c: char| c == '"' || c == '\'' || c == ',' || c == ';');
        let token: String = trimmed
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '#')
            .collect();
        if token.len() >= 16 {
            out.push(token);
        }
    }
    out
}

fn looks_like_private_key(line: &str) -> bool {
    line.contains("-----BEGIN") && line.contains("PRIVATE KEY")
}

fn looks_like_aws_key(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    for prefix in ["AKIA", "ASIA"] {
        let mut start = 0;
        while let Some(pos) = line[start..].find(prefix) {
            let idx = start + pos;
            let end = idx + 20;
            if end <= bytes.len() {
                let candidate = &line[idx..end];
                if candidate
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                {
                    return Some(candidate.to_string());
                }
            }
            start = idx + 1;
        }
    }
    None
}

fn eval_secrets(files: &[FileDiff]) -> GuardOutcome {
    let mut hits: Vec<String> = Vec::new();
    let mut reasons: Vec<String> = Vec::new();
    let mut paths: Vec<String> = Vec::new();

    for f in files {
        if f.binary {
            continue;
        }
        for line in &f.added_line_samples {
            if looks_like_private_key(line) {
                let msg = format!("possible private key in {}", f.path);
                if !hits.contains(&msg) {
                    hits.push(msg.clone());
                    reasons.push(msg);
                    paths.push(f.path.clone());
                }
                continue;
            }
            if let Some(key) = looks_like_aws_key(line) {
                let msg = format!("possible AWS access key in {} ({})", f.path, key);
                if !hits.contains(&msg) {
                    hits.push(msg.clone());
                    reasons.push(msg);
                    paths.push(f.path.clone());
                }
                continue;
            }
            let lower = line.to_lowercase();
            if SECRET_KEYWORDS.iter().any(|k| lower.contains(k)) {
                for candidate in candidate_values(line) {
                    if shannon_entropy(&candidate) >= 3.5 {
                        let msg = format!("possible secret in {}", f.path);
                        if !hits.contains(&msg) {
                            hits.push(msg.clone());
                            reasons.push(msg);
                            paths.push(f.path.clone());
                        }
                        break;
                    }
                }
            }
        }
    }

    let blocked = !hits.is_empty();
    GuardOutcome {
        name: "secrets",
        blocked,
        summary: if blocked {
            "secrets HIT".to_string()
        } else {
            "no secrets".to_string()
        },
        reasons,
        paths,
    }
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "K", "M", "G"];
    let mut val = n as f64;
    let mut unit = 0;
    while val >= 1024.0 && unit < UNITS.len() - 1 {
        val /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}{}", n, UNITS[0])
    } else if val < 10.0 {
        format!("{:.1}{}", val, UNITS[unit])
    } else {
        format!("{:.0}{}", val, UNITS[unit])
    }
}

pub struct GuardConfig {
    pub max_files: usize,
    pub max_bytes: u64,
    pub denylist: Vec<String>,
}

/// Evaluate all guards, in the order the project documents them:
/// denylist, max-files, max-bytes, secret-shaped content.
pub fn evaluate(files: &[FileDiff], config: &GuardConfig) -> Result<GuardReport, String> {
    let outcomes = vec![
        eval_denylist(files, &config.denylist)?,
        eval_max_files(files, config.max_files),
        eval_max_bytes(files, config.max_bytes),
        eval_secrets(files),
    ];
    Ok(GuardReport { outcomes })
}
