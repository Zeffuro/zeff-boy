use super::*;
use crate::hardware::cartridge::compute_footer_checksum;

fn minimal_cart() -> Cartridge {
    let mut rom = vec![0xFF; 0x10000];
    let footer = rom.len() - 10;
    rom[footer + 1] = 0x00;
    rom[footer + 4] = 0x01;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    Cartridge::load(&rom).unwrap()
}

fn color_cart() -> Cartridge {
    let mut rom = vec![0xFF; 0x10000];
    let footer = rom.len() - 10;
    rom[footer + 1] = 0x01;
    rom[footer + 4] = 0x01;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    Cartridge::load(&rom).unwrap()
}

fn large_cart() -> Cartridge {
    let mut rom = vec![0xFF; 2 * 1024 * 1024];
    rom[0x1F_FFF8] = 0xEA;
    rom[0x0F_FFF8] = 0xFF;
    let footer = rom.len() - 10;
    rom[footer + 1] = 0x00;
    rom[footer + 4] = 0x04;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    Cartridge::load(&rom).unwrap()
}

fn eeprom_cart(save_code: u8) -> Cartridge {
    let mut rom = vec![0xFF; 0x10000];
    let footer = rom.len() - 10;
    rom[footer + 1] = 0x00;
    rom[footer + 4] = 0x01;
    rom[footer + 5] = save_code;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    Cartridge::load(&rom).unwrap()
}

#[test]
fn internal_ram_is_read_write() {
    let mut bus = Bus::new(minimal_cart());
    bus.write8(0x1234, 0x56);
    assert_eq!(bus.read8(0x1234), 0x56);
}

#[test]
fn mono_internal_ram_above_16k_is_open_bus() {
    let mut bus = Bus::new(minimal_cart());
    assert_eq!(bus.ram.len(), WS_INTERNAL_RAM_SIZE);
    bus.write8(0x0000, 0x12);
    bus.write8(0x3FFF, 0x34);
    bus.write8(0x4000, 0x56);
    bus.write8(0xFFFF, 0x78);
    assert_eq!(bus.read8(0x0000), 0x12);
    assert_eq!(bus.read8(0x3FFF), 0x34);
    assert_eq!(bus.read8(0x4000), 0x90);
    assert_eq!(bus.read8(0xFFFF), 0x90);
}

#[test]
fn color_internal_ram_is_64k() {
    let mut bus = Bus::new(color_cart());
    assert_eq!(bus.ram.len(), WSC_INTERNAL_RAM_SIZE);
    bus.write8(0x4000, 0x56);
    bus.write8(0xFFFF, 0x78);
    assert_eq!(bus.read8(0x4000), 0x56);
    assert_eq!(bus.read8(0xFFFF), 0x78);
}

#[test]
fn reset_applies_wsc_boot_handoff_memory_and_io() {
    let mut bus = Bus::new(color_cart());
    bus.write8(0xFE00, 0x12);
    bus.io_write8(SYSTEM_CONTROL_PORT, 0x00);

    bus.reset();

    assert_eq!(bus.read8(0xFE00), 0xFF);
    assert_eq!(bus.read8(0xFFFF), 0xFF);
    assert_eq!(bus.io_read8(SYSTEM_CONTROL_PORT), 0x87);
    assert_eq!(bus.io_read8(LINE_COMPARE_PORT), 0x00);
    assert_eq!(bus.io_read8(LCD_CONTROL_PORT), 0x01);
    assert_eq!(bus.io_read8(LCD_VTOTAL_PORT), 0x9E);
    assert_eq!(bus.io_read8(0x0060), 0x0A);
    assert_eq!(bus.io_read8(0x009E), 0x03);
    assert_eq!(
        bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT),
        EEPROM_STATUS_READY
    );
    assert_eq!(bus.io_read8(KEYPAD_PORT), 0x40);
    assert_eq!(bus.io_read8(IRQ_STATUS_PORT), 0x00);
    assert_eq!(bus.io_peek8(IRQ_ACK_PORT), 0x00);
}

#[test]
fn io_bank_ports_update_cartridge_banks() {
    let mut bus = Bus::new(minimal_cart());
    bus.io_write8(ROM_BANK0_PORT, 7);
    bus.io_write8(ROM_BANK1_PORT, 8);
    bus.io_write8(ROM_RAM_BANK_PORT, 9);
    bus.io_write8(ROM_LINEAR_BANK_PORT, 2);
    assert_eq!(bus.io_read8(ROM_BANK0_PORT), 7);
    assert_eq!(bus.io_read8(ROM_BANK1_PORT), 8);
    assert_eq!(bus.io_read8(ROM_RAM_BANK_PORT), 9);
    assert_eq!(bus.io_read8(ROM_LINEAR_BANK_PORT), 0x22);
}

#[test]
fn linear_bank_write_is_deferred_for_one_prefetched_instruction() {
    let mut bus = Bus::new(large_cart());

    bus.io_write8(ROM_LINEAR_BANK_PORT, 0x0E);

    assert_eq!(bus.io_read8(ROM_LINEAR_BANK_PORT), 0x2E);
    assert_eq!(bus.read8(0xFFFF8), 0xEA);

    bus.retire_instruction();
    assert_eq!(bus.read8(0xFFFF8), 0xEA);

    bus.retire_instruction();
    assert_eq!(bus.read8(0xFFFF8), 0xFF);
}

#[test]
fn serial_control_reports_transmit_ready_and_masks_writable_bits() {
    let mut bus = Bus::new(minimal_cart());

    assert_eq!(bus.io_read8(SERIAL_CONTROL_PORT), 0x04);

    bus.io_write8(SERIAL_CONTROL_PORT, 0xA7);

    assert_eq!(bus.io_read8(SERIAL_CONTROL_PORT), 0x84);

    bus.io_write8(SERIAL_CONTROL_PORT, 0xC4);

    assert_eq!(bus.io_read8(SERIAL_CONTROL_PORT), 0xC4);
}

#[test]
fn serial_tx_interrupt_is_level_sensitive_to_uart_and_irq_enable() {
    let mut bus = Bus::new(minimal_cart());

    bus.io_write8(SERIAL_CONTROL_PORT, 0x80);
    assert_eq!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_SERIAL_TX, 0);

    bus.io_write8(IRQ_ENABLE_PORT, IRQ_SERIAL_TX);
    assert_ne!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_SERIAL_TX, 0);

    bus.io_write8(IRQ_ACK_PORT, IRQ_SERIAL_TX);
    assert_ne!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_SERIAL_TX, 0);

    bus.io_write8(SERIAL_CONTROL_PORT, 0x00);
    bus.io_write8(IRQ_ACK_PORT, IRQ_SERIAL_TX);
    assert_eq!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_SERIAL_TX, 0);
}

#[test]
fn serial_tx_completes_after_selected_byte_time_and_syncs_to_peer() {
    let mut left = Bus::new(minimal_cart());
    let mut right = Bus::new(minimal_cart());
    left.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);
    right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

    left.io_write8(SERIAL_DATA_PORT, 0x5A);
    assert_eq!(
        left.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_TX_EMPTY,
        0
    );

    left.step_cycles(3_199);
    left.sync_wonder_swan_link_peer(&mut right);
    assert_eq!(
        left.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_TX_EMPTY,
        0
    );
    assert_eq!(
        right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
        0
    );

    left.step_cycles(1);
    left.sync_wonder_swan_link_peer(&mut right);

    assert_eq!(
        left.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_TX_EMPTY,
        SERIAL_STATUS_TX_EMPTY
    );
    assert_eq!(
        right.io_peek8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
        SERIAL_STATUS_RX_READY
    );
    assert_eq!(right.io_peek8(SERIAL_DATA_PORT), 0x5A);
    assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x5A);
    assert_eq!(
        right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
        0
    );
}

#[test]
fn serial_write_while_tx_busy_does_not_replace_active_byte() {
    let mut left = Bus::new(minimal_cart());
    let mut right = Bus::new(minimal_cart());
    left.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);
    right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

    left.io_write8(SERIAL_DATA_PORT, 0x11);
    left.step_cycles(1_600);
    left.io_write8(SERIAL_DATA_PORT, 0x22);
    left.step_cycles(1_600);
    left.sync_wonder_swan_link_peer(&mut right);

    assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x11);
    assert_eq!(left.uart_debug_snapshot().tx_data, 0x11);
}

#[test]
fn serial_completed_tx_events_queue_until_peer_sync() {
    let mut left = Bus::new(minimal_cart());
    let mut right = Bus::new(minimal_cart());
    left.io_write8(
        SERIAL_CONTROL_PORT,
        SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD,
    );
    right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

    left.io_write8(SERIAL_DATA_PORT, 0x11);
    left.step_cycles(800);
    left.io_write8(SERIAL_DATA_PORT, 0x22);
    left.step_cycles(800);
    assert_eq!(left.uart_debug_snapshot().completed_tx_count, 2);

    left.sync_wonder_swan_link_peer(&mut right);

    assert_eq!(left.uart_debug_snapshot().completed_tx_count, 0);
    assert_eq!(
        right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
        SERIAL_STATUS_OVERRUN
    );
    assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x11);
}

#[test]
fn serial_fast_baud_completes_in_shorter_byte_time() {
    let mut left = Bus::new(minimal_cart());
    let mut right = Bus::new(minimal_cart());
    left.io_write8(
        SERIAL_CONTROL_PORT,
        SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD,
    );
    right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

    left.io_write8(SERIAL_DATA_PORT, 0xA5);
    left.step_cycles(799);
    left.sync_wonder_swan_link_peer(&mut right);
    assert_eq!(
        right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
        0
    );

    left.step_cycles(1);
    left.sync_wonder_swan_link_peer(&mut right);

    assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0xA5);
}

#[test]
fn serial_rx_interrupt_is_level_sensitive_to_uart_and_irq_enable() {
    let mut left = Bus::new(minimal_cart());
    let mut right = Bus::new(minimal_cart());
    left.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);
    right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);
    right.io_write8(IRQ_ENABLE_PORT, IRQ_SERIAL_RX);

    left.io_write8(SERIAL_DATA_PORT, 0x42);
    left.step_cycles(3_200);
    left.sync_wonder_swan_link_peer(&mut right);

    assert_eq!(
        right.io_read8(IRQ_STATUS_PORT) & IRQ_SERIAL_RX,
        IRQ_SERIAL_RX
    );
    right.io_write8(IRQ_ACK_PORT, IRQ_SERIAL_RX);
    assert_eq!(
        right.io_read8(IRQ_STATUS_PORT) & IRQ_SERIAL_RX,
        IRQ_SERIAL_RX
    );

    assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x42);
    right.io_write8(IRQ_ACK_PORT, IRQ_SERIAL_RX);

    assert_eq!(right.io_read8(IRQ_STATUS_PORT) & IRQ_SERIAL_RX, 0);
}

#[test]
fn serial_receive_overrun_preserves_buffer_until_reset() {
    let mut left = Bus::new(minimal_cart());
    let mut right = Bus::new(minimal_cart());
    left.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);
    right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

    left.io_write8(SERIAL_DATA_PORT, 0x11);
    left.step_cycles(3_200);
    left.sync_wonder_swan_link_peer(&mut right);
    left.io_write8(SERIAL_DATA_PORT, 0x22);
    left.step_cycles(3_200);
    left.sync_wonder_swan_link_peer(&mut right);

    assert_eq!(
        right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
        SERIAL_STATUS_OVERRUN
    );
    assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x11);

    right.io_write8(
        SERIAL_CONTROL_PORT,
        SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_RESET_OVERRUN,
    );

    assert_eq!(
        right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
        0
    );
}

#[test]
fn serial_overrun_latches_error_but_allows_receive_after_buffer_read() {
    let mut left = Bus::new(minimal_cart());
    let mut right = Bus::new(minimal_cart());
    left.io_write8(
        SERIAL_CONTROL_PORT,
        SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD,
    );
    right.io_write8(SERIAL_CONTROL_PORT, SERIAL_CONTROL_ENABLE);

    left.io_write8(SERIAL_DATA_PORT, 0x11);
    left.step_cycles(800);
    left.sync_wonder_swan_link_peer(&mut right);
    left.io_write8(SERIAL_DATA_PORT, 0x22);
    left.step_cycles(800);
    left.sync_wonder_swan_link_peer(&mut right);
    assert_eq!(
        right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
        SERIAL_STATUS_OVERRUN
    );

    assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x11);
    left.io_write8(SERIAL_DATA_PORT, 0x33);
    left.step_cycles(800);
    left.sync_wonder_swan_link_peer(&mut right);
    assert_eq!(
        right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_RX_READY,
        SERIAL_STATUS_RX_READY
    );
    assert_eq!(
        right.io_read8(SERIAL_CONTROL_PORT) & SERIAL_STATUS_OVERRUN,
        SERIAL_STATUS_OVERRUN,
        "overrun remains software-visible until B3 bit 5 reset"
    );
    assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x33);

    right.io_write8(
        SERIAL_CONTROL_PORT,
        SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_RESET_OVERRUN,
    );
    left.io_write8(SERIAL_DATA_PORT, 0x44);
    left.step_cycles(800);
    left.sync_wonder_swan_link_peer(&mut right);

    assert_eq!(right.io_read8(SERIAL_DATA_PORT), 0x44);
}

#[test]
fn vblank_sets_enabled_interrupt_active_bit() {
    let mut bus = Bus::new(minimal_cart());
    bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
    bus.io_write8(IRQ_ENABLE_PORT, IRQ_VBLANK);

    bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE * 144);

    assert_eq!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_VBLANK, IRQ_VBLANK);
    assert_eq!(bus.pending_interrupt_vector(), Some(0x26));
    assert_eq!(bus.io_read8(IRQ_VECTOR_BASE_PORT), 0x26);
    assert_eq!(bus.io_read8(IRQ_ACK_PORT), 0x90);
}

#[test]
fn interrupt_acknowledge_port_clears_status_bits() {
    let mut bus = Bus::new(minimal_cart());
    bus.io_write8(IRQ_ENABLE_PORT, IRQ_VBLANK);
    bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE * 144);
    assert_ne!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_VBLANK, 0);
    bus.debug_trace_mode = DebugTraceMode::IoOnly;

    bus.io_write8(IRQ_ACK_PORT, IRQ_VBLANK);

    assert_eq!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_VBLANK, 0);
    assert_eq!(bus.pending_interrupt_vector(), None);
    assert!(bus.debug_trace_events.iter().any(|event| matches!(
        event,
        BusAccessEvent::Write {
            at: None,
            space: TraceWriteKind::Io,
            addr: 0x00B6,
            written_value,
            width: TraceWriteWidth::Byte,
            ..
        } if *written_value == u32::from(IRQ_VBLANK)
    )));
}

#[test]
fn writes_only_trace_skips_reads() {
    let mut bus = Bus::new(minimal_cart());
    bus.debug_trace_mode = DebugTraceMode::WritesOnly;

    bus.read8(0);
    bus.io_read8(IRQ_ENABLE_PORT);
    bus.write8(0, 0x12);
    bus.io_write8(IRQ_ENABLE_PORT, IRQ_VBLANK);

    assert_eq!(bus.debug_trace_events.len(), 2);
    assert!(
        bus.debug_trace_events
            .iter()
            .all(|event| matches!(event, BusAccessEvent::Write { .. }))
    );
}

#[cfg(feature = "profiling")]
#[test]
fn profiling_counts_device_calls_and_transitions() {
    let mut bus = Bus::new(minimal_cart());
    let cycles = super::super::constants::CYCLES_PER_SCANLINE * 144;

    bus.step_cycles(cycles);

    let snapshot = bus.profiling_snapshot();
    assert_eq!(snapshot.bus_step_calls, 1);
    assert_eq!(snapshot.master_cycles, u64::from(cycles));
    assert_eq!(snapshot.uart_step_calls, 1);
    assert_eq!(snapshot.apu_step_calls, 1);
    assert_eq!(snapshot.sound_dma_step_calls, 1);
    assert_eq!(snapshot.ppu_step_calls, 1);
    assert_eq!(snapshot.completed_scanlines, 144);
    assert_eq!(snapshot.vblank_starts, 1);
    assert_eq!(snapshot.hblank_timer_advances, 144);
    assert_eq!(snapshot.vblank_timer_advances, 1);

    bus.reset_profiling();
    assert_eq!(bus.profiling_snapshot(), ProfilingSnapshot::default());
}

#[test]
fn interrupt_priority_prefers_highest_pending_bit() {
    let mut bus = Bus::new(minimal_cart());
    bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
    bus.io_write8(IRQ_ENABLE_PORT, IRQ_HBLANK_TIMER | IRQ_VBLANK);
    bus.raise_interrupt(IRQ_VBLANK);
    bus.raise_interrupt(IRQ_HBLANK_TIMER);

    assert_eq!(bus.pending_interrupt_vector(), Some(0x27));
    assert_eq!(bus.io_read8(IRQ_VECTOR_BASE_PORT), 0x27);
    bus.io_write8(IRQ_ACK_PORT, IRQ_HBLANK_TIMER);
    assert_eq!(bus.pending_interrupt_vector(), Some(0x26));
    assert_eq!(bus.io_read8(IRQ_VECTOR_BASE_PORT), 0x26);
}

#[test]
fn interrupt_enable_write_does_not_clear_latched_status_bits() {
    let mut bus = Bus::new(minimal_cart());
    bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
    bus.io_write8(IRQ_ENABLE_PORT, IRQ_HBLANK_TIMER);
    bus.raise_interrupt(IRQ_HBLANK_TIMER);
    assert_eq!(
        bus.io_read8(IRQ_STATUS_PORT) & IRQ_HBLANK_TIMER,
        IRQ_HBLANK_TIMER
    );

    bus.io_write8(IRQ_ENABLE_PORT, IRQ_VBLANK);

    assert_eq!(
        bus.io_read8(IRQ_STATUS_PORT) & IRQ_HBLANK_TIMER,
        IRQ_HBLANK_TIMER
    );
    assert_eq!(bus.pending_interrupt_vector(), Some(0x27));
}

#[test]
fn current_line_port_tracks_ppu_vcount() {
    let mut bus = Bus::new(minimal_cart());

    bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE * 3);

    assert_eq!(bus.io_read8(CURRENT_LINE_PORT), 3);
}

#[test]
fn line_compare_raises_enabled_interrupt() {
    let mut bus = Bus::new(minimal_cart());
    bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
    bus.io_write8(IRQ_ENABLE_PORT, IRQ_LINE_COMPARE);
    bus.io_write8(LINE_COMPARE_PORT, 2);

    bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE * 2);

    assert_eq!(
        bus.io_read8(IRQ_STATUS_PORT) & IRQ_LINE_COMPARE,
        IRQ_LINE_COMPARE
    );
    assert_eq!(bus.pending_interrupt_vector(), Some(0x24));
}

#[test]
fn hblank_timer_counts_scanlines_and_raises_interrupt() {
    let mut bus = Bus::new(minimal_cart());
    bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
    bus.io_write8(IRQ_ENABLE_PORT, IRQ_HBLANK_TIMER);
    bus.io_write8(HBLANK_TIMER_RELOAD_LO_PORT, 2);
    bus.io_write8(HBLANK_TIMER_RELOAD_HI_PORT, 0);
    bus.io_write8(TIMER_CONTROL_PORT, 0x01);

    bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE);
    assert_eq!(bus.io_read8(HBLANK_TIMER_COUNT_LO_PORT), 1);
    assert_eq!(bus.io_read8(IRQ_STATUS_PORT) & IRQ_HBLANK_TIMER, 0);

    bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE);

    assert_eq!(
        bus.io_read8(IRQ_STATUS_PORT) & IRQ_HBLANK_TIMER,
        IRQ_HBLANK_TIMER
    );
    assert_eq!(bus.pending_interrupt_vector(), Some(0x27));
}

#[test]
fn vblank_timer_counts_frames_and_raises_interrupt() {
    let mut bus = Bus::new(minimal_cart());
    bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
    bus.io_write8(IRQ_ENABLE_PORT, IRQ_VBLANK_TIMER);
    bus.io_write8(VBLANK_TIMER_RELOAD_LO_PORT, 1);
    bus.io_write8(VBLANK_TIMER_RELOAD_HI_PORT, 0);
    bus.io_write8(TIMER_CONTROL_PORT, 0x04);

    bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE * 144);

    assert_eq!(
        bus.io_read8(IRQ_STATUS_PORT) & IRQ_VBLANK_TIMER,
        IRQ_VBLANK_TIMER
    );
    assert_eq!(bus.pending_interrupt_vector(), Some(0x25));
}

#[test]
fn disabled_hblank_timer_does_not_count_but_still_signals_zero_transition() {
    let mut bus = Bus::new(minimal_cart());
    bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
    bus.io_write8(IRQ_ENABLE_PORT, IRQ_HBLANK_TIMER);
    bus.io_write8(HBLANK_TIMER_RELOAD_LO_PORT, 1);
    bus.io_write8(HBLANK_TIMER_RELOAD_HI_PORT, 0);
    bus.io_write8(TIMER_CONTROL_PORT, 0x00);

    bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE);

    assert_eq!(bus.io_read8(HBLANK_TIMER_COUNT_LO_PORT), 1);
    assert_eq!(
        bus.io_read8(IRQ_STATUS_PORT) & IRQ_HBLANK_TIMER,
        IRQ_HBLANK_TIMER
    );
    assert_eq!(bus.pending_interrupt_vector(), Some(0x27));
}

#[test]
fn disabled_vblank_timer_does_not_count_but_still_signals_zero_transition() {
    let mut bus = Bus::new(minimal_cart());
    bus.io_write8(IRQ_VECTOR_BASE_PORT, 0x20);
    bus.io_write8(IRQ_ENABLE_PORT, IRQ_VBLANK_TIMER);
    bus.io_write8(VBLANK_TIMER_RELOAD_LO_PORT, 1);
    bus.io_write8(VBLANK_TIMER_RELOAD_HI_PORT, 0);
    bus.io_write8(TIMER_CONTROL_PORT, 0x00);

    bus.step_cycles(super::super::constants::CYCLES_PER_SCANLINE * 144);

    assert_eq!(bus.io_read8(VBLANK_TIMER_COUNT_LO_PORT), 1);
    assert_eq!(
        bus.io_read8(IRQ_STATUS_PORT) & IRQ_VBLANK_TIMER,
        IRQ_VBLANK_TIMER
    );
    assert_eq!(bus.pending_interrupt_vector(), Some(0x25));
}

#[test]
fn internal_eeprom_write_sets_completion_and_can_be_read_back() {
    let mut bus = Bus::new(minimal_cart());
    bus.io_write8(INTERNAL_EEPROM_ADDR_LO_PORT, 3);
    bus.io_write8(INTERNAL_EEPROM_ADDR_HI_PORT, 0);
    bus.io_write8(INTERNAL_EEPROM_DATA_LO_PORT, 0x34);
    bus.io_write8(INTERNAL_EEPROM_DATA_HI_PORT, 0x12);

    bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x20);

    assert_eq!(bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT) & 0x7E, 0x02);

    bus.io_write8(INTERNAL_EEPROM_DATA_LO_PORT, 0x00);
    bus.io_write8(INTERNAL_EEPROM_DATA_HI_PORT, 0x00);
    bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x10);

    assert_eq!(bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT) & 0x01, 0x00);
    assert_eq!(bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT) & 0x01, 0x01);
    assert_eq!(bus.io_read8(INTERNAL_EEPROM_DATA_LO_PORT), 0x34);
    assert_eq!(bus.io_read8(INTERNAL_EEPROM_DATA_HI_PORT), 0x12);
}

#[test]
fn internal_eeprom_lock_unlock_and_protect_affect_writes() {
    let mut bus = Bus::new(minimal_cart());

    bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x0100);
    bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x40);
    bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x0140);
    bus.io_write16(INTERNAL_EEPROM_DATA_LO_PORT, 0x1234);
    bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x20);
    bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x0180);
    bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x10);
    bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT);
    bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT);
    assert_ne!(bus.io_read16(INTERNAL_EEPROM_DATA_LO_PORT), 0x1234);

    bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x0130);
    bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x40);
    bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x0140);
    bus.io_write16(INTERNAL_EEPROM_DATA_LO_PORT, 0x1234);
    bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x20);
    bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x0180);
    bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x10);
    bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT);
    bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT);
    assert_eq!(bus.io_read16(INTERNAL_EEPROM_DATA_LO_PORT), 0x1234);

    bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, EEPROM_STATUS_PROTECTED);
    assert_ne!(
        bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT) & EEPROM_STATUS_PROTECTED,
        0
    );
    bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x0170);
    bus.io_write16(INTERNAL_EEPROM_DATA_LO_PORT, 0x5678);
    bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x20);
    bus.io_write16(INTERNAL_EEPROM_ADDR_LO_PORT, 0x01B0);
    bus.io_write8(INTERNAL_EEPROM_COMMAND_PORT, 0x10);
    bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT);
    bus.io_read8(INTERNAL_EEPROM_COMMAND_PORT);
    assert_ne!(bus.io_read16(INTERNAL_EEPROM_DATA_LO_PORT), 0x5678);
}

#[test]
fn system_control_reports_console_type_and_unlock_bit() {
    let mut mono = Bus::new(minimal_cart());
    let mut color = Bus::new(color_cart());

    assert_eq!(mono.io_read8(SYSTEM_CONTROL_PORT) & 0x82, 0x80);
    assert_eq!(color.io_read8(SYSTEM_CONTROL_PORT) & 0x82, 0x82);
}

#[test]
fn mono_palette_registers_mask_to_hardware_writable_bits() {
    let mut bus = Bus::new(minimal_cart());

    bus.io_write16(0x20, 0xFFFF);
    bus.io_write16(0x28, 0xFFFF);
    bus.io_write16(0x30, 0x4321);
    bus.io_write16(0x38, 0x4321);

    assert_eq!(bus.io_read16(0x20), 0x7777);
    assert_eq!(bus.io_read16(0x28), 0x7770);
    assert_eq!(bus.io_read16(0x30), 0x4321);
    assert_eq!(bus.io_read16(0x38), 0x4320);
}

#[test]
fn color_model_keeps_mono_palette_io_byte_writes_unmasked() {
    let mut bus = Bus::new(color_cart());

    bus.io_write16(0x28, 0xFFFF);

    assert_eq!(bus.io_read16(0x28), 0xFFFF);
}

#[test]
fn mono_model_hides_color_dma_ports() {
    let mut bus = Bus::new(minimal_cart());
    bus.write8(0x0000, 0x12);
    bus.io_write16(DMA_SOURCE_LO_PORT, 0x0000);
    bus.io_write16(DMA_SOURCE_SEGMENT_PORT, 0x0000);
    bus.io_write16(DMA_DESTINATION_LO_PORT, 0x0100);
    bus.io_write16(DMA_LENGTH_LO_PORT, 0x0002);
    bus.io_write8(DMA_CONTROL_PORT, 0x80);

    assert_eq!(bus.io_read8(DMA_SOURCE_LO_PORT), 0x90);
    assert_eq!(bus.io_read8(DMA_CONTROL_PORT), 0x90);
    assert_eq!(bus.read8(0x0100), 0x00);
}

#[test]
fn mono_model_hides_hyper_voice_control_ports() {
    let mut bus = Bus::new(minimal_cart());

    bus.io_write8(0x006A, 0xFF);
    bus.io_write8(0x006B, 0xFF);

    assert_eq!(bus.io_read8(0x006A), 0x90);
    assert_eq!(bus.io_read8(0x006B), 0x90);
    let apu = bus.apu_debug_snapshot();
    assert_eq!(apu.hyper_voice_control, 0);
    assert_eq!(apu.hyper_voice_channel_control, 0);
}

#[test]
fn color_model_exposes_hyper_voice_control_ports() {
    let mut bus = Bus::new(color_cart());

    bus.io_write8(0x006A, 0x8F);
    bus.io_write8(0x006B, 0xFF);

    assert_eq!(bus.io_read8(0x006A), 0x8F);
    assert_eq!(bus.io_read8(0x006B), 0x70);
}

#[test]
fn dma_control_start_copies_words_and_clears_start_bit() {
    let mut bus = Bus::new(color_cart());
    bus.write8(0x0000, 0x12);
    bus.write8(0x0001, 0x34);
    bus.write8(0x0002, 0x56);
    bus.write8(0x0003, 0x78);
    bus.io_write8(DMA_SOURCE_LO_PORT, 0x00);
    bus.io_write8(DMA_SOURCE_HI_PORT, 0x00);
    bus.io_write8(DMA_SOURCE_SEGMENT_PORT, 0x00);
    bus.io_write8(DMA_DESTINATION_LO_PORT, 0x00);
    bus.io_write8(DMA_DESTINATION_HI_PORT, 0x01);
    bus.io_write8(DMA_LENGTH_LO_PORT, 0x04);
    bus.io_write8(DMA_LENGTH_HI_PORT, 0x00);

    bus.io_write8(DMA_CONTROL_PORT, 0x80);

    assert_eq!(bus.read8(0x0100), 0x12);
    assert_eq!(bus.read8(0x0101), 0x34);
    assert_eq!(bus.read8(0x0102), 0x56);
    assert_eq!(bus.read8(0x0103), 0x78);
    assert_eq!(bus.io_read8(DMA_LENGTH_LO_PORT), 0);
    assert_eq!(bus.io_read8(DMA_LENGTH_HI_PORT), 0);
    assert_eq!(bus.io_read8(DMA_CONTROL_PORT) & 0x80, 0);
}

#[test]
fn dma_registers_mask_alignment_and_source_high_word() {
    let mut bus = Bus::new(color_cart());

    bus.io_write16(DMA_SOURCE_LO_PORT, 0xB001);
    bus.io_write16(DMA_SOURCE_SEGMENT_PORT, 0xFFFF);
    bus.io_write16(DMA_DESTINATION_LO_PORT, 0x7001);
    bus.io_write16(DMA_LENGTH_LO_PORT, 0xFFFF);

    assert_eq!(bus.io_read16(DMA_SOURCE_LO_PORT), 0xB000);
    assert_eq!(bus.io_read16(DMA_SOURCE_SEGMENT_PORT), 0x000F);
    assert_eq!(bus.io_read16(DMA_DESTINATION_LO_PORT), 0x7000);
    assert_eq!(bus.io_read16(DMA_LENGTH_LO_PORT), 0xFFFE);
}

#[test]
fn dma_rejects_sram_and_slow_rom_sources_without_consuming_length() {
    let mut bus = Bus::new(color_cart());

    bus.io_write16(DMA_SOURCE_SEGMENT_PORT, 0x0001);
    bus.io_write16(DMA_SOURCE_LO_PORT, 0x0000);
    bus.io_write16(DMA_DESTINATION_LO_PORT, 0x7000);
    bus.io_write16(DMA_LENGTH_LO_PORT, 0x1000);
    bus.io_write8(DMA_CONTROL_PORT, 0x80);
    assert_eq!(bus.io_read16(DMA_LENGTH_LO_PORT), 0x1000);

    bus.io_write16(DMA_SOURCE_SEGMENT_PORT, 0x0008);
    bus.io_write16(DMA_SOURCE_LO_PORT, 0x0000);
    bus.io_write16(DMA_DESTINATION_LO_PORT, 0x7000);
    bus.io_write16(DMA_LENGTH_LO_PORT, 0x1000);
    let slow_rom_control = bus.io_read8(SYSTEM_CONTROL_PORT) | SYSTEM_CTRL1_ROM_WAIT;
    bus.io_write8(SYSTEM_CONTROL_PORT, slow_rom_control);
    bus.io_write8(DMA_CONTROL_PORT, 0x80);
    assert_eq!(bus.io_read16(DMA_LENGTH_LO_PORT), 0x1000);

    let fast_rom_control = bus.io_read8(SYSTEM_CONTROL_PORT) & !SYSTEM_CTRL1_ROM_WAIT;
    bus.io_write8(SYSTEM_CONTROL_PORT, fast_rom_control);
    bus.io_write8(DMA_CONTROL_PORT, 0x80);
    assert_eq!(bus.io_read16(DMA_LENGTH_LO_PORT), 0x0000);
}

#[test]
fn sound_dma_registers_are_twenty_bit_and_transfer_to_channel_2_volume() {
    let mut bus = Bus::new(color_cart());

    bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0x5555);
    bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x5555);
    bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0xFFFF);
    bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0xFFFF);

    assert_eq!(bus.io_read16(SOUND_DMA_SOURCE_LO_PORT), 0x5555);
    assert_eq!(bus.io_read16(SOUND_DMA_SOURCE_SEGMENT_PORT), 0x000F);
    assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_LO_PORT), 0x5555);
    assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_SEGMENT_PORT), 0x000F);

    bus.write8(0x1234, 0x5A);
    bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0x1234);
    bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0x0000);
    bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x0001);
    bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0x0000);
    bus.io_write8(SOUND_DMA_CONTROL_PORT, SOUND_DMA_ENABLE | 0x03);
    bus.step_cycles(128);

    assert_eq!(bus.io_read8(SOUND_VOLUME_CHANNEL2_PORT), 0x5A);
    assert_eq!(bus.io_read16(SOUND_DMA_SOURCE_LO_PORT), 0x1235);
    assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_LO_PORT), 0x0000);
    assert_eq!(bus.io_read8(SOUND_DMA_CONTROL_PORT), 0x03);
}

#[test]
fn sound_dma_zero_length_enable_fails() {
    let mut bus = Bus::new(color_cart());
    bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x0000);
    bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0x0000);

    bus.io_write8(SOUND_DMA_CONTROL_PORT, SOUND_DMA_ENABLE | 0x03);

    assert_eq!(bus.io_read8(SOUND_DMA_CONTROL_PORT) & SOUND_DMA_ENABLE, 0);
}

#[test]
fn sound_dma_hold_writes_zero_without_consuming_length() {
    let mut bus = Bus::new(color_cart());
    bus.write8(0x1234, 0x7F);
    bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0x1234);
    bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0x0000);
    bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x0001);
    bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0x0000);
    bus.apu.write_hyper_voice_dma_sample(0x55);
    bus.io_write8(
        SOUND_DMA_CONTROL_PORT,
        SOUND_DMA_ENABLE | SOUND_DMA_HOLD | SOUND_DMA_TARGET_HYPERVOICE | 0x03,
    );

    bus.step_cycles(128);

    assert_eq!(bus.apu_debug_snapshot().hyper_voice_sample, 0x00);
    assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_LO_PORT), 0x0001);
    assert_eq!(
        bus.io_read8(SOUND_DMA_CONTROL_PORT) & SOUND_DMA_ENABLE,
        SOUND_DMA_ENABLE
    );
}

#[test]
fn sound_dma_repeat_control_update_and_address_wrap_match_source_oracle() {
    let mut bus = Bus::new(color_cart());

    bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0x5555);
    bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x5555);
    bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0xFFFF);
    bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0xFFFF);
    assert_eq!(bus.io_read16(SOUND_DMA_SOURCE_SEGMENT_PORT), 0x000F);
    assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_SEGMENT_PORT), 0x000F);

    bus.write8(0x1234, 0x55);
    bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0x1234);
    bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0x0000);
    bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x0001);
    bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0x0000);
    bus.io_write8(
        SOUND_DMA_CONTROL_PORT,
        SOUND_DMA_ENABLE | SOUND_DMA_REPEAT | 0x03,
    );
    bus.step_cycles(128);
    assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_LO_PORT), 0x0000);

    let control = bus.io_read8(SOUND_DMA_CONTROL_PORT) | SOUND_DMA_HOLD;
    bus.io_write8(SOUND_DMA_CONTROL_PORT, control);
    assert_ne!(bus.io_read8(SOUND_DMA_CONTROL_PORT) & SOUND_DMA_ENABLE, 0);
    bus.step_cycles(128);
    assert_eq!(bus.io_read8(SOUND_VOLUME_CHANNEL2_PORT), 0x00);
    assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_LO_PORT), 0x0000);

    let control = bus.io_read8(SOUND_DMA_CONTROL_PORT) & !SOUND_DMA_HOLD;
    bus.io_write8(SOUND_DMA_CONTROL_PORT, control);
    bus.step_cycles(128);
    assert_eq!(bus.io_read8(SOUND_VOLUME_CHANNEL2_PORT), 0x55);

    bus.io_write8(SOUND_DMA_CONTROL_PORT, 0);
    bus.write8(0x000D, 0xFF);
    bus.write8(0x000E, 0xAA);
    bus.write8(0x000F, 0x55);
    bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0xFFFF);
    bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0x000F);
    bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x0010);
    bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0x0000);
    bus.io_write8(SOUND_DMA_CONTROL_PORT, SOUND_DMA_ENABLE | 0x03);
    bus.step_cycles(128 * 16);

    assert_eq!(bus.io_read16(SOUND_DMA_SOURCE_LO_PORT), 0x000F);
    assert_eq!(bus.io_read16(SOUND_DMA_SOURCE_SEGMENT_PORT), 0x0000);
    assert_eq!(bus.io_read8(SOUND_VOLUME_CHANNEL2_PORT), 0xAA);
}

#[test]
fn sound_dma_decrement_direction_reads_source_backwards() {
    let mut bus = Bus::new(color_cart());
    bus.write8(0x1234, 0x22);
    bus.write8(0x1235, 0x11);
    bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0x1235);
    bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0x0000);
    bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x0002);
    bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0x0000);
    bus.io_write8(
        SOUND_DMA_CONTROL_PORT,
        SOUND_DMA_ENABLE | SOUND_DMA_DECREMENT | 0x03,
    );

    bus.step_cycles(128);
    assert_eq!(bus.io_read8(SOUND_VOLUME_CHANNEL2_PORT), 0x11);
    assert_eq!(bus.io_read16(SOUND_DMA_SOURCE_LO_PORT), 0x1234);
    assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_LO_PORT), 0x0001);

    bus.step_cycles(128);
    assert_eq!(bus.io_read8(SOUND_VOLUME_CHANNEL2_PORT), 0x22);
    assert_eq!(bus.io_read16(SOUND_DMA_SOURCE_LO_PORT), 0x1233);
    assert_eq!(bus.io_read16(SOUND_DMA_LENGTH_LO_PORT), 0x0000);
    assert_eq!(bus.io_read8(SOUND_DMA_CONTROL_PORT) & SOUND_DMA_ENABLE, 0);
}

#[test]
fn sound_dma_can_target_hyper_voice_sample() {
    let mut bus = Bus::new(color_cart());
    bus.write8(0x1234, 0x6A);
    bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0x1234);
    bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0x0000);
    bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 0x0001);
    bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0x0000);
    bus.io_write8(
        SOUND_DMA_CONTROL_PORT,
        SOUND_DMA_ENABLE | SOUND_DMA_TARGET_HYPERVOICE | 0x03,
    );

    bus.step_cycles(128);

    assert_eq!(bus.apu_debug_snapshot().hyper_voice_sample, 0x6A);
    assert_eq!(bus.io_read8(SOUND_DMA_CONTROL_PORT) & SOUND_DMA_ENABLE, 0);
}

#[test]
fn cartridge_eeprom_ports_read_back_written_word() {
    let mut bus = Bus::new(eeprom_cart(0x10));
    bus.io_write16(CART_EEPROM_COMMAND_LO_PORT, 0x0130);
    bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x40);

    let write_command = 0x0100 | 0x0040 | 3;
    bus.io_write16(CART_EEPROM_COMMAND_LO_PORT, write_command);
    bus.io_write16(CART_EEPROM_DATA_LO_PORT, 0x1234);
    bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x20);

    let read_command = 0x0100 | 0x0080 | 3;
    bus.io_write16(CART_EEPROM_COMMAND_LO_PORT, read_command);
    bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x10);

    assert_eq!(bus.io_read16(CART_EEPROM_DATA_LO_PORT), 0x1234);
    assert_eq!(
        bus.io_read8(CART_EEPROM_CONTROL_STATUS_LO_PORT)
            & (EEPROM_STATUS_READ_DONE | EEPROM_STATUS_READY),
        EEPROM_STATUS_READ_DONE | EEPROM_STATUS_READY
    );
}

#[test]
fn cartridge_eeprom_16kbit_command_uses_extended_address() {
    let mut bus = Bus::new(eeprom_cart(0x20));
    bus.io_write16(CART_EEPROM_COMMAND_LO_PORT, 0x1300);
    bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x40);

    let address = 0x02A5;
    bus.io_write16(
        CART_EEPROM_COMMAND_LO_PORT,
        0x1000 | 0x0400 | address as u16,
    );
    bus.io_write16(CART_EEPROM_DATA_LO_PORT, 0xBEEF);
    bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x20);

    bus.io_write16(
        CART_EEPROM_COMMAND_LO_PORT,
        0x1000 | 0x0800 | address as u16,
    );
    bus.io_write8(CART_EEPROM_CONTROL_STATUS_LO_PORT, 0x10);

    assert_eq!(bus.io_read16(CART_EEPROM_DATA_LO_PORT), 0xBEEF);
}

#[test]
fn rtc_command_status_reports_ready() {
    let mut bus = Bus::new(color_cart());

    bus.io_write8(RTC_COMMAND_STATUS_PORT, 0x13);

    assert_eq!(bus.io_read8(RTC_COMMAND_STATUS_PORT), RTC_ACTIVE | 0x03);
    assert_eq!(bus.io_read8(RTC_COMMAND_STATUS_PORT), RTC_ACTIVE | 0x03);
    assert_eq!(bus.io_read8(RTC_COMMAND_STATUS_PORT), RTC_READY | 0x03);
}

#[test]
fn rtc_status_echoes_unsupported_command_nibble_while_active() {
    let mut bus = Bus::new(color_cart());

    bus.io_write8(RTC_COMMAND_STATUS_PORT, 0x1F);

    assert_eq!(bus.io_peek8(RTC_COMMAND_STATUS_PORT), RTC_ACTIVE | 0x0F);
    assert_eq!(bus.io_read8(RTC_COMMAND_STATUS_PORT), RTC_ACTIVE | 0x0F);
}

#[test]
fn rtc_datetime_payload_can_be_written_and_read_back() {
    let mut bus = Bus::new(color_cart());
    let payload = [0x26, 0x08, 0x02, 0x00, 0x19, 0x45, 0x30];

    bus.io_write8(RTC_PAYLOAD_PORT, payload[0]);
    bus.io_write8(RTC_COMMAND_STATUS_PORT, RTC_WRITE_DATETIME_COMMAND);
    for &value in &payload[1..] {
        bus.io_write8(RTC_PAYLOAD_PORT, value);
    }

    bus.io_write8(RTC_COMMAND_STATUS_PORT, RTC_READ_DATETIME_COMMAND);
    assert_eq!(
        bus.io_read8(RTC_COMMAND_STATUS_PORT),
        RTC_ACTIVE | (RTC_READ_DATETIME_COMMAND & 0x0F)
    );
    assert_eq!(
        bus.io_read8(RTC_COMMAND_STATUS_PORT),
        RTC_ACTIVE | (RTC_READ_DATETIME_COMMAND & 0x0F)
    );
    let mut read_back = [0; 7];
    for value in &mut read_back {
        *value = bus.io_read8(RTC_PAYLOAD_PORT);
    }

    assert_eq!(read_back, payload);
    assert_eq!(bus.io_read8(RTC_PAYLOAD_PORT), RTC_READY);
}
