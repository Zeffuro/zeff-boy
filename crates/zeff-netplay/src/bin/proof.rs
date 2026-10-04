#[cfg(not(target_arch = "wasm32"))]
#[path = "proof/supervision.rs"]
mod supervision;

#[cfg(not(target_arch = "wasm32"))]
#[path = "proof/remote.rs"]
mod remote;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::io::Read;
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::supervision::{ChildGuard, wait_peer};
    use anyhow::{Context, Result, bail, ensure};
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use zeff_netplay::lockstep::Player;
    use zeff_netplay::proof::{Scenario, run_peer};

    pub fn run() -> Result<()> {
        let mut frames = 300;
        let mut scenario = "normal".to_string();
        let mut peer = None;
        let mut network = super::remote::Options::default();
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let value = args.next().context("proof options require a value")?;
            match arg.as_str() {
                "--frames" => frames = value.parse()?,
                "--scenario" => scenario = value,
                "--peer" => peer = Some(value.parse::<SocketAddr>()?),
                "--listen" => network.listen = Some(value.parse::<SocketAddr>()?),
                "--network-peer" => network.peer = Some(value.parse::<SocketAddr>()?),
                "--test-build" => network.test_build = Some(decode_secret(&value)?),
                _ => bail!("unknown proof option {arg}"),
            }
        }
        ensure!(
            (8..=100_000).contains(&frames),
            "proof frames must be 8..100000"
        );
        let build = artifact_digest()?;
        if network.listen.is_some() || network.peer.is_some() {
            ensure!(
                peer.is_none(),
                "remote proof cannot use the child-peer option"
            );
            return super::remote::run(frames, Scenario::parse(&scenario)?, build, network);
        }
        ensure!(
            network.test_build.is_none(),
            "test build requires remote proof mode"
        );
        if let Some(address) = peer {
            ensure!(address.ip().is_loopback(), "proof only joins loopback");
            let secret = decode_secret(&std::env::var("ZEFF_NETPLAY_PROOF_SECRET")?)?;
            let stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
            let report = run_peer(
                stream,
                Player::Two,
                build,
                secret,
                frames,
                Scenario::parse(&scenario)?,
            )?;
            println!("{}", serde_json::to_string(&report)?);
            return Ok(());
        }
        let scenarios = if scenario == "all" {
            Scenario::ALL.to_vec()
        } else {
            vec![Scenario::parse(&scenario)?]
        };
        for scenario in scenarios {
            experiment(frames, scenario, build)?;
        }
        Ok(())
    }

    fn experiment(frames: u64, scenario: Scenario, build: [u8; 32]) -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let mut secret = [0; 32];
        getrandom::fill(&mut secret).map_err(|error| anyhow::anyhow!("random secret: {error}"))?;
        let child = Command::new(std::env::current_exe()?)
            .args([
                "--peer",
                &listener.local_addr()?.to_string(),
                "--frames",
                &frames.to_string(),
                "--scenario",
                scenario.name(),
            ])
            .env("ZEFF_NETPLAY_PROOF_SECRET", encode_secret(secret))
            .env("ZEFF_MUTE_AUDIO", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut child = ChildGuard(Some(child));
        let deadline = Instant::now() + Duration::from_secs(10);
        let stream = loop {
            if child
                .0
                .as_mut()
                .context("missing child process")?
                .try_wait()?
                .is_some()
            {
                let output = wait_peer(&mut child, Duration::from_secs(1))?;
                bail!(
                    "proof peer exited before connection: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error).context("proof accept deadline"),
            }
        };
        let started = Instant::now();
        let host = run_peer(stream, Player::One, build, secret, frames, scenario)?;
        let output = wait_peer(&mut child, Duration::from_secs(4))?;
        ensure!(
            output.status.success(),
            "peer failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let peer: Value = serde_json::from_slice(&output.stdout).context("peer proof report")?;
        if scenario.should_complete() {
            ensure!(
                host.outcome == "complete" && peer["outcome"] == "complete",
                "unexpected failure: host={} peer={}",
                host.outcome,
                peer["outcome"]
            );
            ensure!(
                host.frames == frames && peer["frames"] == frames,
                "incomplete frame ledger"
            );
            ensure!(
                host.reference_checked_frames == frames,
                "incomplete local reference ledger"
            );
            ensure!(
                serde_json::to_value(&host.checkpoint)? == peer["checkpoint"],
                "final checkpoint disagreement"
            );
            ensure!(
                serde_json::to_value(host.transcript)? == peer["transcript"],
                "transcript disagreement"
            );
        } else {
            let (boundary, cause) = match scenario {
                Scenario::Conflict => (6, "input conflicts with pending input"),
                Scenario::Future => (5, "input exceeds the pending frame window"),
                Scenario::Malformed => (5, "invalid packet length"),
                Scenario::Disconnect | Scenario::Timeout => (5, ""),
                Scenario::Desync => (6, "checkpoint mismatch at frame 6"),
                Scenario::WrongSecret => (0, "authentication failed"),
                Scenario::Identity => (0, "session identity mismatch"),
                Scenario::Flood => (6, "checkpoint packet budget exceeded"),
                _ => unreachable!("successful scenarios handled above"),
            };
            ensure!(
                host.frames == boundary && host.outcome.contains(cause),
                "unexpected fault boundary/cause: frame={} cause={}",
                host.frames,
                host.outcome
            );
            if scenario == Scenario::Disconnect {
                ensure!(
                    host.transport_error.as_deref().is_some_and(|kind| matches!(
                        kind,
                        "ConnectionAborted"
                            | "ConnectionReset"
                            | "BrokenPipe"
                            | "NotConnected"
                            | "UnexpectedEof"
                            | "WriteZero"
                    )),
                    "disconnect did not produce a transport closure"
                );
            }
            if scenario == Scenario::Timeout {
                ensure!(
                    host.transport_error
                        .as_deref()
                        .is_some_and(|kind| matches!(kind, "WouldBlock" | "TimedOut")),
                    "stall did not produce a transport timeout"
                );
            }
            ensure!(
                peer["outcome"] != "complete"
                    && peer["frames"]
                        .as_u64()
                        .is_some_and(|frame| if boundary == 0 {
                            frame == 0
                        } else {
                            (5..=6).contains(&frame)
                        }),
                "peer continued past fault boundary"
            );
            let injected_cause = match scenario {
                Scenario::Malformed => Some("injected malformed length"),
                Scenario::Disconnect => Some("injected disconnect"),
                Scenario::Timeout => Some("injected timeout"),
                _ => None,
            };
            if let Some(cause) = injected_cause {
                ensure!(
                    peer["outcome"] == cause,
                    "peer did not execute intended fault"
                );
            }
            if matches!(scenario, Scenario::Identity | Scenario::WrongSecret) {
                ensure!(
                    !host.admitted && peer["admitted"] == false && peer["frames"] == 0,
                    "failed admission ran the core"
                );
            } else {
                ensure!(
                    host.admitted && peer["admitted"] == true,
                    "runtime fault was rejected before its injection"
                );
            }
        }
        let diagnostics = if scenario.should_complete() {
            Value::Null
        } else {
            json!({ "host": host, "peer": peer })
        };
        println!(
            "{}",
            json!({
                "scenario": scenario.name(), "passed": true, "frames": host.frames,
                "checkpoint": host.checkpoint, "elapsed_ms": started.elapsed().as_millis(),
                "reference_checked_frames": host.reference_checked_frames,
                "persistence": host.persistence, "diagnostics": diagnostics,
            })
        );
        Ok(())
    }

    fn artifact_digest() -> Result<[u8; 32]> {
        let mut artifact = std::fs::File::open(std::env::current_exe()?)?;
        let mut buffer = [0; 65536];
        let mut digest = Sha256::new();
        loop {
            let size = artifact.read(&mut buffer)?;
            if size == 0 {
                break;
            }
            digest.update(&buffer[..size]);
        }
        Ok(digest.finalize().into())
    }

    fn encode_secret(secret: [u8; 32]) -> String {
        secret.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub(super) fn decode_secret(secret: &str) -> Result<[u8; 32]> {
        ensure!(
            secret.len() == 64 && secret.is_ascii(),
            "invalid proof secret"
        );
        let mut bytes = [0; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&secret[index * 2..index * 2 + 2], 16)?;
        }
        Ok(bytes)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    native::run()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
