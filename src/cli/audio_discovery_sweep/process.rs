use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};

#[cfg(unix)]
#[path = "process/unix.rs"]
mod native;
#[cfg(windows)]
#[path = "process/windows.rs"]
mod native;

const RETAIN_BYTES: usize = 64 * 1024;
const OUTPUT_LIMIT: u64 = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Termination {
    Exited,
    TimedOut,
    OutputLimit,
    Cancelled,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ProcessOutcome {
    pub termination: Termination,
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub elapsed_ms: u64,
}

pub(super) fn run(
    executable: &Path,
    args: &[OsString],
    working_directory: &Path,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<ProcessOutcome> {
    let start = Instant::now();
    ensure!(!timeout.is_zero(), "child process timeout must be positive");
    let deadline = start
        .checked_add(timeout)
        .context("child process timeout exceeds the monotonic clock")?;
    let mut stdout = Output::default();
    let mut stderr = Output::default();
    if cancel.load(Ordering::Relaxed) {
        return Ok(outcome(Termination::Cancelled, None, stdout, stderr, start));
    }
    let mut child = ChildGuard::spawn(executable, args, working_directory)?;
    let mut stdout_pipe = child
        .child
        .stdout
        .take()
        .context("child stdout is missing")?;
    let mut stderr_pipe = child
        .child
        .stderr
        .take()
        .context("child stderr is missing")?;
    native::prepare_pipe(&stdout_pipe)?;
    native::prepare_pipe(&stderr_pipe)?;
    let mut termination = loop {
        stdout.poll(&mut stdout_pipe)?;
        stderr.poll(&mut stderr_pipe)?;
        if stdout.exceeded() || stderr.exceeded() {
            break Termination::OutputLimit;
        }
        if cancel.load(Ordering::Relaxed) {
            break Termination::Cancelled;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break Termination::TimedOut;
        }
        if child.child.try_wait()?.is_some() {
            break Termination::Exited;
        }
        std::thread::sleep(POLL_INTERVAL.min(remaining));
    };
    let status = child
        .stop()
        .context("could not stop and reap sweep child")?;
    // Never wait for EOF: a descendant may have inherited a pipe before job assignment.
    while !stdout.exceeded() && stdout.poll(&mut stdout_pipe)? {}
    while !stderr.exceeded() && stderr.poll(&mut stderr_pipe)? {}
    if termination == Termination::Exited && (stdout.exceeded() || stderr.exceeded()) {
        termination = Termination::OutputLimit;
    }
    Ok(outcome(termination, status.code(), stdout, stderr, start))
}

fn outcome(
    termination: Termination,
    exit_code: Option<i32>,
    stdout: Output,
    stderr: Output,
    start: Instant,
) -> ProcessOutcome {
    ProcessOutcome {
        termination,
        exit_code,
        stdout: stdout.tail.into(),
        stderr: stderr.tail.into(),
        stdout_bytes: stdout.bytes,
        stderr_bytes: stderr.bytes,
        elapsed_ms: start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    }
}

#[derive(Default)]
struct Output {
    tail: VecDeque<u8>,
    bytes: u64,
}

impl Output {
    fn poll(&mut self, pipe: &mut impl native::Pipe) -> io::Result<bool> {
        if self.exceeded() {
            return Ok(false);
        }
        let mut buffer = [0; 8192];
        let count = native::read_available(pipe, &mut buffer)?;
        self.bytes += count as u64;
        let discard = (self.tail.len() + count).saturating_sub(RETAIN_BYTES);
        self.tail.drain(..discard);
        self.tail.extend(&buffer[..count]);
        Ok(count > 0)
    }

    fn exceeded(&self) -> bool {
        self.bytes > OUTPUT_LIMIT
    }
}

struct ChildGuard {
    child: Child,
    group: native::Group,
    stopped: bool,
}

impl ChildGuard {
    fn spawn(executable: &Path, args: &[OsString], directory: &Path) -> Result<Self> {
        let group = native::Group::new()?;
        let mut command = Command::new(executable);
        command
            .args(args)
            .current_dir(directory)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        native::configure(&mut command);
        let mut guard = Self {
            child: command.spawn().context("could not start sweep child")?,
            group,
            stopped: false,
        };
        guard.group.attach(&guard.child)?;
        Ok(guard)
    }

    fn stop(&mut self) -> io::Result<ExitStatus> {
        let group_result = self.group.terminate();
        let kill_result = self.child.kill();
        let status = match kill_result {
            Ok(()) => self.child.wait()?,
            Err(error) => self.child.try_wait()?.ok_or(error)?,
        };
        group_result?;
        self.stopped = true;
        Ok(status)
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.stopped {
            let _ = self.stop();
        }
    }
}

#[cfg(test)]
#[path = "process/tests.rs"]
mod tests;
