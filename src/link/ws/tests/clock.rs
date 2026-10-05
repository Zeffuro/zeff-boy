use super::*;

fn halted_color_emulator() -> WonderSwanEmulator {
    let mut rom = minimal_running_ws_rom();
    rom[0] = 0xf4;
    let footer = rom.len() - 10;
    rom[footer + 1] = 1;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    let mut emu = WonderSwanEmulator::from_rom_data(&rom).unwrap();
    while emu.cpu_last_opcode() != 0xf4 {
        emu.step_instruction();
    }
    emu.io_write8(0xb2, 0);
    emu
}

fn dma(emu: &mut WonderSwanEmulator) {
    let before = emu.bus_cycles();
    let cpu = emu.cpu_cycles();
    for (port, value) in [
        (0x40, 0),
        (0x41, 0x10),
        (0x42, 0),
        (0x44, 0),
        (0x45, 0x20),
        (0x46, 4),
        (0x47, 0),
        (0x48, 0x80),
    ] {
        emu.io_write8(port, value);
    }
    assert_eq!(emu.bus_cycles(), before + 9);
    assert_eq!(emu.cpu_cycles(), cpu);
}

fn advance_bus(emu: &mut WonderSwanEmulator, target: u64) {
    let mut budget = 200_000;
    while emu.bus_cycles() < target {
        assert!(budget > 0);
        budget -= 1;
        emu.step_instruction();
    }
    assert_eq!(emu.bus_cycles(), target);
}

#[test]
fn remote_link_uses_uart_bus_clock_across_sender_and_receiver_dma() {
    for (sender_dma, receiver_dma) in [(true, false), (false, true), (true, true)] {
        let (left_transport, right_transport) = LocalLinkTransport::pair();
        let mut left_link = wonder_swan_remote_link(left_transport, 1);
        let mut right_link = wonder_swan_remote_link(right_transport, 2);
        let mut left = halted_color_emulator();
        let mut right = halted_color_emulator();
        if sender_dma {
            dma(&mut left);
        }
        let control = SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD;
        left.io_write8(SERIAL_CONTROL_PORT, control);
        right.io_write8(SERIAL_CONTROL_PORT, control);
        left_link.poll_emulator(&mut left).unwrap();
        right_link.poll_emulator(&mut right).unwrap();
        let left_epoch = left.bus_cycles();
        let right_epoch = right.bus_cycles();
        if receiver_dma {
            dma(&mut right);
        }
        left.io_write8(SERIAL_DATA_PORT, 0xa5);
        advance_bus(&mut left, left_epoch + 800);
        left_link.poll_emulator(&mut left).unwrap();
        right_link.poll_emulator(&mut right).unwrap();
        assert_eq!(
            right_link.inbound_events.front().unwrap().completed_cycle,
            800
        );
        advance_bus(&mut right, right_epoch + 800 + REMOTE_RX_DELAY_CYCLES - 1);
        right_link.poll_emulator(&mut right).unwrap();
        assert_eq!(
            right.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
            0
        );
        advance_bus(&mut right, right_epoch + 800 + REMOTE_RX_DELAY_CYCLES);
        right_link.poll_emulator(&mut right).unwrap();
        assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0xa5);
        assert_eq!(right_link.local_session_cycle(&right), 4_000);
    }
}

#[test]
fn remote_link_runahead_counts_dma_bus_time() {
    let (left_transport, right_transport) = LocalLinkTransport::pair();
    let mut left_link = wonder_swan_remote_link(left_transport, 1);
    let mut right_link = wonder_swan_remote_link(right_transport, 2);
    let mut left = halted_color_emulator();
    let mut right = halted_color_emulator();
    left_link.poll_emulator(&mut left).unwrap();
    right_link.poll_emulator(&mut right).unwrap();
    left_link.poll_emulator(&mut left).unwrap();
    let epoch = left.bus_cycles();
    advance_bus(&mut left, epoch + MAX_REMOTE_LEAD_CYCLES);
    assert!(left_link.can_advance(&left));
    dma(&mut left);
    assert!(!left_link.can_advance(&left));
}
