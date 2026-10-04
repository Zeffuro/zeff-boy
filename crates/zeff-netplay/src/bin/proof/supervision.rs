use std::io::Read;
use std::process::{Child, Output};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};

const OUTPUT_LIMIT: u64 = 65536;

pub(super) struct ChildGuard(pub Option<Child>);

impl ChildGuard {
    fn terminate(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.0 = None;
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub(super) fn wait_peer(child: &mut ChildGuard, budget: Duration) -> Result<Output> {
    let process = child.0.as_mut().context("missing child process")?;
    let stdout = process.stdout.take().context("missing child stdout")?;
    let stderr = process.stderr.take().context("missing child stderr")?;
    let stdout = thread::spawn(move || read_output(stdout));
    let stderr = thread::spawn(move || read_output(stderr));
    let deadline = Instant::now() + budget;
    let status = (|| -> Result<_> {
        loop {
            if let Some(status) = child.0.as_mut().context("missing child")?.try_wait()? {
                return Ok(status);
            }
            ensure!(Instant::now() < deadline, "peer exit deadline exceeded");
            thread::sleep(Duration::from_millis(5));
        }
    })();
    child.terminate();
    let stdout = stdout
        .join()
        .map_err(|_| anyhow::anyhow!("stdout reader panicked"))?;
    let stderr = stderr
        .join()
        .map_err(|_| anyhow::anyhow!("stderr reader panicked"))?;
    Ok(Output {
        status: status?,
        stdout: stdout?,
        stderr: stderr?,
    })
}

fn read_output(output: impl Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    output.take(OUTPUT_LIMIT + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= OUTPUT_LIMIT,
        "peer output limit exceeded"
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn nonexiting_child() {
        if std::env::var_os("ZEFF_PROOF_SUPERVISION_CHILD").is_some() {
            thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    fn deadline_kills_and_reaps_nonexiting_peer() {
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "supervision::tests::nonexiting_child"])
            .env("ZEFF_PROOF_SUPERVISION_CHILD", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut guard = ChildGuard(Some(child));
        let started = Instant::now();
        let error = wait_peer(&mut guard, Duration::from_millis(100)).unwrap_err();
        assert!(error.to_string().contains("peer exit deadline"));
        assert!(guard.0.is_none());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
