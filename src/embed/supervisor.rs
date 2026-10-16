//! Timer-based memory guard: run `embed` in a child process and kill it the
//! moment available memory drops below the floor, even mid-batch.
//!
//! The in-process guard (codeindex's `MemoryGuard`) only checks between
//! batches, and a running ONNX batch cannot be interrupted in-process. Killing
//! the child frees everything it holds; embeddings are persisted per batch, so
//! the restarted child resumes where the killed one stopped.

use std::path::Path;
use std::process::{Child, Command};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use codeindex_embedding::memory::available_memory_mib;

use crate::config::MemoryGuardConfig;

/// Set on the supervised child so it runs `embed` directly.
pub const CHILD_ENV: &str = "DECOMBINE_EMBED_SUPERVISED";

pub fn is_supervised_child() -> bool {
    std::env::var_os(CHILD_ENV).is_some()
}

/// Run `decombine --config <config_path> embed` under supervision until it
/// completes. Returns an error if the child fails for any reason other than a
/// memory kill.
pub fn supervise_embed(
    config_path: &Path,
    guard: &MemoryGuardConfig,
    interval: Duration,
) -> Result<()> {
    let exe = std::env::current_exe().context("locating the decombine executable")?;
    let resume = guard.resume_above_mb();
    loop {
        wait_for_resume(resume, interval);
        let mut child = Command::new(&exe)
            .arg("--config")
            .arg(config_path)
            .arg("embed")
            .env(CHILD_ENV, "1")
            .spawn()
            .context("spawning supervised embed")?;
        match watch(&mut child, guard.pause_below_mb, interval)? {
            Watch::Finished => return Ok(()),
            Watch::Killed(available) => eprintln!(
                "embed killed: {available} MiB available < {} MiB (memory_guard); \
                 resuming at >= {resume} MiB",
                guard.pause_below_mb
            ),
        }
    }
}

enum Watch {
    Finished,
    Killed(u64),
}

fn watch(child: &mut Child, pause_below_mib: u64, interval: Duration) -> Result<Watch> {
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(Watch::Finished);
            }
            bail!("supervised embed failed: {status}");
        }
        if let Some(available) = available_memory_mib().filter(|mib| *mib < pause_below_mib) {
            // SIGKILL: the point is to free memory now, not after cleanup.
            child.kill().ok();
            child.wait()?;
            return Ok(Watch::Killed(available));
        }
        std::thread::sleep(interval);
    }
}

fn wait_for_resume(resume_above_mib: u64, interval: Duration) {
    let mut announced = false;
    while let Some(available) = available_memory_mib().filter(|mib| *mib < resume_above_mib) {
        if !announced {
            eprintln!(
                "embed waiting: {available} MiB available, need {resume_above_mib} MiB \
                 (memory_guard)"
            );
            announced = true;
        }
        std::thread::sleep(interval.max(Duration::from_secs(1)));
    }
}
