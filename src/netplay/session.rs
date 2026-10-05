use crate::platform::Instant;
use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use zeff_nes_core::emulator::rollback::{NesRollbackSession, NesRollbackSnapshot};
use zeff_netplay::lockstep::Player;
use zeff_netplay::rollback::{PREDICTION_WINDOW, Timeline};
use zeff_netplay::wire::Message;

use super::{
    Response, Start, identity,
    network::{Event, Network},
};
use crate::emu_backend::EmuBackend;

mod checks;
mod core;
use core::{Lease, Snapshot};
mod execution;
mod pause;
use checks::Hashes;
use execution::FrameBucket;
use pause::PauseControl;

const STALL_LIMIT: Duration = Duration::from_secs(3);
const HEARTBEAT: Duration = Duration::from_millis(100);
const EVENT_BUDGET: usize = 32;
const RESPONSE_CAPACITY: usize = 64;

pub(crate) struct Session {
    network: Network,
    player: Player,
    timeline: Timeline,
    lease: Lease,
    restore_snapshot: Snapshot,
    snapshots: BTreeMap<u64, Snapshot>,
    outputs: BTreeMap<u64, FrameBucket>,
    committed: u64,
    responses: VecDeque<Response>,
    hashes: Hashes,
    pause: PauseControl,
    config: [u8; 32],
    checkpoint: Vec<u8>,
    persistent: [u8; 32],
    publication: bool,
    verification: bool,
    ready: bool,
    admission_since: Instant,
    stalled_since: Option<Instant>,
    heartbeat_at: Instant,
    stats_at: Instant,
    sampled: Option<u64>,
    peer_progress: (u64, u64),
    chat_sent: super::chat::RateLimit,
    chat_received: super::chat::RateLimit,
}

impl Session {
    #[cfg(test)]
    pub(crate) fn corrupt_restore_checkpoint_for_test(&mut self) {
        self.checkpoint.truncate(1);
    }

    pub(crate) fn start(backend: &mut EmuBackend, start: Start) -> Result<Self> {
        let admission = identity::identity_with_delay(backend, start.build, start.input_delay)?;
        #[cfg(not(target_arch = "wasm32"))]
        let mut admission = admission;
        #[cfg(not(target_arch = "wasm32"))]
        {
            admission.build_info =
                super::compatibility::describe(backend, start.allow_different_versions);
        }
        #[cfg(target_arch = "wasm32")]
        ensure!(
            !start.allow_different_versions,
            "browser netplay requires matching builds"
        );
        let checkpoint = backend.encode_state_bytes()?;
        let publication = core::persistence(backend, None)?;
        let lease = Lease::begin(backend, start.player)?;
        let snapshot = lease.capture(backend)?;
        let restore_snapshot = lease.capture(backend)?;
        ensure!(
            snapshot.frame() == 0,
            "netplay rollback requires fresh frame zero"
        );
        let network = Network::spawn_transport(
            start.stream,
            start.player,
            admission.clone(),
            start.secret,
            start.scope,
            start.input_delay,
        )?;
        core::persistence(backend, Some(false))?;
        Ok(Self {
            network,
            player: start.player,
            timeline: Timeline::with_delay(start.player, start.input_delay),
            lease,
            restore_snapshot,
            snapshots: BTreeMap::from([(0, snapshot)]),
            outputs: BTreeMap::new(),
            committed: 0,
            responses: VecDeque::new(),
            hashes: Hashes::default(),
            pause: PauseControl::with_delay(start.input_delay),
            config: admission.config,
            checkpoint,
            persistent: admission.persistent,
            publication,
            verification: start.verify_every_frame,
            ready: false,
            admission_since: Instant::now(),
            stalled_since: None,
            heartbeat_at: Instant::now(),
            stats_at: Instant::now(),
            sampled: None,
            peer_progress: (0, 0),
            chat_sent: Default::default(),
            chat_received: Default::default(),
        })
    }

    pub(crate) fn step(&mut self, backend: &mut EmuBackend, buttons: u16) -> Result<()> {
        self.lease.validate_input(buttons)?;
        self.check_waiting()?;
        ensure!(self.ready, "netplay admission is pending");
        let rollback_frames = self.batch(backend, None)?;
        let frame = self.timeline.frame();
        let blocked_by_pause = self.pause.barrier().is_some_and(|barrier| frame >= barrier);
        let mut advanced = false;
        if !blocked_by_pause {
            let (scheduled, value) = self.timeline.sample_local(buttons)?;
            if self.sampled != Some(scheduled) {
                self.network.send(Message::Input {
                    player: self.player,
                    frame: scheduled,
                    buttons: value,
                })?;
                self.sampled = Some(scheduled);
            }
            if let Some(input) = self.timeline.next_frame()? {
                self.execute(backend, input)?;
                self.timeline.advance(input)?;
                advanced = true;
            }
        }
        if advanced || (blocked_by_pause && self.timeline.confirmed_frame() >= frame) {
            self.stalled_since = None;
        } else {
            self.stalled_since.get_or_insert_with(Instant::now);
        }
        self.commit()?;
        self.report_pause()?;
        self.send_progress(true)?;
        self.present(true, advanced || rollback_frames != 0, rollback_frames)?;
        self.check_waiting()
    }

    pub(crate) fn request_pause(&mut self, paused: bool) -> Result<()> {
        ensure!(self.ready, "netplay admission is pending");
        if let Some(message) = self.pause.request(self.timeline.frame(), paused)? {
            self.network.send(message)?;
        }
        Ok(())
    }

    pub(crate) fn send_chat(&mut self, text: String) -> Result<()> {
        ensure!(self.ready, "Connect before sending a message.");
        let text = text.trim().to_owned();
        zeff_netplay::wire::validate_chat(&text)?;
        self.chat_sent.take(Instant::now())?;
        self.network.send(Message::Chat { text: text.clone() })?;
        self.queue(Response::Chat { local: true, text })
    }

    pub(crate) fn poll(&mut self, backend: &mut EmuBackend) -> Result<Option<Response>> {
        self.check_waiting()?;
        if let Some(response) = self.responses.pop_front() {
            return Ok(Some(response));
        }
        let before = self.committed;
        let rollback_frames = self.batch(backend, None)?;
        if rollback_frames != 0 || self.committed != before {
            self.present(false, rollback_frames != 0, rollback_frames)?;
        }
        Ok(self.responses.pop_front())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn events(&self) -> &crossbeam_channel::Receiver<Event> {
        self.network.events()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn accept_event(
        &mut self,
        backend: &mut EmuBackend,
        event: Event,
    ) -> Result<Option<Response>> {
        self.check_waiting()?;
        let before = self.committed;
        let rollback_frames = self.batch(backend, Some(event))?;
        if rollback_frames != 0 || self.committed != before {
            self.present(false, rollback_frames != 0, rollback_frames)?;
        }
        Ok(self.responses.pop_front())
    }

    fn batch(&mut self, backend: &mut EmuBackend, first: Option<Event>) -> Result<u64> {
        let mut consumed = 0;
        if let Some(event) = first {
            self.handle_event(event)?;
            consumed = 1;
        }
        while consumed < EVENT_BUDGET {
            let Some(event) = self.network.poll()? else {
                break;
            };
            self.handle_event(event)?;
            consumed += 1;
        }
        let rollback_frames = self.correct(backend)?;
        if self.timeline.next_frame()?.is_some()
            || self
                .pause
                .barrier()
                .is_some_and(|barrier| self.timeline.confirmed_frame() >= barrier)
        {
            self.stalled_since = None;
        }
        self.commit()?;
        self.report_pause()?;
        self.send_progress(false)?;
        self.report_network_stats(Instant::now());
        Ok(rollback_frames)
    }

    fn report_network_stats(&mut self, now: Instant) {
        if self.ready
            && now.saturating_duration_since(self.stats_at) >= Duration::from_millis(500)
            && self.responses.is_empty()
        {
            self.responses
                .push_back(Response::NetworkStats(self.network.stats()));
            self.stats_at = now;
        }
    }

    fn handle_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Ready => {
                ensure!(!self.ready, "unexpected admission completion");
                self.ready = true;
                self.queue(Response::Ready)?;
            }
            Event::Failed(reason) => anyhow::bail!(reason),
            Event::Message(message) => {
                ensure!(self.ready, "netplay message before admission");
                match message {
                    Message::Chat { text } => {
                        if self.chat_received.take(Instant::now()).is_ok() {
                            self.queue(Response::Chat { local: false, text })?;
                        }
                    }
                    Message::Input {
                        player,
                        frame,
                        buttons,
                    } => {
                        ensure!(player != self.player, "remote input owns the local port");
                        self.lease.validate_input(buttons)?;
                        self.timeline.receive_remote(frame, buttons)?;
                    }
                    Message::Progress { frame, confirmed } => {
                        ensure!(confirmed <= frame, "invalid peer progress");
                        ensure!(
                            frame
                                <= self
                                    .timeline
                                    .frame()
                                    .checked_add(self.timeline.input_delay().lookahead())
                                    .context("frame overflow")?,
                            "peer progress exceeds lookahead"
                        );
                        ensure!(
                            frame >= self.peer_progress.0 && confirmed >= self.peer_progress.1,
                            "peer progress moved backwards"
                        );
                        self.peer_progress = (frame, confirmed);
                    }
                    Message::PauseChange {
                        request,
                        frame,
                        paused,
                    } => {
                        let ack =
                            self.pause
                                .observe(self.timeline.frame(), request, frame, paused)?;
                        self.network.send(ack)?;
                    }
                    Message::PauseAck { request } => {
                        if let Some(change) =
                            self.pause.acknowledge(self.timeline.frame(), request)?
                        {
                            self.network.send(change)?;
                        }
                    }
                    Message::Pause { .. } => {
                        anyhow::bail!("legacy pause message in rollback session")
                    }
                    checkpoint @ Message::Checkpoint { .. } => self.hashes.remote(
                        checkpoint,
                        self.timeline.frame(),
                        self.timeline.input_delay().lookahead(),
                    )?,
                    Message::Close { frame } => {
                        anyhow::bail!("netplay peer closed at frame {frame}")
                    }
                }
            }
        }
        Ok(())
    }

    fn check_waiting(&self) -> Result<()> {
        ensure!(
            self.ready || self.admission_since.elapsed() < STALL_LIMIT,
            "netplay admission stalled"
        );
        ensure!(
            self.stalled_since
                .is_none_or(|since| since.elapsed() < STALL_LIMIT),
            "netplay prediction stalled"
        );
        Ok(())
    }

    fn send_progress(&mut self, force: bool) -> Result<()> {
        if self.ready && (force || self.heartbeat_at.elapsed() >= HEARTBEAT) {
            self.network.send(Message::Progress {
                frame: self.timeline.frame(),
                confirmed: self.timeline.confirmed_frame(),
            })?;
            self.heartbeat_at = Instant::now();
        }
        Ok(())
    }

    fn report_pause(&mut self) -> Result<()> {
        if let Some((local, peer)) = self
            .pause
            .status(self.timeline.frame(), self.timeline.confirmed_frame())
        {
            self.queue(Response::Paused {
                frame: self.timeline.frame(),
                local,
                peer,
            })?;
        }
        Ok(())
    }

    pub(crate) fn restore(mut self, backend: &mut EmuBackend) -> Result<()> {
        self.network.cancel();
        self.lease
            .restore_checkpoint(backend, &self.restore_snapshot, self.checkpoint.clone())?;
        ensure!(
            backend.encode_state_bytes()? == self.checkpoint,
            "netplay restoration differs"
        );
        ensure!(
            identity::persistent_hash(backend)? == self.persistent,
            "netplay persistent restoration differs"
        );
        core::persistence(backend, Some(self.publication))?;
        Ok(())
    }
}

#[cfg(test)]
fn nes_mut(backend: &mut EmuBackend) -> Result<&mut crate::emu_backend::nes::NesBackend> {
    match backend {
        EmuBackend::Nes(nes) => Ok(nes),
        _ => anyhow::bail!("netplay lost NES backend"),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod input_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod pce_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod sega8_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod ws_tests;
