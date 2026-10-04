use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use serde_json::json;
use zeff_netplay::endpoint::ConnectionScope;
use zeff_netplay::lockstep::Player;
use zeff_netplay::proof::{PeerSetup, Scenario, run_peer_with_setup};

#[derive(Default)]
pub(super) struct Options {
    pub listen: Option<SocketAddr>,
    pub peer: Option<SocketAddr>,
    pub test_build: Option<[u8; 32]>,
}

pub(super) fn run(
    frames: u64,
    scenario: Scenario,
    artifact: [u8; 32],
    options: Options,
) -> Result<()> {
    let scope = ConnectionScope::TrustedPrivate;
    ensure!(
        options.listen.is_some() != options.peer.is_some(),
        "select exactly one remote proof role"
    );
    let secret = super::native::decode_secret(
        &std::env::var("ZEFF_NETPLAY_PROOF_SECRET")
            .context("remote proof requires a capability")?,
    )?;
    let (stream, player) = if let Some(address) = options.listen {
        scope.validate_bind(address)?;
        let listener = TcpListener::bind(address).context("binding private proof listener")?;
        listener.set_nonblocking(true)?;
        println!(
            "{}",
            json!({"listening": listener.local_addr()?.to_string()})
        );
        std::io::stdout().flush()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let stream = loop {
            ensure!(
                Instant::now() < deadline,
                "private proof accept deadline exceeded"
            );
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error).context("accepting private proof peer"),
            }
        };
        (stream, Player::One)
    } else {
        let address = options.peer.context("missing private proof destination")?;
        scope.validate_destination(address)?;
        (
            TcpStream::connect_timeout(&address, Duration::from_secs(2))
                .context("connecting private proof peer")?,
            Player::Two,
        )
    };
    scope.validate_connection(&stream)?;
    let local_endpoint = stream.local_addr()?.to_string();
    let peer_endpoint = stream.peer_addr()?.to_string();
    let started = Instant::now();
    let report = run_peer_with_setup(
        stream,
        frames,
        scenario,
        PeerSetup {
            player,
            build: options.test_build.unwrap_or(artifact),
            secret,
            scope,
        },
    )?;
    println!(
        "{}",
        json!({
            "scope": "trusted-private-plaintext",
            "identity_mode": if options.test_build.is_some() { "synthetic-test-build" } else { "exact-artifact" },
            "artifact_sha256": artifact,
            "scenario": scenario.name(),
            "requested_frames": frames,
            "local_endpoint": local_endpoint,
            "peer_endpoint": peer_endpoint,
            "elapsed_ms": started.elapsed().as_millis(),
            "report": report,
        })
    );
    Ok(())
}
