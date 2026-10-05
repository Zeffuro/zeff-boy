use std::net::{Shutdown, TcpStream};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::Serialize;

use super::{Checkpoint, Machine, sample};
use crate::endpoint::ConnectionScope;
use crate::lockstep::{INPUT_DELAY, Lockstep, MAX_AHEAD, Player};
use crate::wire::{Connection, Identity, Message, admit_scoped};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scenario {
    Normal,
    Jitter,
    Duplicate,
    Stale,
    Conflict,
    Future,
    Malformed,
    Disconnect,
    Timeout,
    Desync,
    WrongSecret,
    Identity,
    Flood,
}

impl Scenario {
    pub const ALL: [Self; 13] = [
        Self::Normal,
        Self::Jitter,
        Self::Duplicate,
        Self::Stale,
        Self::Conflict,
        Self::Future,
        Self::Malformed,
        Self::Disconnect,
        Self::Timeout,
        Self::Desync,
        Self::WrongSecret,
        Self::Identity,
        Self::Flood,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Jitter => "jitter",
            Self::Duplicate => "duplicate",
            Self::Stale => "stale",
            Self::Conflict => "conflict",
            Self::Future => "future",
            Self::Malformed => "malformed",
            Self::Disconnect => "disconnect",
            Self::Timeout => "timeout",
            Self::Desync => "desync",
            Self::WrongSecret => "wrong-secret",
            Self::Identity => "identity",
            Self::Flood => "flood",
        }
    }

    pub fn parse(name: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|scenario| scenario.name() == name)
            .ok_or_else(|| anyhow!("unknown proof scenario"))
    }

    pub fn should_complete(self) -> bool {
        matches!(
            self,
            Self::Normal | Self::Jitter | Self::Duplicate | Self::Stale
        )
    }
}

#[derive(Debug, Serialize)]
pub struct InputRecord {
    pub frame: u64,
    pub ports: [u8; 2],
}

#[derive(Debug, Serialize)]
pub struct PeerReport {
    pub player: u8,
    pub admitted: bool,
    pub frames: u64,
    pub outcome: String,
    pub transport_error: Option<String>,
    pub identity: Identity,
    pub cpu_cycles: u64,
    pub transcript: Option<[u8; 32]>,
    pub checkpoint: Option<Checkpoint>,
    pub reference_checked_frames: u64,
    pub history: Vec<InputRecord>,
    pub persistence: &'static str,
}

pub fn run_peer(
    stream: TcpStream,
    player: Player,
    build: [u8; 32],
    secret: [u8; 32],
    frames: u64,
    scenario: Scenario,
) -> Result<PeerReport> {
    run_peer_with_setup(
        stream,
        frames,
        scenario,
        PeerSetup {
            player,
            build,
            secret,
            scope: ConnectionScope::Loopback,
        },
    )
}

pub struct PeerSetup {
    pub player: Player,
    pub build: [u8; 32],
    pub secret: [u8; 32],
    pub scope: ConnectionScope,
}

pub fn run_peer_with_setup(
    stream: TcpStream,
    frames: u64,
    scenario: Scenario,
    setup: PeerSetup,
) -> Result<PeerReport> {
    let PeerSetup {
        player,
        build,
        mut secret,
        scope,
    } = setup;
    ensure!(
        (8..=100_000).contains(&frames),
        "proof frames must be 8..100000"
    );
    let mut machine = Machine::new()?;
    let mut identity = machine.identity(build)?;
    let injector = stream.try_clone()?;
    if player == Player::Two {
        match scenario {
            Scenario::Identity => identity.config[0] ^= 1,
            Scenario::WrongSecret => secret[0] ^= 1,
            _ => {}
        }
    }
    let mut schedule = Lockstep::new();
    let mut report = PeerReport {
        player: if player == Player::One { 1 } else { 2 },
        admitted: false,
        frames: 0,
        outcome: String::new(),
        transport_error: None,
        identity: identity.clone(),
        cpu_cycles: 0,
        transcript: None,
        checkpoint: None,
        reference_checked_frames: 0,
        history: Vec::new(),
        persistence: "leased-discard",
    };
    let result = (|| -> Result<()> {
        let mut connection = admit_scoped(stream, player, &identity, &secret, scope)?;
        report.admitted = true;
        report.transcript = Some(connection.transcript());
        run_frames(
            &mut connection,
            &injector,
            &mut machine,
            &mut schedule,
            &mut report,
            player,
            frames,
            scenario,
        )?;
        Ok(())
    })();
    report.frames = machine.frame;
    report.cpu_cycles = machine.emu.cpu_cycles();
    report.history = schedule
        .history()
        .iter()
        .map(|input| InputRecord {
            frame: input.frame,
            ports: input.ports,
        })
        .collect();
    report.transport_error = result
        .as_ref()
        .err()
        .and_then(|error| error.downcast_ref::<std::io::Error>())
        .map(|error| format!("{:?}", error.kind()));
    report.outcome = match result {
        Ok(()) => "complete".to_string(),
        Err(error) => format!("{error:#}"),
    };
    schedule.close();
    machine.close();
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
fn run_frames(
    connection: &mut Connection,
    injector: &TcpStream,
    machine: &mut Machine,
    schedule: &mut Lockstep,
    report: &mut PeerReport,
    player: Player,
    frames: u64,
    scenario: Scenario,
) -> Result<()> {
    let mut reference = if player == Player::One {
        Some(Machine::new()?)
    } else {
        None
    };
    for frame in 0..frames {
        let scheduled = frame
            .checked_add(INPUT_DELAY)
            .ok_or_else(|| anyhow!("frame overflow"))?;
        let buttons = sample(player, frame);
        schedule
            .submit(player, scheduled, buttons)
            .map_err(anyhow::Error::msg)?;
        let input = Message::Input {
            player,
            frame: scheduled,
            buttons: u16::from(buttons),
        };
        if player == Player::Two {
            inject_before(connection, injector, frame, scenario)?;
        }
        connection.send(&input)?;
        if player == Player::Two {
            match scenario {
                Scenario::Duplicate => connection.send(&input)?,
                Scenario::Flood if frame == 5 => {
                    for _ in 0..32 {
                        connection.send(&input)?;
                    }
                }
                Scenario::Conflict if frame == 5 => connection.send(&Message::Input {
                    player,
                    frame: scheduled,
                    buttons: u16::from(buttons ^ 1),
                })?,
                _ => {}
            }
        }
        receive_input(connection, schedule, scheduled)?;
        let confirmed = schedule
            .advance()
            .map_err(anyhow::Error::msg)?
            .ok_or_else(|| anyhow!("input missing at lockstep boundary"))?;
        let mut checkpoint = machine.step(confirmed)?;
        if player == Player::Two && scenario == Scenario::Desync && frame == 5 {
            machine.emu.cpu_write8(0x6000, 0x99);
            checkpoint.logical = machine.logical()?;
            checkpoint.persistent = machine.persistent();
        }
        report.checkpoint = Some(checkpoint.clone());
        if let Some(reference) = reference.as_mut() {
            let ports = if frame < INPUT_DELAY {
                [0, 0]
            } else {
                [
                    sample(Player::One, frame - INPUT_DELAY),
                    sample(Player::Two, frame - INPUT_DELAY),
                ]
            };
            let expected = reference.step(crate::lockstep::ConfirmedInput { frame, ports })?;
            ensure!(
                checkpoint == expected,
                "network execution differs from local reference at frame {}",
                checkpoint.frame
            );
            report.reference_checked_frames += 1;
        }
        connection.send(&checkpoint.message())?;
        let remote = receive_checkpoint(connection, schedule)?;
        ensure!(
            remote == checkpoint.message(),
            "checkpoint mismatch at frame {}",
            checkpoint.frame
        );
        if player == Player::One && frames >= 10_000 && (frame + 1) % 10_000 == 0 {
            eprintln!(
                "checked {} of {frames} frames against peer and local reference",
                frame + 1
            );
        }
    }
    connection.send(&Message::Close { frame: frames })?;
    ensure!(
        connection.receive()? == Message::Close { frame: frames },
        "missing close agreement"
    );
    Ok(())
}

fn inject_before(
    connection: &mut Connection,
    stream: &TcpStream,
    frame: u64,
    scenario: Scenario,
) -> Result<()> {
    if scenario == Scenario::Jitter {
        thread::sleep(Duration::from_millis([0, 1, 4, 2, 8][frame as usize % 5]));
    }
    if frame != 5 {
        return Ok(());
    }
    match scenario {
        Scenario::Stale => connection.send(&Message::Input {
            player: Player::Two,
            frame: 0,
            buttons: 0,
        })?,
        Scenario::Future => connection.send(&Message::Input {
            player: Player::Two,
            frame: frame + MAX_AHEAD + 1,
            buttons: 0,
        })?,
        Scenario::Malformed => {
            connection.send_invalid_length_for_test()?;
            // Dropping an unread socket can reset it before the host reads the injection.
            for _ in 0..2 {
                if connection.receive().is_err() {
                    bail!("injected malformed length");
                }
            }
            bail!("malformed peer did not close after injection");
        }
        Scenario::Disconnect => {
            stream.shutdown(Shutdown::Both)?;
            bail!("injected disconnect");
        }
        Scenario::Timeout => {
            thread::sleep(Duration::from_secs(3));
            bail!("injected timeout");
        }
        _ => {}
    }
    Ok(())
}

fn receive_input(
    connection: &mut Connection,
    schedule: &mut Lockstep,
    expected: u64,
) -> Result<()> {
    for _ in 0..16 {
        match connection.receive().context("receiving scheduled input")? {
            Message::Input {
                player,
                frame,
                buttons,
            } => {
                schedule
                    .submit(
                        player,
                        frame,
                        u8::try_from(buttons).context("unsupported proof input bits")?,
                    )
                    .map_err(anyhow::Error::msg)?;
                if frame == expected {
                    return Ok(());
                }
            }
            _ => bail!("unexpected message while waiting for input"),
        }
    }
    bail!("input packet budget exceeded")
}

fn receive_checkpoint(connection: &mut Connection, schedule: &mut Lockstep) -> Result<Message> {
    for _ in 0..16 {
        match connection.receive()? {
            input @ Message::Checkpoint { .. } => return Ok(input),
            Message::Input {
                player,
                frame,
                buttons,
            } => {
                schedule
                    .submit(
                        player,
                        frame,
                        u8::try_from(buttons).context("unsupported proof input bits")?,
                    )
                    .map_err(anyhow::Error::msg)?;
            }
            _ => bail!("unexpected message while waiting for checkpoint"),
        }
    }
    bail!("checkpoint packet budget exceeded")
}
