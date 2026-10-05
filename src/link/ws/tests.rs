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
    assert!(matches!(
        right_link.take_replay_events().as_slice(),
        [ReplayEvent::WonderSwanLink {
            event: ReplayWonderSwanLinkEvent::RemoteByte {
                generation: _,
                baud_bps: _,
                byte: 0x5A,
            },
            ..
        }]
    ));
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

mod clock;
