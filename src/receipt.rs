use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone)]
pub struct FileChange {
    pub path: String,
    pub added: u64,
    pub removed: u64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Ok,
    Blocked,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Limits {
    pub max_files: usize,
    pub max_bytes: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Receipt {
    pub id: String,
    pub from_ref: String,
    pub base_commit: String,
    pub base_commit_short: String,
    pub command: Vec<String>,
    pub started_at: String,
    pub finished_at: String,
    pub duration_secs: u64,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub files: Vec<FileChange>,
    pub files_changed: usize,
    pub bytes_added: u64,
    pub limits: Limits,
    pub denylist: Vec<String>,
    pub verdict: Verdict,
    pub reasons: Vec<String>,
    pub offending_paths: Vec<String>,
    pub guard_summary: Vec<String>,
    pub kept: bool,
}

/// Metadata written when a burrow starts, before we know the outcome.
#[derive(Serialize, Deserialize, Clone)]
pub struct BurrowInfo {
    pub id: String,
    pub from_ref: String,
    pub base_commit: String,
    pub base_commit_short: String,
    pub command: Vec<String>,
    pub started_at: String,
    pub started_at_unix: u64,
    pub pid: u32,
}

pub fn save_info(meta_dir: &Path, info: &BurrowInfo) -> Result<(), String> {
    let json = serde_json::to_string_pretty(info).map_err(|e| e.to_string())?;
    std::fs::write(meta_dir.join("info.json"), json).map_err(|e| e.to_string())
}

pub fn load_info(meta_dir: &Path) -> Result<BurrowInfo, String> {
    let s = std::fs::read_to_string(meta_dir.join("info.json")).map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

pub fn save_receipt(meta_dir: &Path, receipt: &Receipt) -> Result<(), String> {
    let json = serde_json::to_string_pretty(receipt).map_err(|e| e.to_string())?;
    std::fs::write(meta_dir.join("receipt.json"), json).map_err(|e| e.to_string())
}

pub fn load_receipt(meta_dir: &Path) -> Result<Receipt, String> {
    let s = std::fs::read_to_string(meta_dir.join("receipt.json")).map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

pub fn has_receipt(meta_dir: &Path) -> bool {
    meta_dir.join("receipt.json").exists()
}
