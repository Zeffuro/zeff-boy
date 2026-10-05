use super::*;
use crate::hardware::cartridge::compute_footer_checksum;
use crate::hardware::cpu::CpuState;

mod snapshots;

fn machine(color: bool, code: &[u8]) -> Emulator {
    let mut rom = vec![0xff; 0x10000];
    let footer = rom.len() - 10;
    rom[footer + 1] = u8::from(color);
    rom[footer + 4] = 1;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    let mut machine = Emulator::new(&rom, 48_000).unwrap();
    machine.cpu.segments = [0; 4];
    machine.cpu.ip = 0x100;
    machine.cpu.flags = 0xf002;
    machine.cpu.regs[0] = 0;
    machine.bus.ram[0x100..0x100 + code.len()].copy_from_slice(code);
    if code.is_empty() {
        machine.cpu.state = CpuState::Halted;
    }
    machine
}

fn uart(machine: &mut Emulator, fast: bool, byte: Option<u8>) {
    machine.io_write8(0xb3, if fast { 0xc0 } else { 0x80 });
    if let Some(byte) = byte {
        machine.io_write8(0xb1, byte);
    }
}

fn dma(machine: &mut Emulator) {
    machine.io_write8(0x40, 0x00);
    machine.io_write8(0x41, 0x10);
    machine.io_write8(0x44, 0x00);
    machine.io_write8(0x45, 0x20);
    machine.io_write8(0x46, 4);
    machine.io_write8(0x47, 0);
}

#[test]
fn idle_pair_and_ws11_inputs_use_fixed_world_frames() {
    for color in [false, true] {
        let mut left = machine(color, &[]);
        let mut right = machine(color, &[]);
        left.io_write8(0x16, 200);
        right.io_write8(0x16, 160);
        let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
        for frame in 1..=3 {
            let output = pair
                .advance_frame([&mut left, &mut right], [0x0715, 0x02a9])
                .unwrap();
            assert_eq!(output.frame, frame);
            assert_eq!(output.bus_ticks, [frame * u64::from(CYCLES_PER_FRAME); 2]);
            assert_eq!(left.frame_count(), frame);
            assert_eq!(right.frame_count(), frame);
            assert!(!output.audio[0].is_empty());
            assert_eq!(output.audio[0], output.audio[1]);
            assert_eq!(pair.pending_events(), 0);
        }
        assert_ne!(
            left.ppu_debug_snapshot().vcount,
            right.ppu_debug_snapshot().vcount
        );
        let keypad = left.bus.keypad.save_state();
        assert_eq!(
            (keypad.x_buttons, keypad.y_buttons, keypad.ab_start),
            (5, 1, 14)
        );
        let keypad = right.bus.keypad.save_state();
        assert_eq!(
            (keypad.x_buttons, keypad.y_buttons, keypad.ab_start),
            (9, 10, 8)
        );
        assert_eq!(pair.frame(), 3);
    }
}

#[test]
fn world_boundary_preserves_scanlines_and_hardware_sprite_latch() {
    let code = [
        0xe4, 0x02, 0x3c, 144, 0x72, 0xfa, 0xb0, 0, 0xe6, 0x14, 0xc6, 0x06, 0x00, 0x02, 0x55, 0xf4,
    ];
    let mut left = machine(false, &code);
    let mut right = machine(false, &[]);
    for (port, value) in [(0x00, 0), (0x01, 7), (0x04, 1), (0x14, 1), (0x16, 200)] {
        left.io_write8(port, value);
    }
    left.bus.ram[0x200] = 0x31;
    let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
    pair.advance_frame([&mut left, &mut right], [0; 2]).unwrap();
    assert_eq!(left.io_peek8(0x14), 0);
    assert_eq!(left.bus.ram[0x200], 0x55);
    assert_eq!(left.bus.ppu.sprite_cache_state().0[0], 0x31);
    assert_eq!(&left.framebuffer()[..4], &[0x18, 0x18, 0x18, 0xff]);
    assert!(!left.frame_ready());
    assert_eq!(left.frame_count(), 1);
}

#[test]
fn fast_slow_and_bidirectional_bytes_arrive_on_bus_deadlines() {
    for color in [false, true] {
        for fast in [false, true] {
            for bidirectional in [false, true] {
                let mut left = machine(color, &[]);
                let mut right = machine(color, &[]);
                uart(&mut left, fast, Some(0x35));
                uart(&mut right, fast, bidirectional.then_some(0xa7));
                let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
                let deadline = if fast { 800 } else { 3200 };
                pair.advance_to([&mut left, &mut right], deadline - 1)
                    .unwrap();
                assert_eq!(right.uart_debug_snapshot().status & 1, 0);
                assert_eq!(left.uart_debug_snapshot().status & 1, 0);
                pair.advance_to([&mut left, &mut right], deadline).unwrap();
                assert_eq!(right.uart_debug_snapshot().rx_data, 0x35);
                assert_eq!(right.uart_debug_snapshot().status & 1, 1);
                assert_eq!(
                    left.uart_debug_snapshot().status & 1,
                    u8::from(bidirectional)
                );
                if bidirectional {
                    assert_eq!(left.uart_debug_snapshot().rx_data, 0xa7);
                }
                assert_eq!(pair.pending_events(), 0);
            }
        }
    }
}

#[test]
fn sender_pre_epoch_and_receiver_instruction_dma_do_not_shift_cable_ticks() {
    let mut left = machine(true, &[]);
    let mut right = machine(true, &[0xb0, 0x80, 0xe6, 0x48, 0xf4]);
    dma(&mut left);
    left.io_write8(0x48, 0x80);
    dma(&mut right);
    assert_eq!(left.bus_cycles() - left.cpu_cycles(), 9);
    uart(&mut left, true, Some(0x6d));
    uart(&mut right, true, None);
    let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
    pair.advance_to([&mut left, &mut right], 799).unwrap();
    assert_eq!(right.uart_debug_snapshot().status & 1, 0);
    assert_eq!(right.bus_cycles() - right.cpu_cycles(), 9);
    pair.advance_to([&mut left, &mut right], 800).unwrap();
    assert_eq!(pair.bus_ticks([&left, &right]).unwrap(), [800; 2]);
    assert_eq!(right.uart_debug_snapshot().rx_data, 0x6d);
}

#[test]
fn crossing_input_instruction_reads_pre_rx_then_observes_rx_at_boundary() {
    let mut left = machine(false, &[0x90; 16]);
    let mut right = machine(false, &[0xe4, 0xb1, 0xf4]);
    uart(&mut left, true, Some(0x9c));
    uart(&mut right, true, None);
    let mut pending = left.bus.uart_save_state();
    pending.tx_cycles_remaining = 5;
    left.bus.load_uart_save_state(pending);
    let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
    pair.advance_to([&mut left, &mut right], 7).unwrap();
    assert_eq!(right.cpu.regs[0] & 0xff, 0);
    assert_eq!(right.cpu.ip, 0x102);
    assert_eq!(right.uart_debug_snapshot().status & 1, 1);
    assert_eq!(right.uart_debug_snapshot().rx_data, 0x9c);
}

#[test]
fn external_rx_irq_wakes_halted_receiver_before_handler_runs() {
    let mut left = machine(false, &[]);
    let mut right = machine(false, &[]);
    uart(&mut left, true, Some(0x81));
    uart(&mut right, true, None);
    right.cpu.flags |= 0x200;
    right.io_write8(0xb0, 0x20);
    right.io_write8(0xb2, 8);
    right.bus.write16(0x23 * 4, 0x200);
    right.bus.write16(0x23 * 4 + 2, 0);
    let handler = [
        0xe4, 0xb1, 0xa2, 0x01, 0x03, 0xb0, 0x08, 0xe6, 0xb6, 0xc6, 0x06, 0x00, 0x03, 0x5a, 0xf4,
    ];
    right.bus.ram[0x200..0x200 + handler.len()].copy_from_slice(&handler);
    let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
    pair.advance_to([&mut left, &mut right], 800).unwrap();
    assert_eq!(right.cpu_state(), CpuState::Halted);
    assert_eq!(right.io_peek8(0xb4) & 8, 8);
    assert_eq!(right.uart_debug_snapshot().status & 1, 1);
    assert_eq!(right.bus.ram[0x300], 0);
    pair.advance_to([&mut left, &mut right], 900).unwrap();
    assert_eq!(right.bus.ram[0x300], 0x5a);
    assert_eq!(right.bus.ram[0x301], 0x81);
    assert_eq!(right.io_peek8(0xb4) & 8, 0);
    assert_eq!(right.uart_debug_snapshot().status & 1, 0);
    assert_eq!(right.cpu_state(), CpuState::Halted);
}

#[test]
fn control_changes_and_disabled_rx_follow_hardware_uart_rules() {
    let mut left = machine(false, &[0xb0, 0xc0, 0xe6, 0xb3, 0xf4]);
    let mut right = machine(false, &[0xb0, 0x00, 0xe6, 0xb3, 0xf4]);
    uart(&mut left, false, Some(0x41));
    uart(&mut right, false, None);
    let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
    pair.advance_to([&mut left, &mut right], 3199).unwrap();
    assert_eq!(left.uart_debug_snapshot().tx_cycles_remaining, 1);
    pair.advance_to([&mut left, &mut right], 3200).unwrap();
    assert_eq!(left.uart_debug_snapshot().status & 4, 4);
    assert_eq!(right.uart_debug_snapshot().status & 1, 0);
}

#[test]
fn unread_rx_overruns_and_control_reset_preserves_first_byte() {
    let mut left = machine(false, &[]);
    let mut right = machine(false, &[]);
    uart(&mut left, true, Some(0x31));
    uart(&mut right, true, None);
    right.receive_wonder_swan_link_byte(0x22);
    let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
    pair.advance_to([&mut left, &mut right], 800).unwrap();
    assert_eq!(right.uart_debug_snapshot().rx_data, 0x22);
    assert_eq!(right.uart_debug_snapshot().status & 3, 3);
    let saved = pair.capture([&left, &right]).unwrap();
    assert_eq!(saved.machines[1].uart_debug_snapshot().status & 3, 3);
    let mut restored = saved.machines[1].clone();
    restored.io_write8(0xb3, 0xe0);
    assert_eq!(restored.uart_debug_snapshot().status & 3, 1);
    assert_eq!(restored.uart_debug_snapshot().rx_data, 0x22);
}

#[test]
fn queue_order_and_overflow_are_stable_and_bounded() {
    let mut left = machine(false, &[]);
    let mut right = machine(false, &[]);
    uart(&mut left, true, None);
    uart(&mut right, true, None);
    let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
    for sender in [1, 0] {
        let mut state = [&mut left, &mut right][sender].bus.uart_save_state();
        state
            .completed_tx_events
            .push(crate::hardware::bus::WonderSwanTxEvent {
                completed_cycle: 1,
                byte: 0x33 + sender as u8,
                baud_bps: 38400,
                generation: 7,
            });
        [&mut left, &mut right][sender]
            .bus
            .load_uart_save_state(state);
    }
    pair.collect(&mut [&mut left, &mut right]).unwrap();
    assert_eq!(
        pair.schedule
            .events
            .iter()
            .map(|event| event.sender)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    pair.schedule
        .events
        .resize(MAX_CABLE_EVENTS, pair.schedule.events[0].clone());
    let mut state = left.bus.uart_save_state();
    state
        .completed_tx_events
        .push(crate::hardware::bus::WonderSwanTxEvent {
            completed_cycle: 2,
            byte: 0,
            baud_bps: 38400,
            generation: 8,
        });
    left.bus.load_uart_save_state(state);
    assert!(pair.collect(&mut [&mut left, &mut right]).is_err());
    assert_eq!(pair.pending_events(), MAX_CABLE_EVENTS);
}
