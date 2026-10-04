use std::fs::File;
use std::io::{ErrorKind, Read};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use zeff_netplay::endpoint::ConnectionScope;
use zeff_netplay::lockstep::Player;
use zeff_netplay::rollback::InputDelay;

use super::Start;

const CONNECT_BUDGET: Duration = Duration::from_secs(30);
const CONNECT_ATTEMPT: Duration = Duration::from_millis(100);
const RETRY_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HostOptions {
    pub(crate) address: SocketAddr,
    pub(crate) scope: ConnectionScope,
    pub(crate) input_delay: InputDelay,
}

impl HostOptions {
    pub(crate) fn validate(self) -> Result<()> {
        self.scope
            .validate_bind(self.address)
            .context("Host address must belong to the selected local or private network.")
    }
}

pub(crate) struct Connector {
    result: Option<Receiver<Result<Start>>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    allow_different_versions: bool,
}

impl Connector {
    pub(crate) fn host(options: HostOptions) -> Result<(Self, String)> {
        options.validate()?;
        Self::host_with_options(options, executable_build()?, CONNECT_BUDGET)
    }

    pub(crate) fn join(invitation: &str, scope: ConnectionScope) -> Result<Self> {
        let (address, secret, input_delay) = parse_invitation(invitation, scope)?;
        Self::join_with_build(
            address,
            secret,
            executable_build()?,
            CONNECT_BUDGET,
            scope,
            input_delay,
        )
    }

    #[cfg(test)]
    fn host_with_build(build: [u8; 32], budget: Duration) -> Result<(Self, String)> {
        Self::host_with_options(
            HostOptions {
                address: "127.0.0.1:0".parse().unwrap(),
                scope: ConnectionScope::Loopback,
                input_delay: InputDelay::default(),
            },
            build,
            budget,
        )
    }

    fn host_with_options(
        options: HostOptions,
        build: [u8; 32],
        budget: Duration,
    ) -> Result<(Self, String)> {
        options.validate()?;
        let deadline = Instant::now() + budget;
        let listener = TcpListener::bind(options.address).context(
            "Could not host netplay. Check the local address and choose an unused port.",
        )?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let mut secret = [0; 32];
        getrandom::fill(&mut secret).context("creating netplay invitation")?;
        let invitation = format!(
            "{address}/{}/{}",
            const_hex::encode(secret),
            options.input_delay.frames()
        );
        let connector = Self::spawn(move |cancelled| {
            loop {
                check_pending(cancelled, deadline, options.scope)?;
                match listener.accept() {
                    Ok((stream, _)) => {
                        check_pending(cancelled, deadline, options.scope)?;
                        return make_start(stream, Player::One, build, secret, cancelled, options);
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            ErrorKind::WouldBlock | ErrorKind::Interrupted
                        ) =>
                    {
                        pause_retry(deadline);
                    }
                    Err(error) => return Err(error).context("accepting netplay peer"),
                }
            }
        })?;
        Ok((connector, invitation))
    }

    fn join_with_build(
        address: SocketAddr,
        secret: [u8; 32],
        build: [u8; 32],
        budget: Duration,
        scope: ConnectionScope,
        input_delay: InputDelay,
    ) -> Result<Self> {
        scope.validate_destination(address)?;
        let deadline = Instant::now() + budget;
        Self::spawn(move |cancelled| {
            loop {
                check_pending(cancelled, deadline, scope)?;
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    bail!(timeout_message(scope));
                }
                match TcpStream::connect_timeout(&address, remaining.min(CONNECT_ATTEMPT)) {
                    Ok(stream) => {
                        check_pending(cancelled, deadline, scope)?;
                        return make_start(
                            stream,
                            Player::Two,
                            build,
                            secret,
                            cancelled,
                            HostOptions {
                                address,
                                scope,
                                input_delay,
                            },
                        );
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            ErrorKind::ConnectionRefused
                                | ErrorKind::TimedOut
                                | ErrorKind::WouldBlock
                                | ErrorKind::Interrupted
                        ) =>
                    {
                        pause_retry(deadline);
                    }
                    Err(error) => {
                        return Err(error).context(
                            "Could not join netplay. Check the host address and selected network.",
                        );
                    }
                }
            }
        })
    }

    fn spawn(run: impl FnOnce(&AtomicBool) -> Result<Start> + Send + 'static) -> Result<Self> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let (sender, result) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("nes-netplay-connect".to_owned())
            .spawn(move || {
                let connected = run(&worker_cancelled);
                if !worker_cancelled.load(Ordering::Acquire) {
                    let _ = sender.try_send(connected);
                }
            })
            .context("starting netplay connection worker")?;
        Ok(Self {
            result: Some(result),
            cancelled,
            worker: Some(worker),
            allow_different_versions: false,
        })
    }

    pub(crate) fn poll(&mut self) -> Result<Option<Start>> {
        let Some(receiver) = &self.result else {
            return Ok(None);
        };
        let connected = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return Ok(None),
            Err(TryRecvError::Disconnected) => {
                Err(anyhow::anyhow!("netplay connection worker disconnected"))
            }
        };
        self.result.take();
        self.join_worker();
        connected.map(|mut start| {
            start.allow_different_versions = self.allow_different_versions;
            Some(start)
        })
    }

    pub(crate) fn set_version_consent(&mut self, allow: bool) {
        self.allow_different_versions = allow;
    }

    pub(crate) fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.result.take();
        if let Some(worker) = &self.worker {
            worker.thread().unpark();
        }
        self.join_worker();
    }

    fn join_worker(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Connector {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn parse_invitation(
    invitation: &str,
    scope: ConnectionScope,
) -> Result<(SocketAddr, [u8; 32], InputDelay)> {
    ensure!(invitation.len() <= 128, "invalid netplay invitation");
    let mut fields = invitation.split('/');
    let address = fields.next().unwrap_or_default();
    let secret = fields
        .next()
        .context("netplay invitation needs address and capability")?;
    let input_delay = match fields.next() {
        Some(value) => {
            ensure!(
                value.len() == 1 && value.bytes().all(|byte| byte.is_ascii_digit()),
                "invalid netplay input delay"
            );
            InputDelay::new(value.parse().context("invalid netplay input delay")?)?
        }
        None => InputDelay::default(),
    };
    ensure!(fields.next().is_none(), "invalid netplay invitation");
    let address: SocketAddr = address
        .parse()
        .map_err(|_| anyhow::anyhow!("netplay address must be a numeric socket address"))?;
    scope.validate_destination(address).context(match scope {
        ConnectionScope::Loopback => "Choose Private LAN / network to join a private-address invitation. Same computer requires a loopback address.",
        ConnectionScope::TrustedPrivate => "Use a numeric local or private address and a nonzero port in the invitation.",
    })?;
    ensure!(
        secret.len() == 64 && secret.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "netplay capability must contain 64 hexadecimal digits"
    );
    let secret = const_hex::decode_to_array(secret)
        .map_err(|_| anyhow::anyhow!("invalid netplay capability"))?;
    Ok((address, secret, input_delay))
}

pub(crate) fn validate_invitation(invitation: &str, scope: ConnectionScope) -> Result<()> {
    parse_invitation(invitation, scope).map(|_| ())
}

pub(crate) fn invitation_delay(invitation: &str, scope: ConnectionScope) -> Result<InputDelay> {
    parse_invitation(invitation, scope).map(|(_, _, input_delay)| input_delay)
}

fn check_pending(cancelled: &AtomicBool, deadline: Instant, scope: ConnectionScope) -> Result<()> {
    ensure!(
        !cancelled.load(Ordering::Acquire),
        "netplay connection cancelled"
    );
    ensure!(Instant::now() < deadline, timeout_message(scope));
    Ok(())
}

fn timeout_message(scope: ConnectionScope) -> &'static str {
    match scope {
        ConnectionScope::Loopback => {
            "netplay connection timed out. Open the other App on this computer and use the current invitation."
        }
        ConnectionScope::TrustedPrivate => {
            "netplay connection timed out. Check the current invitation, private network and host firewall's TCP port rule."
        }
    }
}

fn pause_retry(deadline: Instant) {
    thread::park_timeout(RETRY_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
}

fn make_start(
    stream: TcpStream,
    player: Player,
    build: [u8; 32],
    secret: [u8; 32],
    cancelled: &AtomicBool,
    options: HostOptions,
) -> Result<Start> {
    let HostOptions {
        scope, input_delay, ..
    } = options;
    ensure!(
        !cancelled.load(Ordering::Acquire),
        "netplay connection cancelled"
    );
    scope.validate_connection(&stream)?;
    stream.set_nonblocking(false)?;
    stream.set_nodelay(true)?;
    Ok(Start {
        stream,
        player,
        build,
        secret,
        scope,
        input_delay,
        allow_different_versions: false,
        verify_every_frame: false,
    })
}

pub(crate) fn executable_build() -> Result<[u8; 32]> {
    static BUILD: OnceLock<std::result::Result<[u8; 32], String>> = OnceLock::new();
    // Complete local file I/O before creating the cancellable socket worker.
    BUILD
        .get_or_init(|| hash_executable().map_err(|error| error.to_string()))
        .clone()
        .map_err(anyhow::Error::msg)
}

fn hash_executable() -> Result<[u8; 32]> {
    let path = std::env::current_exe().context("locating netplay executable")?;
    let mut file = File::open(path).context("opening netplay executable")?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let deadline = Instant::now() + CONNECT_BUDGET;
    loop {
        ensure!(
            Instant::now() < deadline,
            "netplay executable hashing timed out"
        );
        let count = file
            .read(&mut buffer)
            .context("hashing netplay executable")?;
        if count == 0 {
            return Ok(hash.finalize().into());
        }
        hash.update(&buffer[..count]);
    }
}

#[cfg(test)]
mod tests;
