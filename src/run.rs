use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct RunOutcome {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration: Duration,
}

pub fn run_command(
    cwd: &Path,
    cmd: &[String],
    extra_env: &[(&str, String)],
    timeout: Option<Duration>,
) -> Result<RunOutcome, String> {
    let mut command = Command::new(&cmd[0]);
    command.args(&cmd[1..]);
    command.current_dir(cwd);
    for (k, v) in extra_env {
        command.env(k, v);
    }
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // New process group, headed by the child itself, so a timeout can signal
    // the whole tree (including grandchildren the child spawns) at once.
    command.process_group(0);

    let start = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|e| format!("failed to run {:?}: {}", cmd[0], e))?;
    let pgid = child.id() as i32;

    let (exit_code, timed_out) = match timeout {
        None => {
            let status = child.wait().map_err(|e| e.to_string())?;
            (status.code(), false)
        }
        Some(limit) => wait_with_timeout(&mut child, pgid, limit)?,
    };

    Ok(RunOutcome {
        exit_code,
        timed_out,
        duration: start.elapsed(),
    })
}

fn wait_with_timeout(
    child: &mut Child,
    pgid: i32,
    limit: Duration,
) -> Result<(Option<i32>, bool), String> {
    let start = Instant::now();
    let poll = Duration::from_millis(100);

    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok((status.code(), false));
        }
        if start.elapsed() >= limit {
            break;
        }
        std::thread::sleep(poll);
    }

    // Grace period: SIGTERM, then SIGKILL if the group is still alive.
    unsafe {
        libc::kill(-pgid, libc::SIGTERM);
    }
    let grace_start = Instant::now();
    let grace = Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok((status.code(), true));
        }
        if grace_start.elapsed() >= grace {
            break;
        }
        std::thread::sleep(poll);
    }

    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    Ok((status.code(), true))
}
