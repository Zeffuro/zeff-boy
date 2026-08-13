#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use zeff_gb_core::emulator::Emulator as GameBoyEmulator;
use zeff_gb_core::hardware::bus::{GameBoyLinkAction, GameBoyLinkReply};

use crate::emu_backend::EmuBackend;

use super::{LinkConnectionState, LinkPacketKind, LinkSession, LinkSessionError, LinkTransport};

const MASTER_REPLY_SPIN_LIMIT: usize = 64;
const EVENT_MASTER_START: u8 = 1;
const EVENT_TRANSFER_REPLY: u8 = 2;

const MASTER_START_PAYLOAD_LEN: usize = 1 + 8 + 8 + 8 + 1 + 8;
const TRANSFER_REPLY_PAYLOAD_LEN: usize = 1 + 8 + 8 + 1 + 1 + 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GbTransferId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GameBoyLinkPayloadError {
    WrongLength { expected: usize, actual: usize },
    UnknownEvent(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameBoyLinkEvent {
    MasterStart {
        transfer_id: GbTransferId,
        start_tick: u64,
        action: GameBoyLinkAction,
    },
    TransferReply {
        transfer_id: GbTransferId,
        sample_tick: u64,
        reply: GameBoyLinkReply,
    },
}

pub(crate) struct GameBoyRemoteLink<T: LinkTransport> {
    session: LinkSession<T>,
    next_transfer_id: u64,
    pending_master_transfer: Option<GbTransferId>,
    #[cfg(not(target_arch = "wasm32"))]
    trace: Option<LinkTrace>,
}

impl<T: LinkTransport> GameBoyRemoteLink<T> {
    pub(crate) fn new(session: LinkSession<T>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let trace = LinkTrace::from_env(session.endpoint());
        Self {
            session,
            next_transfer_id: 0,
            pending_master_transfer: None,
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
        self.drain_incoming(emulator)?;
        self.send_pending_master_start(emulator)?;
        self.poll_for_pending_master_reply(emulator)?;
        self.drain_incoming(emulator)?;

        Ok(())
    }

    pub(crate) fn disconnect(&mut self) {
        let _ = self.session.send(LinkPacketKind::Disconnect, &[]);
        self.session.disconnect();
    }

    fn drain_incoming(&mut self, emulator: &mut GameBoyEmulator) -> Result<(), LinkSessionError> {
        loop {
            let Some(packet) = self.session.try_receive_packet()? else {
                return Ok(());
            };

            match packet.kind {
                LinkPacketKind::LinkEvent => {
                    let event = decode_game_boy_link_event(&packet.payload)
                        .map_err(|_| LinkSessionError::MalformedPacketPayload)?;
                    self.handle_event(emulator, event)?;
                }
                LinkPacketKind::Disconnect => {
                    self.trace("recv disconnect".to_string());
                    self.session.disconnect();
                    return Err(LinkSessionError::Transport(
                        super::LinkTransportError::Disconnected,
                    ));
                }
                LinkPacketKind::Hello | LinkPacketKind::LinkState => {}
            }
        }
    }

    fn handle_event(
        &mut self,
        emulator: &mut GameBoyEmulator,
        event: GameBoyLinkEvent,
    ) -> Result<(), LinkSessionError> {
        match event {
            GameBoyLinkEvent::MasterStart {
                transfer_id,
                start_tick,
                action,
            } => {
                let reply = emulator.game_boy_link_reply_to_master_start();
                self.trace(format!(
                    "recv master_start id={} tick={} out={:02X} period={} gen={} reply={}",
                    transfer_id.0,
                    start_tick,
                    action.out_byte,
                    action.clock_period_t_cycles,
                    action.serial_generation,
                    format_reply(reply)
                ));
                self.send_event(GameBoyLinkEvent::TransferReply {
                    transfer_id,
                    sample_tick: emulator.cpu_cycles(),
                    reply,
                })?;
                if reply.passive {
                    if emulator.complete_game_boy_external_link_transfer(action.out_byte) {
                        self.trace(format!(
                            "complete passive id={} in={:02X}",
                            transfer_id.0, action.out_byte
                        ));
                    } else {
                        self.trace(format!(
                            "passive completion missed id={} in={:02X}",
                            transfer_id.0, action.out_byte
                        ));
                    }
                }
            }
            GameBoyLinkEvent::TransferReply {
                transfer_id,
                sample_tick,
                reply,
            } => {
                self.trace(format!(
                    "recv reply id={} tick={} {}",
                    transfer_id.0,
                    sample_tick,
                    format_reply(reply)
                ));
                if self.pending_master_transfer != Some(transfer_id) {
                    self.trace(format!(
                        "protocol fault unexpected_reply id={} pending={:?}",
                        transfer_id.0,
                        self.pending_master_transfer.map(|id| id.0)
                    ));
                    return Err(LinkSessionError::MalformedPacketPayload);
                }
                if emulator.apply_game_boy_link_reply(reply) {
                    self.pending_master_transfer = None;
                    self.trace(format!(
                        "bound reply id={} in={:02X} passive={}",
                        transfer_id.0, reply.out_byte, reply.passive
                    ));
                } else {
                    self.trace(format!("reply rejected id={}", transfer_id.0));
                    return Err(LinkSessionError::MalformedPacketPayload);
                }
            }
        }

        Ok(())
    }

    fn send_pending_master_start(
        &mut self,
        emulator: &mut GameBoyEmulator,
    ) -> Result<(), LinkSessionError> {
        let Some(action) = emulator.take_game_boy_link_action() else {
            return Ok(());
        };

        if self.pending_master_transfer.is_some() {
            self.trace(format!(
                "protocol fault local_master_while_pending pending={:?} out={:02X}",
                self.pending_master_transfer.map(|id| id.0),
                action.out_byte
            ));
            return Err(LinkSessionError::MalformedPacketPayload);
        }

        let transfer_id = self.allocate_transfer_id();
        self.pending_master_transfer = Some(transfer_id);
        self.trace(format!(
            "send master_start id={} tick={} out={:02X} period={} gen={}",
            transfer_id.0,
            emulator.cpu_cycles(),
            action.out_byte,
            action.clock_period_t_cycles,
            action.serial_generation
        ));
        self.send_event(GameBoyLinkEvent::MasterStart {
            transfer_id,
            start_tick: emulator.cpu_cycles(),
            action,
        })
    }

    fn poll_for_pending_master_reply(
        &mut self,
        emulator: &mut GameBoyEmulator,
    ) -> Result<(), LinkSessionError> {
        for _ in 0..MASTER_REPLY_SPIN_LIMIT {
            if self.pending_master_transfer.is_none() {
                return Ok(());
            }
            if let Some(packet) = self.session.try_receive_packet()? {
                match packet.kind {
                    LinkPacketKind::LinkEvent => {
                        let event = decode_game_boy_link_event(&packet.payload)
                            .map_err(|_| LinkSessionError::MalformedPacketPayload)?;
                        self.handle_event(emulator, event)?;
                    }
                    LinkPacketKind::Disconnect => {
                        self.trace("recv disconnect".to_string());
                        self.session.disconnect();
                        return Err(LinkSessionError::Transport(
                            super::LinkTransportError::Disconnected,
                        ));
                    }
                    LinkPacketKind::Hello | LinkPacketKind::LinkState => {}
                }
            } else {
                #[cfg(not(target_arch = "wasm32"))]
                std::thread::yield_now();
            }
        }

        Ok(())
    }

    fn send_event(&mut self, event: GameBoyLinkEvent) -> Result<(), LinkSessionError> {
        self.session.send(
            LinkPacketKind::LinkEvent,
            &encode_game_boy_link_event(event),
        )?;
        Ok(())
    }

    fn allocate_transfer_id(&mut self) -> GbTransferId {
        let endpoint = u64::from(self.session.endpoint().0) << 56;
        let counter = self.next_transfer_id & 0x00FF_FFFF_FFFF_FFFF;
        self.next_transfer_id = self.next_transfer_id.wrapping_add(1);
        GbTransferId(endpoint | counter)
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

fn encode_game_boy_link_event(event: GameBoyLinkEvent) -> Vec<u8> {
    match event {
        GameBoyLinkEvent::MasterStart {
            transfer_id,
            start_tick,
            action,
        } => {
            let mut out = Vec::with_capacity(MASTER_START_PAYLOAD_LEN);
            out.push(EVENT_MASTER_START);
            out.extend_from_slice(&transfer_id.0.to_le_bytes());
            out.extend_from_slice(&start_tick.to_le_bytes());
            out.extend_from_slice(&action.clock_period_t_cycles.to_le_bytes());
            out.push(action.out_byte);
            out.extend_from_slice(&action.serial_generation.to_le_bytes());
            out
        }
        GameBoyLinkEvent::TransferReply {
            transfer_id,
            sample_tick,
            reply,
        } => {
            let mut out = Vec::with_capacity(TRANSFER_REPLY_PAYLOAD_LEN);
            out.push(EVENT_TRANSFER_REPLY);
            out.extend_from_slice(&transfer_id.0.to_le_bytes());
            out.extend_from_slice(&sample_tick.to_le_bytes());
            out.push(reply.out_byte);
            out.push(u8::from(reply.passive));
            out.extend_from_slice(&reply.serial_generation.to_le_bytes());
            out
        }
    }
}

fn decode_game_boy_link_event(payload: &[u8]) -> Result<GameBoyLinkEvent, GameBoyLinkPayloadError> {
    let Some(kind) = payload.first().copied() else {
        return Err(GameBoyLinkPayloadError::WrongLength {
            expected: 1,
            actual: 0,
        });
    };

    match kind {
        EVENT_MASTER_START => {
            if payload.len() != MASTER_START_PAYLOAD_LEN {
                return Err(GameBoyLinkPayloadError::WrongLength {
                    expected: MASTER_START_PAYLOAD_LEN,
                    actual: payload.len(),
                });
            }
            let transfer_id = GbTransferId(read_u64(payload, 1));
            Ok(GameBoyLinkEvent::MasterStart {
                transfer_id,
                start_tick: read_u64(payload, 9),
                action: GameBoyLinkAction {
                    clock_period_t_cycles: read_u64(payload, 17),
                    out_byte: payload[25],
                    serial_generation: read_u64(payload, 26),
                },
            })
        }
        EVENT_TRANSFER_REPLY => {
            if payload.len() != TRANSFER_REPLY_PAYLOAD_LEN {
                return Err(GameBoyLinkPayloadError::WrongLength {
                    expected: TRANSFER_REPLY_PAYLOAD_LEN,
                    actual: payload.len(),
                });
            }
            Ok(GameBoyLinkEvent::TransferReply {
                transfer_id: GbTransferId(read_u64(payload, 1)),
                sample_tick: read_u64(payload, 9),
                reply: GameBoyLinkReply {
                    out_byte: payload[17],
                    passive: payload[18] != 0,
                    serial_generation: read_u64(payload, 19),
                },
            })
        }
        other => Err(GameBoyLinkPayloadError::UnknownEvent(other)),
    }
}

fn read_u64(payload: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        payload[offset..offset + 8]
            .try_into()
            .expect("payload length checked before u64 read"),
    )
}

fn format_reply(reply: GameBoyLinkReply) -> String {
    format!(
        "out={:02X} passive={} gen={}",
        reply.out_byte, reply.passive, reply.serial_generation
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
    use zeff_gb_core::hardware::types::constants::{INTERRUPT_IF, SERIAL_SB, SERIAL_SC};
    use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

    use crate::link::transport::LocalLinkTransport;
    use crate::link::{LinkEndpointId, LinkSystemType};

    #[test]
    fn game_boy_link_event_payload_roundtrips_master_start() {
        let event = GameBoyLinkEvent::MasterStart {
            transfer_id: GbTransferId(0x0100_0000_0000_0007),
            start_tick: 123,
            action: GameBoyLinkAction {
                out_byte: 0xAB,
                clock_period_t_cycles: 4096,
                serial_generation: 42,
            },
        };

        assert_eq!(
            decode_game_boy_link_event(&encode_game_boy_link_event(event)),
            Ok(event)
        );
    }

    #[test]
    fn game_boy_link_event_payload_roundtrips_transfer_reply() {
        let event = GameBoyLinkEvent::TransferReply {
            transfer_id: GbTransferId(0x0200_0000_0000_0009),
            sample_tick: 456,
            reply: GameBoyLinkReply {
                out_byte: 0x34,
                passive: true,
                serial_generation: 77,
            },
        };

        assert_eq!(
            decode_game_boy_link_event(&encode_game_boy_link_event(event)),
            Ok(event)
        );
    }

    #[test]
    fn game_boy_remote_link_binds_reply_to_exact_transfer_id() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
            left_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        ));
        let mut right_link = GameBoyRemoteLink::new(LinkSession::new(
            right_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(2),
        ));
        let mut left = gb_emulator();
        let mut right = gb_emulator();

        left.set_game_boy_link_peer_present(true);
        right.set_game_boy_link_peer_present(true);
        left.write_byte(SERIAL_SB, 0xAB);
        right.write_byte(SERIAL_SB, 0x34);
        left.write_byte(SERIAL_SC, 0x81);
        right.write_byte(SERIAL_SC, 0x80);

        left_link.poll_emulator(&mut left).unwrap();
        right_link.poll_emulator(&mut right).unwrap();
        left_link.poll_emulator(&mut left).unwrap();

        assert_eq!(right.cpu_peek8(SERIAL_SB), 0xAB);
        assert_eq!(right.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(right.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
        assert_eq!(left.cpu_peek8(SERIAL_SB), 0xAB);
        assert_eq!(left.cpu_peek8(SERIAL_SC) & 0x80, 0x80);

        left.step_frame();
        assert_eq!(left.cpu_peek8(SERIAL_SB), 0x34);
        assert_eq!(left.cpu_peek8(SERIAL_SC) & 0x80, 0);
        assert_eq!(left.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
    }

    #[test]
    fn game_boy_remote_link_rejects_unmatched_reply() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
            left_transport,
            LinkSystemType::GameBoy,
            LinkEndpointId(1),
        ));
        let mut right_session =
            LinkSession::new(right_transport, LinkSystemType::GameBoy, LinkEndpointId(2));
        let mut left = gb_emulator();

        right_session
            .send(
                LinkPacketKind::LinkEvent,
                &encode_game_boy_link_event(GameBoyLinkEvent::TransferReply {
                    transfer_id: GbTransferId(0x0200_0000_0000_0001),
                    sample_tick: 0,
                    reply: GameBoyLinkReply {
                        out_byte: 0x34,
                        passive: true,
                        serial_generation: 0,
                    },
                }),
            )
            .unwrap();

        assert_eq!(
            left_link.poll_emulator(&mut left),
            Err(LinkSessionError::MalformedPacketPayload)
        );
    }

    fn gb_emulator() -> GameBoyEmulator {
        let rom = vec![0u8; 0x8000];
        GameBoyEmulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .expect("GB emulator should initialize")
    }

    #[allow(dead_code)]
    fn gb_backend() -> EmuBackend {
        EmuBackend::from_gb(gb_emulator(), PathBuf::from("test.gb"))
    }
}
