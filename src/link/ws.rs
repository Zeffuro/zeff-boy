use std::collections::VecDeque;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use zeff_ws_core::emulator::Emulator as WonderSwanEmulator;
use zeff_ws_core::hardware::bus::{DebugTraceEvent, UartDebugSnapshot, WonderSwanTxEvent};
use zeff_ws_core::hardware::cpu::FetchedInstruction;

use super::{LinkConnectionState, LinkPacketKind, LinkSession, LinkSessionError, LinkTransport};

const EVENT_TX_BYTE: u8 = 1;
const TX_BYTE_PAYLOAD_LEN: usize = 1 + 8 + 8 + 4 + 1;
const WATERMARK_PAYLOAD_LEN: usize = 8;
const WS_CYCLES_PER_FRAME: u64 = zeff_ws_core::hardware::constants::CYCLES_PER_FRAME as u64;
const WATERMARK_INTERVAL_CYCLES: u64 = WS_CYCLES_PER_FRAME / 2;
const MAX_REMOTE_LEAD_CYCLES: u64 = WS_CYCLES_PER_FRAME * 2;
const SLOW_UART_BYTE_CYCLES: u64 = 3_200;
const REMOTE_RX_DELAY_CYCLES: u64 = SLOW_UART_BYTE_CYCLES;
const MAX_RX_DELIVERIES_PER_POLL: usize = 1;
const SERIAL_STATUS_RX_READY: u8 = 0x01;
const SERIAL_STATUS_OVERRUN: u8 = 0x02;
const SERIAL_STATUS_FAST_BAUD: u8 = 0x40;
const SERIAL_STATUS_ENABLE: u8 = 0x80;
const SERIAL_DATA_PORT: u16 = 0x00B1;
const IRQ_ENABLE_PORT: u16 = 0x00B2;
const SERIAL_CONTROL_PORT: u16 = 0x00B3;
const IRQ_STATUS_PORT: u16 = 0x00B4;
const IRQ_ACK_PORT: u16 = 0x00B6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WonderSwanLinkPayloadError {
    WrongLength { expected: usize, actual: usize },
    UnknownEvent(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WonderSwanLinkEvent {
    completed_cycle: u64,
    generation: u64,
    baud_bps: u32,
    byte: u8,
}

pub(crate) struct WonderSwanRemoteLink<T: LinkTransport> {
    session: LinkSession<T>,
    local_epoch_cycle: Option<u64>,
    last_sent_watermark: Option<u64>,
    remote_watermark: Option<u64>,
    inbound_events: VecDeque<WonderSwanLinkEvent>,
    // Preserve UART byte spacing even if TCP packets arrive back-to-back.
    next_rx_delivery_cycle: Option<u64>,
    last_uart_trace_state: Option<UartTraceState>,
    #[cfg(not(target_arch = "wasm32"))]
    trace: Option<LinkTrace>,
}

impl<T: LinkTransport> WonderSwanRemoteLink<T> {
    pub(crate) fn new(session: LinkSession<T>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let trace = LinkTrace::from_env(session.endpoint());
        Self {
            session,
            local_epoch_cycle: None,
            last_sent_watermark: None,
            remote_watermark: None,
            inbound_events: VecDeque::new(),
            next_rx_delivery_cycle: None,
            last_uart_trace_state: None,
            #[cfg(not(target_arch = "wasm32"))]
            trace,
        }
    }

    pub(crate) fn state(&self) -> LinkConnectionState {
        self.session.state()
    }

    pub(crate) fn trace_enabled(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.trace.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    pub(crate) fn trace_serial_io_events(
        &mut self,
        emulator: &WonderSwanEmulator,
        fetched: Option<FetchedInstruction>,
        events: &[DebugTraceEvent],
    ) {
        if !self.trace_enabled() {
            return;
        }

        for event in events.iter().copied() {
            if let Some(line) = format_serial_io_trace_line(
                emulator,
                self.local_session_cycle(emulator),
                fetched,
                event,
            ) {
                self.trace(line);
            }
        }
    }

    pub(crate) fn poll_emulator(
        &mut self,
        emulator: &mut WonderSwanEmulator,
    ) -> Result<(), LinkSessionError> {
        if self.state() == LinkConnectionState::Disconnected {
            return Err(LinkSessionError::Transport(
                super::LinkTransportError::Disconnected,
            ));
        }

        self.initialize_epoch(emulator);
        self.trace_uart_state(emulator, "poll_start");
        self.drain_incoming(emulator)?;
        self.deliver_due_events(emulator);
        self.send_completed_tx_events(emulator)?;
        self.send_watermark(emulator)?;
        self.drain_incoming(emulator)?;
        self.deliver_due_events(emulator);
        self.trace_uart_state(emulator, "poll_end");
        Ok(())
    }

    pub(crate) fn can_advance(&self, emulator: &WonderSwanEmulator) -> bool {
        let local_cycle = self.local_session_cycle(emulator);
        let allowed_cycle = self
            .remote_watermark
            .unwrap_or(0)
            .saturating_add(MAX_REMOTE_LEAD_CYCLES);
        local_cycle <= allowed_cycle
    }

    pub(crate) fn disconnect(&mut self) {
        let _ = self.session.send(LinkPacketKind::Disconnect, &[]);
        self.session.disconnect();
    }

    fn drain_incoming(
        &mut self,
        _emulator: &mut WonderSwanEmulator,
    ) -> Result<(), LinkSessionError> {
        loop {
            let Some(packet) = self.session.try_receive_packet()? else {
                return Ok(());
            };

            match packet.kind {
                LinkPacketKind::LinkEvent => {
                    let event = decode_wonder_swan_link_event(&packet.payload)
                        .map_err(|_| LinkSessionError::MalformedPacketPayload)?;
                    self.trace(format!(
                        "recv tx-event session_cycle={} gen={} baud={} byte={:02X} queue_before={}",
                        event.completed_cycle,
                        event.generation,
                        event.baud_bps,
                        event.byte,
                        self.inbound_events.len()
                    ));
                    self.inbound_events.push_back(event);
                }
                LinkPacketKind::Disconnect => {
                    self.trace("recv disconnect".to_string());
                    self.session.disconnect();
                    return Err(LinkSessionError::Transport(
                        super::LinkTransportError::Disconnected,
                    ));
                }
                LinkPacketKind::LinkState => {
                    let watermark = decode_watermark(&packet.payload)
                        .map_err(|_| LinkSessionError::MalformedPacketPayload)?;
                    self.remote_watermark = Some(
                        self.remote_watermark
                            .map_or(watermark, |current| current.max(watermark)),
                    );
                }
                LinkPacketKind::Hello => {}
            }
        }
    }

    fn initialize_epoch(&mut self, emulator: &mut WonderSwanEmulator) {
        if self.local_epoch_cycle.is_some() {
            return;
        }

        let epoch = emulator.cpu_cycles();
        self.local_epoch_cycle = Some(epoch);

        let mut stale_events = 0usize;
        while emulator.take_wonder_swan_link_tx_event().is_some() {
            stale_events += 1;
        }
        self.trace(format!(
            "epoch local_cycle={} stale_completed_tx_dropped={} remote_rx_delay_cycles={}",
            epoch, stale_events, REMOTE_RX_DELAY_CYCLES
        ));
    }

    fn send_watermark(&mut self, emulator: &WonderSwanEmulator) -> Result<(), LinkSessionError> {
        let session_cycle = self.local_session_cycle(emulator);
        let should_send = self.last_sent_watermark.is_none_or(|last| {
            session_cycle == 0 || session_cycle.saturating_sub(last) >= WATERMARK_INTERVAL_CYCLES
        });
        if !should_send {
            return Ok(());
        }

        self.session
            .send(LinkPacketKind::LinkState, &encode_watermark(session_cycle))?;
        self.last_sent_watermark = Some(session_cycle);
        Ok(())
    }

    fn send_completed_tx_events(
        &mut self,
        emulator: &mut WonderSwanEmulator,
    ) -> Result<(), LinkSessionError> {
        while let Some(event) = emulator.take_wonder_swan_link_tx_event() {
            self.send_event(event)?;
        }
        Ok(())
    }

    fn send_event(&mut self, event: WonderSwanTxEvent) -> Result<(), LinkSessionError> {
        let Some(epoch) = self.local_epoch_cycle else {
            return Ok(());
        };

        if event.completed_cycle <= epoch {
            return Ok(());
        }

        self.session.send(
            LinkPacketKind::LinkEvent,
            &encode_wonder_swan_link_event(WonderSwanLinkEvent {
                completed_cycle: event.completed_cycle - epoch,
                generation: event.generation,
                baud_bps: event.baud_bps,
                byte: event.byte,
            }),
        )?;
        self.trace(format!(
            "send tx-event local_cycle={} session_cycle={} gen={} baud={} byte={:02X}",
            event.completed_cycle,
            event.completed_cycle - epoch,
            event.generation,
            event.baud_bps,
            event.byte
        ));
        Ok(())
    }

    fn deliver_due_events(&mut self, emulator: &mut WonderSwanEmulator) {
        let local_cycle = self.local_session_cycle(emulator);
        let mut delivered = 0usize;
        while self
            .inbound_events
            .front()
            .is_some_and(|event| self.scheduled_delivery_cycle(*event) <= local_cycle)
        {
            if delivered >= MAX_RX_DELIVERIES_PER_POLL {
                self.trace(format!(
                    "defer due-rx local_session_cycle={} pending_due={} queue_len={}",
                    local_cycle,
                    self.inbound_events
                        .iter()
                        .take_while(|event| self.scheduled_delivery_cycle(**event) <= local_cycle)
                        .count(),
                    self.inbound_events.len()
                ));
                self.next_rx_delivery_cycle = Some(
                    local_cycle
                        .saturating_add(byte_cycles(self.inbound_events.front().unwrap().baud_bps)),
                );
                break;
            }
            let event = self
                .inbound_events
                .pop_front()
                .expect("front checked before pop");
            let delivery_cycle = self.scheduled_delivery_cycle(event);
            let late_by = local_cycle.saturating_sub(delivery_cycle);
            delivered += 1;
            self.next_rx_delivery_cycle =
                Some(local_cycle.saturating_add(byte_cycles(event.baud_bps)));
            self.trace(format!(
                "deliver rx local_session_cycle={} tx_session_cycle={} delivery_session_cycle={} late_by={} byte={:02X} queue_after={} next_rx_delivery_cycle={}",
                local_cycle,
                event.completed_cycle,
                delivery_cycle,
                late_by,
                event.byte,
                self.inbound_events.len(),
                self.next_rx_delivery_cycle.unwrap_or(0)
            ));
            let before = emulator.uart_debug_snapshot();
            emulator.receive_wonder_swan_link_byte(event.byte);
            let after = emulator.uart_debug_snapshot();
            self.trace(format!(
                "deliver outcome={} before_status={:02X} after_status={:02X} before_rx={:02X} after_rx={:02X} receiver_baud={} sender_baud={}",
                receive_outcome(event, before, after),
                before.status,
                after.status,
                before.rx_data,
                after.rx_data,
                before.baud_bps,
                event.baud_bps
            ));
        }
    }

    fn local_session_cycle(&self, emulator: &WonderSwanEmulator) -> u64 {
        emulator.cpu_cycles().saturating_sub(
            self.local_epoch_cycle
                .unwrap_or_else(|| emulator.cpu_cycles()),
        )
    }

    fn scheduled_delivery_cycle(&self, event: WonderSwanLinkEvent) -> u64 {
        let base_cycle = event.completed_cycle.saturating_add(REMOTE_RX_DELAY_CYCLES);
        self.next_rx_delivery_cycle
            .map_or(base_cycle, |next_cycle| base_cycle.max(next_cycle))
    }

    fn trace_uart_state(&mut self, emulator: &WonderSwanEmulator, context: &str) {
        let snapshot = emulator.uart_debug_snapshot();
        let current = UartTraceState::from_snapshot(snapshot);
        let Some(previous) = self.last_uart_trace_state.replace(current) else {
            self.trace(format!(
                "uart state context={} local_session_cycle={} status={:02X} rx={:02X} ie={:02X} irq={:02X}",
                context,
                self.local_session_cycle(emulator),
                snapshot.status,
                snapshot.rx_data,
                emulator.io_peek8(0xB2),
                emulator.io_peek8(0xB4)
            ));
            return;
        };

        if previous != current {
            self.trace(format!(
                "uart state context={} local_session_cycle={} status={:02X}->{:02X} rx={:02X}->{:02X} ie={:02X} irq={:02X}",
                context,
                self.local_session_cycle(emulator),
                previous.status,
                current.status,
                previous.rx_data,
                current.rx_data,
                emulator.io_peek8(0xB2),
                emulator.io_peek8(0xB4)
            ));
        }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct UartTraceState {
    status: u8,
    rx_data: u8,
}

impl UartTraceState {
    fn from_snapshot(snapshot: UartDebugSnapshot) -> Self {
        Self {
            status: snapshot.status
                & (SERIAL_STATUS_RX_READY
                    | SERIAL_STATUS_OVERRUN
                    | SERIAL_STATUS_FAST_BAUD
                    | SERIAL_STATUS_ENABLE),
            rx_data: snapshot.rx_data,
        }
    }
}

fn receive_outcome(
    event: WonderSwanLinkEvent,
    before: zeff_ws_core::hardware::bus::UartDebugSnapshot,
    after: zeff_ws_core::hardware::bus::UartDebugSnapshot,
) -> &'static str {
    if before.status & SERIAL_STATUS_ENABLE == 0 {
        "dropped_disabled"
    } else if before.status & SERIAL_STATUS_OVERRUN != 0 {
        "dropped_overrun_latched"
    } else if before.status & SERIAL_STATUS_FAST_BAUD != after.status & SERIAL_STATUS_FAST_BAUD {
        "baud_changed_during_receive"
    } else if before.baud_bps != event.baud_bps {
        "baud_mismatch_accepted"
    } else if before.status & SERIAL_STATUS_RX_READY != 0 {
        "overrun_set"
    } else if after.status & SERIAL_STATUS_RX_READY != 0 && after.rx_data == event.byte {
        "accepted"
    } else {
        "unknown"
    }
}

fn byte_cycles(baud_bps: u32) -> u64 {
    (u64::from(zeff_ws_core::hardware::constants::CPU_CLOCK_HZ) / u64::from(baud_bps)) * 10
}

fn format_serial_io_trace_line(
    emulator: &WonderSwanEmulator,
    session_cycle: u64,
    fetched: Option<FetchedInstruction>,
    event: DebugTraceEvent,
) -> Option<String> {
    let (access, port, detail) = match event {
        DebugTraceEvent::IoRead { port, value }
            if matches!(port, SERIAL_DATA_PORT | SERIAL_CONTROL_PORT) =>
        {
            ("ioread", port, format!("value={value:02X}"))
        }
        DebugTraceEvent::IoWrite {
            port,
            written_value,
            old_value,
            new_value,
        } if matches!(
            port,
            SERIAL_DATA_PORT
                | IRQ_ENABLE_PORT
                | SERIAL_CONTROL_PORT
                | IRQ_STATUS_PORT
                | IRQ_ACK_PORT
        ) =>
        {
            (
                "iowrite",
                port,
                format!(
                    "write={written_value:02X} visible_before={old_value:02X} visible_after={new_value:02X}"
                ),
            )
        }
        _ => return None,
    };
    let pc = fetched.map_or_else(|| emulator.cpu_pc(), |fetch| fetch.pc);
    let op = fetched.map_or_else(|| emulator.cpu_last_opcode(), |fetch| fetch.opcode);
    Some(format!(
        "serial-io frame={} local_cycle={} session_cycle={} pc={:05X} op={:02X} {} port={:04X} {} b1={:02X} b3={:02X} ie={:02X} irq={:02X}",
        emulator.frame_count(),
        emulator.cpu_cycles(),
        session_cycle,
        pc,
        op,
        access,
        port,
        detail,
        emulator.io_peek8(SERIAL_DATA_PORT),
        emulator.io_peek8(SERIAL_CONTROL_PORT),
        emulator.io_peek8(IRQ_ENABLE_PORT),
        emulator.io_peek8(IRQ_STATUS_PORT)
    ))
}

fn encode_wonder_swan_link_event(event: WonderSwanLinkEvent) -> Vec<u8> {
    let mut out = Vec::with_capacity(TX_BYTE_PAYLOAD_LEN);
    out.push(EVENT_TX_BYTE);
    out.extend_from_slice(&event.completed_cycle.to_le_bytes());
    out.extend_from_slice(&event.generation.to_le_bytes());
    out.extend_from_slice(&event.baud_bps.to_le_bytes());
    out.push(event.byte);
    out
}

fn decode_wonder_swan_link_event(
    payload: &[u8],
) -> Result<WonderSwanLinkEvent, WonderSwanLinkPayloadError> {
    let Some(kind) = payload.first().copied() else {
        return Err(WonderSwanLinkPayloadError::WrongLength {
            expected: 1,
            actual: 0,
        });
    };
    if kind != EVENT_TX_BYTE {
        return Err(WonderSwanLinkPayloadError::UnknownEvent(kind));
    }
    if payload.len() != TX_BYTE_PAYLOAD_LEN {
        return Err(WonderSwanLinkPayloadError::WrongLength {
            expected: TX_BYTE_PAYLOAD_LEN,
            actual: payload.len(),
        });
    }

    Ok(WonderSwanLinkEvent {
        completed_cycle: read_u64(payload, 1),
        generation: read_u64(payload, 9),
        baud_bps: read_u32(payload, 17),
        byte: payload[21],
    })
}

fn read_u64(payload: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        payload[offset..offset + 8]
            .try_into()
            .expect("payload length checked before u64 read"),
    )
}

fn read_u32(payload: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        payload[offset..offset + 4]
            .try_into()
            .expect("payload length checked before u32 read"),
    )
}

fn encode_watermark(session_cycle: u64) -> [u8; WATERMARK_PAYLOAD_LEN] {
    session_cycle.to_le_bytes()
}

fn decode_watermark(payload: &[u8]) -> Result<u64, WonderSwanLinkPayloadError> {
    if payload.len() != WATERMARK_PAYLOAD_LEN {
        return Err(WonderSwanLinkPayloadError::WrongLength {
            expected: WATERMARK_PAYLOAD_LEN,
            actual: payload.len(),
        });
    }
    Ok(u64::from_le_bytes(
        payload
            .try_into()
            .expect("payload length checked before watermark read"),
    ))
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
    use crate::link::{LinkEndpointId, LinkSystemType};
    use crate::link::{LinkSession, transport::LocalLinkTransport};
    use zeff_ws_core::hardware::cartridge::compute_footer_checksum;

    const SERIAL_DATA_PORT: u16 = 0x00B1;
    const SERIAL_CONTROL_PORT: u16 = 0x00B3;
    const SERIAL_CONTROL_ENABLE: u8 = 0x80;
    const SERIAL_CONTROL_FAST_BAUD: u8 = 0x40;
    const SERIAL_STATUS_RX_READY: u8 = 0x01;
    const SERIAL_STATUS_OVERRUN: u8 = 0x02;

    #[test]
    fn wonder_swan_link_event_payload_roundtrips_tx_byte() {
        let event = WonderSwanLinkEvent {
            completed_cycle: 12_345,
            generation: 7,
            baud_bps: 38_400,
            byte: 0x5A,
        };

        assert_eq!(
            decode_wonder_swan_link_event(&encode_wonder_swan_link_event(event)),
            Ok(event)
        );
    }

    #[test]
    fn wonder_swan_watermark_payload_roundtrips_session_cycle() {
        assert_eq!(decode_watermark(&encode_watermark(0)), Ok(0));
        assert_eq!(decode_watermark(&encode_watermark(123_456)), Ok(123_456));
    }

    #[test]
    fn remote_link_discards_completed_tx_events_from_before_connection_epoch() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = wonder_swan_remote_link(left_transport, 1);
        let mut right_link = wonder_swan_remote_link(right_transport, 2);
        let mut left = wonder_swan_emulator();
        let mut right = wonder_swan_emulator();

        left.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);
        right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

        left.io_write8(SERIAL_DATA_PORT, 0xA5);
        step_until_cycle(&mut left, 3_200);

        left_link.poll_emulator(&mut left).unwrap();
        right_link.poll_emulator(&mut right).unwrap();

        assert_eq!(
            right.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
            0,
            "bytes completed before Host/Join should not be delivered after connection"
        );
    }

    #[test]
    fn remote_link_delivers_tx_event_after_remote_rx_delay() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = wonder_swan_remote_link(left_transport, 1);
        let mut right_link = wonder_swan_remote_link(right_transport, 2);
        let mut left = wonder_swan_emulator();
        let mut right = wonder_swan_emulator();

        left.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);
        right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

        left_link.poll_emulator(&mut left).unwrap();
        right_link.poll_emulator(&mut right).unwrap();

        left.io_write8(SERIAL_DATA_PORT, 0x5A);
        step_until_cycle(&mut left, 3_200);
        left_link.poll_emulator(&mut left).unwrap();
        right_link.poll_emulator(&mut right).unwrap();

        assert_eq!(
            right.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
            0,
            "receiver should queue a future byte instead of injecting it immediately"
        );

        step_until_cycle(&mut right, 3_200 + REMOTE_RX_DELAY_CYCLES);
        right_link.poll_emulator(&mut right).unwrap();

        assert_eq!(
            right.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
            SERIAL_STATUS_RX_READY
        );
        assert_eq!(right.io_peek8(SERIAL_DATA_PORT), 0x5A);
    }

    #[test]
    fn remote_link_rx_delay_is_bounded_to_one_slow_uart_byte() {
        const _: () = assert!(REMOTE_RX_DELAY_CYCLES == 3_200);
        const _: () = assert!(
            REMOTE_RX_DELAY_CYCLES < WS_CYCLES_PER_FRAME / 10,
            "the artificial receive delay must stay below frame-scale menu polling timeouts"
        );
    }

    #[test]
    fn remote_link_preserves_rx_spacing_when_due_events_arrive_late() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = wonder_swan_remote_link(left_transport, 1);
        let mut right_link = wonder_swan_remote_link(right_transport, 2);
        let mut left = wonder_swan_emulator();
        let mut right = wonder_swan_emulator();

        left.io_write8(
            SERIAL_CONTROL_PORT,
            SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD,
        );
        right.io_write8(
            SERIAL_CONTROL_PORT,
            SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD,
        );

        left_link.poll_emulator(&mut left).unwrap();
        right_link.poll_emulator(&mut right).unwrap();

        left.io_write8(SERIAL_DATA_PORT, 0x11);
        step_until_cycle(&mut left, 800);
        left.io_write8(SERIAL_DATA_PORT, 0x22);
        step_until_cycle(&mut left, 1_600);
        left_link.poll_emulator(&mut left).unwrap();

        step_until_cycle(&mut right, REMOTE_RX_DELAY_CYCLES + 1_600);
        right_link.poll_emulator(&mut right).unwrap();

        assert_eq!(right.io_peek8(SERIAL_DATA_PORT), 0x11);
        assert_eq!(
            right.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
            0,
            "late queued bytes should not be collapsed into a false overrun"
        );

        right_link.poll_emulator(&mut right).unwrap();
        assert_eq!(
            right.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
            0,
            "polling again without advancing to the shifted delivery cycle must not inject another byte"
        );
        assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x11);

        let late_receive_cycle = REMOTE_RX_DELAY_CYCLES + 12_800;
        step_until_cycle(&mut right, late_receive_cycle);
        right_link.poll_emulator(&mut right).unwrap();

        assert_eq!(
            right.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
            SERIAL_STATUS_RX_READY
        );
        assert_eq!(right.io_peek8(SERIAL_DATA_PORT), 0x22);
        assert_eq!(
            right.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
            0
        );
    }

    #[test]
    fn remote_link_preserves_rx_spacing_across_temporarily_empty_queue() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = wonder_swan_remote_link(left_transport, 1);
        let mut right_link = wonder_swan_remote_link(right_transport, 2);
        let mut left = wonder_swan_emulator();
        let mut right = wonder_swan_emulator();

        left.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);
        right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

        left_link.poll_emulator(&mut left).unwrap();
        right_link.poll_emulator(&mut right).unwrap();

        left.io_write8(SERIAL_DATA_PORT, 0x11);
        step_until_cycle(&mut left, 3_200);
        left_link.poll_emulator(&mut left).unwrap();

        step_until_cycle(&mut right, REMOTE_RX_DELAY_CYCLES + 3_200);
        right_link.poll_emulator(&mut right).unwrap();

        assert_eq!(right.io_peek8(SERIAL_DATA_PORT), 0x11);
        assert_eq!(
            right.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
            0
        );

        left.io_write8(SERIAL_DATA_PORT, 0x22);
        step_until_cycle(&mut left, 6_400);
        left_link.poll_emulator(&mut left).unwrap();

        right_link.poll_emulator(&mut right).unwrap();
        assert_eq!(
            right.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
            0,
            "a new remote packet must not be delivered immediately after the previous byte just because the inbound queue was briefly empty"
        );
        assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x11);

        let next_delivery_cycle = right_link
            .next_rx_delivery_cycle
            .expect("deferred byte should keep a next delivery slot");
        step_until_cycle(&mut right, next_delivery_cycle);
        right_link.poll_emulator(&mut right).unwrap();

        assert_eq!(
            right.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
            SERIAL_STATUS_RX_READY
        );
        assert_eq!(right.io_peek8(SERIAL_DATA_PORT), 0x22);
        assert_eq!(
            right.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
            0
        );
    }

    #[test]
    fn remote_link_caps_local_runahead_until_peer_watermark_arrives() {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = wonder_swan_remote_link(left_transport, 1);
        let mut right_link = wonder_swan_remote_link(right_transport, 2);
        let mut left = wonder_swan_emulator();
        let mut right = wonder_swan_emulator();

        left_link.poll_emulator(&mut left).unwrap();
        right_link.poll_emulator(&mut right).unwrap();
        left_link.poll_emulator(&mut left).unwrap();

        assert!(left_link.can_advance(&left));
        step_until_cycle(&mut left, MAX_REMOTE_LEAD_CYCLES + 1);
        left_link.poll_emulator(&mut left).unwrap();
        assert!(
            !left_link.can_advance(&left),
            "local side should pause instead of running arbitrarily far ahead"
        );

        step_until_cycle(&mut right, WATERMARK_INTERVAL_CYCLES);
        right_link.poll_emulator(&mut right).unwrap();
        left_link.poll_emulator(&mut left).unwrap();

        assert!(
            left_link.can_advance(&left),
            "peer watermark should release the local runahead cap"
        );
    }

    fn wonder_swan_remote_link(
        transport: LocalLinkTransport,
        endpoint: u8,
    ) -> WonderSwanRemoteLink<LocalLinkTransport> {
        WonderSwanRemoteLink::new(LinkSession::new(
            transport,
            LinkSystemType::WonderSwan,
            LinkEndpointId(endpoint),
        ))
    }

    fn step_until_cycle(emulator: &mut WonderSwanEmulator, target_cycle: u64) {
        while emulator.cpu_cycles() < target_cycle {
            emulator
                .step_instruction()
                .expect("minimal WonderSwan test ROM should keep running");
        }
    }

    fn wonder_swan_emulator() -> WonderSwanEmulator {
        WonderSwanEmulator::from_rom_data(&minimal_running_ws_rom())
            .expect("minimal WonderSwan ROM should initialize")
    }

    fn minimal_running_ws_rom() -> Vec<u8> {
        let mut rom = vec![0x90; 0x10000];
        rom[0] = 0x90;
        rom[1] = 0xEB;
        rom[2] = 0xFC;
        let reset_vector = rom.len() - 16;
        rom[reset_vector..reset_vector + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
        let footer = rom.len() - 10;
        rom[footer] = 0x01;
        rom[footer + 1] = 0x00;
        rom[footer + 2] = 0x23;
        rom[footer + 4] = 0x01;
        let checksum = compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        rom
    }
}
