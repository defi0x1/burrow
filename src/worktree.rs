use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Layout {
    pub repo_root: PathBuf,
    pub state_dir: PathBuf,
}

impl Layout {
    pub fn new(repo_root: PathBuf) -> Self {
        let state_dir = repo_root.join(".burrow");
        Layout {
            repo_root,
            state_dir,
        }
    }

    pub fn burrows_dir(&self) -> PathBuf {
        self.state_dir.join("burrows")
    }

    pub fn meta_root(&self) -> PathBuf {
        self.state_dir.join("meta")
    }

    pub fn worktree_path(&self, id: &str) -> PathBuf {
        self.burrows_dir().join(id)
    }

    pub fn meta_dir(&self, id: &str) -> PathBuf {
        self.meta_root().join(id)
    }

    pub fn ensure_dirs(&self) -> Result<(), String> {
        fs::create_dir_all(self.burrows_dir()).map_err(|e| e.to_string())?;
        fs::create_dir_all(self.meta_root()).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        if let Ok(entries) = fs::read_dir(self.meta_root()) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        ids.push(name.to_string());
                    }
                }
            }
        }
        ids.sort();
        ids
    }
}

/// A short, sortable, human-readable id: timestamp plus a random suffix.
pub fn new_id() -> String {
    let now = SystemTime::now();
    let secs = now.duration_since(UNIX_EPOCH).unwrap().as_secs();
    let tm = format_timestamp(secs);
    format!("{}-{}", tm, short_suffix())
}

fn format_timestamp(secs: u64) -> String {
    // Minimal, dependency-free UTC civil-from-days conversion.
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}T{:02}-{:02}-{:02}", y, mo, d, h, m, s)
}

/// Howard Hinnant's days-from-civil algorithm, inverted.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn short_suffix() -> String {
    let mut buf = [0u8; 2];
    if let Ok(mut f) = fs::File::open("/dev/urandom") {
        if f.read_exact(&mut buf).is_ok() {
            return format!("{:02x}{:02x}", buf[0], buf[1]);
        }
    }
    // Fallback: nanosecond jitter plus pid, still unique enough in practice.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let pid = std::process::id();
    format!("{:04x}", (nanos ^ pid) & 0xffff)
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (y, mo, d) = civil_from_days((secs / 86400) as i64);
    let rem = secs % 86400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, m, s)
}

pub struct Lock {
    path: PathBuf,
    active: bool,
}

impl Lock {
    pub fn acquire(meta_dir: &Path) -> Result<Lock, String> {
        let path = meta_dir.join("lock");
        fs::write(&path, std::process::id().to_string()).map_err(|e| e.to_string())?;
        Ok(Lock { path, active: true })
    }

    /// Release the lock without deleting anything else. Called explicitly so
    /// that ordering relative to receipt writes is obvious at the call site.
    pub fn release(mut self) {
        let _ = fs::remove_file(&self.path);
        self.active = false;
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        if self.active {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub fn lock_path(meta_dir: &Path) -> PathBuf {
    meta_dir.join("lock")
}

pub fn is_locked(meta_dir: &Path) -> bool {
    lock_path(meta_dir).exists()
}

/// A lock is stale when its pid is no longer alive: the process that held it
/// died without a chance to clean up.
pub fn is_stale(meta_dir: &Path) -> bool {
    match fs::read_to_string(lock_path(meta_dir)) {
        Ok(s) => match s.trim().parse::<i32>() {
            Ok(pid) => unsafe { libc::kill(pid, 0) != 0 },
            Err(_) => true,
        },
        Err(_) => false,
    }
}
