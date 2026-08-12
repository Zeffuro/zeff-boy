use std::collections::VecDeque;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use zeff_gb_core::emulator::Emulator as GameBoyEmulator;
use zeff_gb_core::hardware::bus::GameBoyLinkState;

use crate::emu_backend::EmuBackend;

use super::{LinkConnectionState, LinkPacketKind, LinkSession, LinkSessionError, LinkTransport};

const FLAG_PENDING_MASTER: u8 = 0x01;
const FLAG_EXTERNAL_CLOCK: u8 = 0x02;
const PAYLOAD_LEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GameBoyLinkPayloadError {
    WrongLength { expected: usize, actual: usize },
}

pub(crate) fn encode_game_boy_link_state(state: GameBoyLinkState) -> [u8; PAYLOAD_LEN] {
    let mut flags = 0;
    if state.pending_master_byte.is_some() {
        flags |= FLAG_PENDING_MASTER;
    }
    if state.external_clock_byte.is_some() {
        flags |= FLAG_EXTERNAL_CLOCK;
    }

    [
        flags,
        state.pending_master_byte.unwrap_or(0),
        state.external_clock_byte.unwrap_or(0),
        state.output_byte,
    ]
}

pub(crate) fn decode_game_boy_link_state(
    payload: &[u8],
) -> Result<GameBoyLinkState, GameBoyLinkPayloadError> {
    if payload.len() != PAYLOAD_LEN {
        return Err(GameBoyLinkPayloadError::WrongLength {
            expected: PAYLOAD_LEN,
            actual: payload.len(),
        });
    }

    Ok(GameBoyLinkState {
        pending_master_byte: (payload[0] & FLAG_PENDING_MASTER != 0).then_some(payload[1]),
        external_clock_byte: (payload[0] & FLAG_EXTERNAL_CLOCK != 0).then_some(payload[2]),
        output_byte: payload[3],
    })
}

pub(crate) struct GameBoyRemoteLink<T: LinkTransport> {
    session: LinkSession<T>,
    last_sent_state: Option<GameBoyLinkState>,
    last_sent_peer_sequence: u64,
    peer_states: VecDeque<PeerState>,
    peer_state_sequence: u64,
    force_send_current_state: bool,
    #[cfg(not(target_arch = "wasm32"))]
    trace: Option<LinkTrace>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PeerState {
    state: GameBoyLinkState,
    sequence: u64,
}

impl<T: LinkTransport> GameBoyRemoteLink<T> {
    pub(crate) fn new(session: LinkSession<T>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let trace = LinkTrace::from_env(session.endpoint());
        Self {
            session,
            last_sent_state: None,
            last_sent_peer_sequence: 0,
            peer_states: VecDeque::new(),
            peer_state_sequence: 0,
            force_send_current_state: false,
            #[cfg(not(target_arch = "wasm32"))]
            trace,
        }
    }

    pub(crate) fn state(&self) -> LinkConnectionState {
        self.session.state()
    }

    pub(crate) fn poll_backend(
        &mut self,
        backend: &mut EmuBackend,
    ) -> Result<(), LinkSessionError> {
        if self.state() == LinkConnectionState::Disconnected {
            backend.set_link_peer_present(false);
            return Err(LinkSessionError::Transport(
                super::LinkTransportError::Disconnected,
            ));
        }
        let EmuBackend::Gb(gb) = backend else {
            return Err(LinkSessionError::IncompatibleSystems);
        };
        self.poll_emulator(&mut gb.emu)
    }

    pub(crate) fn poll_emulator(
        &mut self,
        emulator: &mut GameBoyEmulator,
    ) -> Result<(), LinkSessionError> {
        if self.state() == LinkConnectionState::Disconnected {
            emulator.set_game_boy_link_peer_present(false);
            return Err(LinkSessionError::Transport(
                super::LinkTransportError::Disconnected,
            ));
        }
        emulator.set_game_boy_link_peer_present(true);
        self.drain_incoming()?;
        self.send_current_state_if_needed(emulator)?;
        self.resolve_emulator(emulator);
        self.send_current_state_if_needed(emulator)?;
        self.drain_incoming()?;
        self.send_current_state_if_needed(emulator)?;
        self.resolve_emulator(emulator);
        self.send_current_state_if_needed(emulator)?;

        Ok(())
    }

    pub(crate) fn disconnect(&mut self) {
        let _ = self.session.send(LinkPacketKind::Disconnect, &[]);
        self.session.disconnect();
    }

    fn drain_incoming(&mut self) -> Result<(), LinkSessionError> {
        loop {
            let Some(packet) = self.session.try_receive_packet()? else {
                return Ok(());
            };

            match packet.kind {
                LinkPacketKind::LinkState => {
                    let state = decode_game_boy_link_state(&packet.payload)
                        .map_err(|_| LinkSessionError::MalformedPacketPayload)?;
                    self.peer_state_sequence = self.peer_state_sequence.wrapping_add(1);
                    self.trace(format!(
                        "recv seq={} state={}",
                        self.peer_state_sequence,
                        format_state(state)
                    ));
                    self.peer_states.push_back(PeerState {
                        state,
                        sequence: self.peer_state_sequence,
                    });
                    if state.pending_master_byte.is_some() {
                        self.force_send_current_state = true;
                    }
                }
                LinkPacketKind::Disconnect => {
                    self.trace("recv disconnect".to_string());
                    self.session.disconnect();
                    return Err(LinkSessionError::Transport(
                        super::LinkTransportError::Disconnected,
                    ));
                }
                LinkPacketKind::Hello | LinkPacketKind::LinkEvent => {}
            }
        }
    }

    fn resolve_emulator(&mut self, emulator: &mut GameBoyEmulator) {
        while let Some(peer) = self.peer_states.front().copied() {
            let local_state = emulator.game_boy_link_state();
            if local_state.external_clock_byte.is_some()
                && let Some(peer_master_index) = self
                    .peer_states
                    .iter()
                    .position(|peer| peer.state.pending_master_byte.is_some())
            {
                for _ in 0..peer_master_index {
                    if let Some(dropped) = self.peer_states.pop_front() {
                        self.trace(format!(
                            "drop pre-master peer_seq={} peer={}",
                            dropped.sequence,
                            format_state(dropped.state)
                        ));
                    }
                }

                let Some(peer) = self.peer_states.pop_front() else {
                    return;
                };
                self.trace(format!(
                    "resolve local={} peer_seq={} peer={}",
                    format_state(local_state),
                    peer.sequence,
                    format_state(peer.state)
                ));
                if emulator.sync_game_boy_remote_link_peer_with_idle_response(peer.state, None) {
                    self.trace(format!(
                        "complete local={}",
                        format_state(emulator.game_boy_link_state())
                    ));
                    continue;
                }
                return;
            }

            if local_state.pending_master_byte.is_some()
                && peer.state.is_idle()
                && let Some(active_index) = self
                    .peer_states
                    .iter()
                    .position(|peer| !peer.state.is_idle())
            {
                for _ in 0..active_index {
                    if let Some(dropped) = self.peer_states.pop_front() {
                        self.trace(format!(
                            "drop pre-active peer_seq={} peer={}",
                            dropped.sequence,
                            format_state(dropped.state)
                        ));
                    }
                }
                continue;
            }

            if self.last_sent_state != Some(local_state) {
                return;
            }
            if peer.sequence < self.last_sent_peer_sequence {
                self.trace(format!(
                    "drop stale peer_seq={} peer={} last_sent_peer_seq={}",
                    peer.sequence,
                    format_state(peer.state),
                    self.last_sent_peer_sequence
                ));
                self.peer_states.pop_front();
                continue;
            }
            if peer.sequence == self.last_sent_peer_sequence && peer.state.is_idle() {
                self.trace(format!(
                    "drop stale idle peer_seq={} peer={} last_sent_peer_seq={}",
                    peer.sequence,
                    format_state(peer.state),
                    self.last_sent_peer_sequence
                ));
                self.peer_states.pop_front();
                continue;
            }
            if !can_resolve(local_state, peer.state) {
                if peer.state.is_idle() {
                    self.peer_states.pop_front();
                    continue;
                }
                return;
            }

            self.trace(format!(
                "resolve local={} peer_seq={} peer={}",
                format_state(local_state),
                peer.sequence,
                format_state(peer.state)
            ));
            if emulator.sync_game_boy_remote_link_peer_with_idle_response(peer.state, None) {
                self.trace(format!(
                    "complete local={}",
                    format_state(emulator.game_boy_link_state())
                ));
                self.peer_states.pop_front();
            } else {
                return;
            }
        }
    }

    fn send_current_state_if_needed(
        &mut self,
        emulator: &GameBoyEmulator,
    ) -> Result<(), LinkSessionError> {
        let state = emulator.game_boy_link_state();
        let changed = self.last_sent_state != Some(state);
        if !changed && !self.force_send_current_state {
            return Ok(());
        }

        if changed {
            self.last_sent_peer_sequence = self.peer_state_sequence;
        }
        self.last_sent_state = Some(state);
        self.force_send_current_state = false;
        self.trace(format!("send state={}", format_state(state)));
        self.session.send(
            LinkPacketKind::LinkState,
            &encode_game_boy_link_state(state),
        )?;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn trace(&mut self, message: String) {
        if let Some(trace) = &mut self.trace {
            trace.write(&message);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn trace(&mut self, _message: String) {}
}

fn can_resolve(local: GameBoyLinkState, peer: GameBoyLinkState) -> bool {
    if local.pending_master_byte.is_some() {
        return peer.pending_master_byte.is_some()
            || peer.external_clock_byte.is_some()
            || peer.is_idle();
    }

    local.external_clock_byte.is_some() && peer.pending_master_byte.is_some()
}

fn format_state(state: GameBoyLinkState) -> String {
    format!(
        "pm={:?} ext={:?} out={:02X}",
        state.pending_master_byte, state.external_clock_byte, state.output_byte
    )
}

#[cfg(not(target_arch = "wasm32"))]
struct LinkTrace {
    file: std::fs::File,
}

#[cfg(not(target_arch = "wasm32"))]
impl LinkTrace {
    fn from_env(endpoint: super::LinkEndpointId) -> Option<Self> {
        let raw = std::env::var("ZEFF_BOY_LINK_TRACE").ok()?;
        if raw.trim().is_empty() || raw.eq_ignore_ascii_case("0") {
            return None;
        }

        let path = trace_path(&raw, endpoint);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        Some(Self { file })
    }

    fn write(&mut self, message: &str) {
        let _ = writeln!(self.file, "{message}");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn trace_path(raw: &str, endpoint: super::LinkEndpointId) -> PathBuf {
    let pid = std::process::id();
    if raw == "1" {
        return PathBuf::from(".tmp").join(format!("zeff-boy-link-{pid}-e{}.log", endpoint.0));
    }

    let substituted = raw
        .replace("{pid}", &pid.to_string())
        .replace("{endpoint}", &endpoint.0.to_string());
    let path = PathBuf::from(substituted);
    if path.extension().is_some() {
        path
    } else {
        path.join(format!("zeff-boy-link-{pid}-e{}.log", endpoint.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use zeff_gb_core::hardware::types::constants::{SERIAL_SB, SERIAL_SC};
    use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

    use crate::link::transport::LocalLinkTransport;
    use crate::link::{LinkEndpointId, LinkSystemType};

    #[test]
    fn game_boy_link_state_payload_roundtrips_active_fields() {
        let state = GameBoyLinkState {
            pending_master_byte: Some(0xAB),
            external_clock_byte: Some(0x34),
            output_byte: 0x56,
        };

        assert_eq!(
            decode_game_boy_link_state(&encode_game_boy_link_state(state)).unwrap(),
            state
        );
    }

    #[test]
    fn game_boy_link_state_payload_roundtrips_idle_state() {
        let state = GameBoyLinkState::default();

        assert_eq!(
            decode_game_boy_link_state(&encode_game_boy_link_state(state)).unwrap(),
            state
        );
    }

    #[test]
    fn game_boy_link_resolution_allows_idle_response_for_local_master() {
        assert!(can_resolve(
            GameBoyLinkState {
                pending_master_byte: Some(0xAB),
                external_clock_byte: None,
                output_byte: 0xAB,
            },
            GameBoyLinkState::default(),
        ));
        assert!(can_resolve(
            GameBoyLinkState {
                pending_master_byte: Some(0xAB),
                external_clock_byte: None,
                output_byte: 0xAB,
            },
            GameBoyLinkState {
                pending_master_byte: None,
                external_clock_byte: Some(0x34),
                output_byte: 0x34,
            },
        ));
    }

    #[test]
    fn game_boy_remote_link_uses_connected_idle_peer_output_after_local_state_was_sent() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
            left_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        ));
        let mut right_session =
            LinkSession::new(right_transport, LinkSystemType::GameBoy, LinkEndpointId(2));
        let mut backend = gb_backend();

        right_session
            .send(
                LinkPacketKind::LinkState,
                &encode_game_boy_link_state(GameBoyLinkState {
                    pending_master_byte: None,
                    external_clock_byte: None,
                    output_byte: 0xCD,
                }),
            )
            .unwrap();
        arm_internal_clock_transfer(&mut backend, 0xAB);

        left_link.poll_backend(&mut backend).unwrap();
        assert_eq!(game_boy_serial_registers(&backend), (0xAB, 0x80));

        let packet = right_session.try_receive_packet().unwrap().unwrap();
        assert_eq!(packet.kind, LinkPacketKind::LinkState);
        assert_eq!(
            decode_game_boy_link_state(&packet.payload).unwrap(),
            GameBoyLinkState {
                pending_master_byte: Some(0xAB),
                external_clock_byte: None,
                output_byte: 0xAB,
            }
        );

        right_session
            .send(
                LinkPacketKind::LinkState,
                &encode_game_boy_link_state(GameBoyLinkState {
                    pending_master_byte: None,
                    external_clock_byte: None,
                    output_byte: 0xCD,
                }),
            )
            .unwrap();

        left_link.poll_backend(&mut backend).unwrap();
        assert_eq!(game_boy_serial_registers(&backend), (0xCD, 0x00));
    }

    #[test]
    fn game_boy_remote_link_prefers_queued_active_peer_state_over_earlier_idle_for_local_master() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
            left_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        ));
        let mut right_session =
            LinkSession::new(right_transport, LinkSystemType::GameBoy, LinkEndpointId(2));
        let mut backend = gb_backend();

        right_session
            .send(
                LinkPacketKind::LinkState,
                &encode_game_boy_link_state(GameBoyLinkState {
                    pending_master_byte: None,
                    external_clock_byte: None,
                    output_byte: 0xCD,
                }),
            )
            .unwrap();
        right_session
            .send(
                LinkPacketKind::LinkState,
                &encode_game_boy_link_state(GameBoyLinkState {
                    pending_master_byte: None,
                    external_clock_byte: Some(0x34),
                    output_byte: 0x34,
                }),
            )
            .unwrap();
        arm_internal_clock_transfer(&mut backend, 0xAB);

        left_link.poll_backend(&mut backend).unwrap();

        assert_eq!(game_boy_serial_registers(&backend), (0x34, 0x00));
    }

    #[test]
    fn game_boy_remote_link_keeps_early_external_peer_state_until_local_master_starts() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
            left_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        ));
        let mut right_session =
            LinkSession::new(right_transport, LinkSystemType::GameBoy, LinkEndpointId(2));
        let mut backend = gb_backend();

        right_session
            .send(
                LinkPacketKind::LinkState,
                &encode_game_boy_link_state(GameBoyLinkState {
                    pending_master_byte: None,
                    external_clock_byte: Some(0x34),
                    output_byte: 0x34,
                }),
            )
            .unwrap();

        left_link.poll_backend(&mut backend).unwrap();
        assert_eq!(game_boy_serial_registers(&backend), (0x00, 0x00));

        arm_internal_clock_transfer(&mut backend, 0xAB);
        left_link.poll_backend(&mut backend).unwrap();

        assert_eq!(game_boy_serial_registers(&backend), (0x34, 0x00));
    }

    #[test]
    fn game_boy_remote_link_does_not_echo_unchanged_external_peer_state() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
            left_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        ));
        let mut right_session =
            LinkSession::new(right_transport, LinkSystemType::GameBoy, LinkEndpointId(2));
        let mut backend = gb_backend();

        arm_external_clock_transfer(&mut backend, 0x02);
        left_link.poll_backend(&mut backend).unwrap();
        while right_session.try_receive_packet().unwrap().is_some() {}

        right_session
            .send(
                LinkPacketKind::LinkState,
                &encode_game_boy_link_state(GameBoyLinkState {
                    pending_master_byte: None,
                    external_clock_byte: Some(0x02),
                    output_byte: 0x02,
                }),
            )
            .unwrap();

        left_link.poll_backend(&mut backend).unwrap();

        assert_eq!(game_boy_serial_registers(&backend), (0x02, 0x80));
        assert!(
            right_session.try_receive_packet().unwrap().is_none(),
            "peer external-clock readiness must not force an unchanged local-state echo"
        );
    }

    #[test]
    fn game_boy_remote_link_resolves_queued_peer_master_for_local_external_clock() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
            left_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        ));
        let mut right_session =
            LinkSession::new(right_transport, LinkSystemType::GameBoy, LinkEndpointId(2));
        let mut backend = gb_backend();

        right_session
            .send(
                LinkPacketKind::LinkState,
                &encode_game_boy_link_state(GameBoyLinkState {
                    pending_master_byte: Some(0xAB),
                    external_clock_byte: None,
                    output_byte: 0xAB,
                }),
            )
            .unwrap();
        right_session
            .send(
                LinkPacketKind::LinkState,
                &encode_game_boy_link_state(GameBoyLinkState {
                    pending_master_byte: None,
                    external_clock_byte: None,
                    output_byte: 0x00,
                }),
            )
            .unwrap();
        arm_external_clock_transfer(&mut backend, 0x34);

        left_link.poll_backend(&mut backend).unwrap();

        assert_eq!(game_boy_serial_registers(&backend), (0xAB, 0x00));
    }

    #[test]
    fn game_boy_remote_link_drops_superseded_active_peer_state_before_local_master() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
            left_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        ));
        let mut right_session =
            LinkSession::new(right_transport, LinkSystemType::GameBoy, LinkEndpointId(2));
        let mut backend = gb_backend();

        right_session
            .send(
                LinkPacketKind::LinkState,
                &encode_game_boy_link_state(GameBoyLinkState {
                    pending_master_byte: Some(0x75),
                    external_clock_byte: None,
                    output_byte: 0x75,
                }),
            )
            .unwrap();
        right_session
            .send(
                LinkPacketKind::LinkState,
                &encode_game_boy_link_state(GameBoyLinkState {
                    pending_master_byte: None,
                    external_clock_byte: None,
                    output_byte: 0x00,
                }),
            )
            .unwrap();

        left_link.poll_backend(&mut backend).unwrap();
        assert_eq!(game_boy_serial_registers(&backend), (0x00, 0x00));

        arm_internal_clock_transfer(&mut backend, 0x00);
        left_link.poll_backend(&mut backend).unwrap();

        assert_eq!(game_boy_serial_registers(&backend), (0x00, 0x80));

        let mut saw_local_master = false;
        while let Some(packet) = right_session.try_receive_packet().unwrap() {
            assert_eq!(packet.kind, LinkPacketKind::LinkState);
            if decode_game_boy_link_state(&packet.payload).unwrap()
                == (GameBoyLinkState {
                    pending_master_byte: Some(0x00),
                    external_clock_byte: None,
                    output_byte: 0x00,
                })
            {
                saw_local_master = true;
                break;
            }
        }
        assert!(saw_local_master);

        right_session
            .send(
                LinkPacketKind::LinkState,
                &encode_game_boy_link_state(GameBoyLinkState {
                    pending_master_byte: None,
                    external_clock_byte: None,
                    output_byte: 0x00,
                }),
            )
            .unwrap();

        left_link.poll_backend(&mut backend).unwrap();
        assert_eq!(game_boy_serial_registers(&backend), (0x00, 0x00));
    }

    fn gb_backend() -> EmuBackend {
        let rom = vec![0u8; 0x8000];
        let gb =
            zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
                .expect("GB emulator should initialize");
        EmuBackend::from_gb(gb, PathBuf::from("test.gb"))
    }

    fn arm_internal_clock_transfer(backend: &mut EmuBackend, byte: u8) {
        backend.set_link_peer_present(true);
        let EmuBackend::Gb(gb) = backend else {
            panic!("expected GB backend");
        };
        gb.emu.write_byte(SERIAL_SB, byte);
        gb.emu.write_byte(SERIAL_SC, 0x81);
        gb.emu.step_frame();
    }

    fn arm_external_clock_transfer(backend: &mut EmuBackend, byte: u8) {
        backend.set_link_peer_present(true);
        let EmuBackend::Gb(gb) = backend else {
            panic!("expected GB backend");
        };
        gb.emu.write_byte(SERIAL_SB, byte);
        gb.emu.write_byte(SERIAL_SC, 0x80);
    }

    fn game_boy_serial_registers(backend: &EmuBackend) -> (u8, u8) {
        let EmuBackend::Gb(gb) = backend else {
            panic!("expected GB backend");
        };
        (
            gb.emu.cpu_peek8(SERIAL_SB),
            gb.emu.cpu_peek8(SERIAL_SC) & 0x80,
        )
    }
}
