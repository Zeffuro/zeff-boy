use crate::hardware::cartridge::EEPROM_WRITE_BUSY_CYCLES;
use zeff_emu_common::debug::{TraceWriteKind, TraceWriteWidth};

use super::*;
use crate::hardware::cartridge::RomHeader;
use crate::hardware::constants::{CYCLES_PER_FRAME, SCREEN_WIDTH};
use crate::hardware::timer::TimerTimingState;

fn cartridge() -> Cartridge {
    let mut rom = vec![0; 0xC0];
    rom[0xB2] = 0x96;
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    Cartridge::load(&rom).unwrap()
}

fn sram_cartridge() -> Cartridge {
    let mut rom = vec![0; 0xC0];
    rom[0xB2] = 0x96;
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom.extend_from_slice(b"SRAM_V113");
    Cartridge::load(&rom).unwrap()
}

fn eeprom_cartridge() -> Cartridge {
    let mut rom = vec![0; 0xC0];
    rom[0xB2] = 0x96;
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom.extend_from_slice(b"EEPROM_V122");
    Cartridge::load(&rom).unwrap()
}

fn emerald_cartridge() -> Cartridge {
    let mut rom = vec![0; 0xC0];
    rom[0xB2] = 0x96;
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"BPEE");
    Cartridge::load(&rom).unwrap()
}

fn hide_oam_except_first(bus: &mut Bus) {
    for base in (8..OAM_SIZE).step_by(8) {
        bus.write16(0x0700_0000 + base as u32, 1 << 9);
    }
}

fn poke_vram8(bus: &mut Bus, addr: u32, value: u8) {
    bus.vram[vram_index(addr)] = value;
}

fn framebuffer_pixel(bus: &Bus, x: usize, y: usize) -> &[u8] {
    let offset = (y * SCREEN_WIDTH + x) * 4;
    &bus.ppu.framebuffer()[offset..offset + 4]
}

#[test]
fn deadline_cache_handles_before_equal_and_across_ppu_boundary() {
    let mut bus = Bus::new(cartridge(), 48_000);
    let first = bus.fresh_event_deadline();
    assert_eq!(first.sources, DEADLINE_PPU);

    bus.step_cycles(first.remaining - 1);
    assert_eq!(
        bus.event_deadline,
        BusDeadline {
            remaining: 1,
            sources: DEADLINE_PPU,
        }
    );
    assert!(!bus.ppu.in_hblank());

    bus.step_cycles(1);
    assert!(bus.event_deadline_is_invalid_for_test());
    assert!(bus.ppu.in_hblank());

    let next = bus.fresh_event_deadline();
    bus.step_cycles(next.remaining + 1);
    assert_eq!(bus.event_deadline, bus.fresh_event_deadline());
}

#[test]
fn deadline_cache_preserves_tied_ppu_and_timer_events() {
    let mut bus = Bus::new(cartridge(), 48_000);
    let ppu_cycles = bus.ppu.cycles_until_next_status_event();
    let reload = (0x1_0000 - (ppu_cycles - 1)) as u16;
    bus.write16(0x0400_0100, reload);
    bus.write16(0x0400_0102, 0x00C0);
    let deadline = bus.fresh_event_deadline();

    assert_eq!(deadline.remaining, ppu_cycles);
    assert_eq!(deadline.sources, DEADLINE_PPU | (1 << DEADLINE_TIMER_SHIFT));

    bus.step_cycles(deadline.remaining);

    assert!(bus.ppu.in_hblank());
    assert_eq!(bus.timers.read16(0, false), reload);
    assert_ne!(bus.read16(0x0400_0202) & (1 << 3), 0);
    assert!(bus.event_deadline_is_invalid_for_test());
}

#[test]
fn deadline_cache_invalidates_for_timer_irq_and_reset_mutations() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.step_cycles(1);
    assert!(!bus.event_deadline_is_invalid_for_test());

    bus.write16(0x0400_0100, 0xFFFF);
    assert!(bus.event_deadline_is_invalid_for_test());
    bus.step_cycles(1);
    bus.write16(0x0400_0200, 1);
    bus.request_interrupt(1);
    assert!(bus.event_deadline_is_invalid_for_test());
    assert_eq!(bus.fresh_event_deadline().sources, DEADLINE_IRQ);

    bus.step_cycles(4);
    assert_eq!(bus.take_irq_sample_delay_cycles(), 3);
    assert!(bus.event_deadline_is_invalid_for_test());
    bus.step_cycles(1);
    bus.reset_hardware();
    assert!(bus.event_deadline_is_invalid_for_test());
}

#[test]
fn lazy_timer_materialization_matches_eager_service_across_partitions() {
    let mut lazy = Bus::new(cartridge(), 48_000);
    lazy.write16(0x0400_0200, 0x0078);
    lazy.write16(0x0400_0208, 1);
    lazy.write16(0x0400_0082, (1 << 8) | (1 << 12) | (1 << 14));
    for sample in 0..16u16 {
        lazy.write16(
            0x0400_00A0,
            sample.wrapping_mul(0x0202).wrapping_add(0x0201),
        );
        lazy.write16(
            0x0400_00A4,
            sample.wrapping_mul(0x0202).wrapping_add(0x4241),
        );
    }
    lazy.write16(0x0400_0100, 0xFFF0);
    lazy.write16(0x0400_0102, 0x00C0);
    lazy.write16(0x0400_0104, 0xFFF0);
    lazy.write16(0x0400_0106, 0x00C1);
    lazy.write16(0x0400_0108, 0xFFFE);
    lazy.write16(0x0400_010A, 0x00C4);
    lazy.write16(0x0400_010C, 0xFFFB);
    lazy.write16(0x0400_010E, 0x00C2);
    assert!(lazy.timers.set_timing_state(TimerTimingState {
        cycle_accum: [31, 127, 0x03FF, 341],
        start_delay_cycles: [1, 0, 1, 0],
        clock_phase: 0x03FF,
    }));

    let mut eager = lazy.clone();
    eager.set_eager_timer_materialization_for_test(true);

    let mut saw_deferred_timer_cycles = false;
    for index in 0..160u32 {
        let cycles = index.wrapping_mul(37) % 97 + 1;
        lazy.step_cycles(cycles);
        eager.step_cycles(cycles);
        saw_deferred_timer_cycles |= lazy.timer_materialization_is_pending_for_test();

        for timer in 0..4 {
            assert_eq!(
                lazy.read16(0x0400_0100 + timer * 4),
                eager.read16(0x0400_0100 + timer * 4)
            );
            assert_eq!(
                lazy.read16(0x0400_0102 + timer * 4),
                eager.read16(0x0400_0102 + timer * 4)
            );
        }
        assert_eq!(lazy.timer_timing_state(), eager.timer_timing_state());
        assert_eq!(lazy.fresh_event_deadline(), eager.fresh_event_deadline());
        assert_eq!(lazy.read16(0x0400_0202), eager.read16(0x0400_0202));
        assert_eq!(lazy.ppu.state(), eager.ppu.state());
        assert_eq!(lazy.apu.save_state(), eager.apu.save_state());
    }

    assert!(saw_deferred_timer_cycles);
    assert!(!eager.timer_materialization_is_pending_for_test());
    assert!(lazy.apu.fifo_len(0) < 32);
    assert!(lazy.apu.fifo_len(1) < 32);
}

fn fill_obj_tiles(bus: &mut Bus, tile_count: u32, value: u8) {
    for offset in 0..(tile_count * 32) {
        poke_vram8(bus, 0x0601_0000 + offset, value);
    }
}

fn write_halfword_bits(bus: &mut Bus, addr: u32, bits: &[u8]) {
    for (index, bit) in bits.iter().enumerate() {
        bus.write16(addr + index as u32 * 2, u16::from(bit & 1));
    }
}

fn read_halfword_bits(bus: &Bus, addr: u32, count: usize) -> Vec<u8> {
    (0..count)
        .map(|index| (bus.read16(addr + index as u32 * 2) & 1) as u8)
        .collect()
}

fn eeprom_dma_write(bus: &mut Bus, source: u32, count: usize) {
    bus.write32(0x0400_00D4, source);
    bus.write32(0x0400_00D8, 0x0D00_0000);
    bus.write32(0x0400_00DC, 0x8000_0000 | count as u32);
}

fn eeprom_dma_read(bus: &mut Bus, destination: u32, count: usize) {
    bus.write32(0x0400_00D4, 0x0D00_0000);
    bus.write32(0x0400_00D8, destination);
    bus.write32(0x0400_00DC, 0x8000_0000 | count as u32);
}

fn finish_eeprom_write(bus: &mut Bus) {
    bus.step_cycles(EEPROM_WRITE_BUSY_CYCLES);
}

fn eeprom_address_bits(page: usize, width: usize) -> Vec<u8> {
    (0..width)
        .rev()
        .map(|shift| ((page >> shift) & 1) as u8)
        .collect()
}

fn eeprom_data_bits(bytes: [u8; 8]) -> Vec<u8> {
    bytes
        .into_iter()
        .flat_map(|byte| (0..8).rev().map(move |shift| (byte >> shift) & 1))
        .collect()
}

fn bytes_from_eeprom_data_bits(bits: &[u8]) -> [u8; 8] {
    let mut bytes = [0; 8];
    for (byte_index, byte) in bytes.iter_mut().enumerate() {
        for bit in &bits[byte_index * 8..byte_index * 8 + 8] {
            *byte = (*byte << 1) | (*bit & 1);
        }
    }
    bytes
}

#[test]
fn backup_region_mirrors_across_0e_and_0f_address_space() {
    let mut bus = Bus::new(sram_cartridge(), 48_000);

    bus.write8(0x0E00_0020, 0x42);

    assert_eq!(bus.read8(0x0E01_0020), 0x42);
    assert_eq!(bus.read8(0x0F00_0020), 0x42);
}

#[test]
fn native_width_reads_preserve_ram_and_video_mirroring() {
    let mut bus = Bus::new(cartridge(), 48_000);

    for addr in [
        0x0204_0000,
        0x0300_8000,
        0x0500_0400,
        0x0601_8000,
        0x0700_0400,
    ] {
        bus.write32(addr, 0x4433_2211);
        assert_eq!(bus.peek16(addr), 0x2211, "halfword at {addr:08X}");
        assert_eq!(bus.peek32(addr), 0x4433_2211, "word at {addr:08X}");
    }
}

#[test]
fn debug_trace_events_preserve_width_without_access_timing() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.debug_trace_enabled = true;
    bus.debug_trace_reads = true;
    bus.debug_trace_writes = true;

    bus.write16(0x0200_0000, 0xBEEF);
    assert_eq!(bus.read32(0x0200_0000), 0x0000_BEEF);

    let events = bus.debug_trace_events.into_inner();
    assert!(matches!(
        events.as_slice(),
        [
            DebugTraceEvent::Write {
                at: None,
                space: TraceWriteKind::Memory,
                addr: 0x0200_0000,
                width: TraceWriteWidth::Halfword,
                mapped_addr: None,
                ..
            },
            DebugTraceEvent::Read {
                at: None,
                space: TraceWriteKind::Memory,
                addr: 0x0200_0000,
                width: TraceWriteWidth::Word,
                mapped_addr: None,
                ..
            }
        ]
    ));
}

#[test]
fn backup_halfword_and_word_reads_repeat_selected_byte() {
    let mut bus = Bus::new(sram_cartridge(), 48_000);

    bus.write8(0x0E00_0040, 0x12);
    bus.write8(0x0E00_0041, 0x34);

    assert_eq!(bus.read16(0x0E00_0040), 0x1212);
    assert_eq!(bus.read32(0x0E00_0040), 0x1212_1212);
    assert_eq!(bus.read16(0x0E00_0041), 0x3434);
    assert_eq!(bus.read32(0x0E00_0041), 0x3434_3434);
}

#[test]
fn backup_halfword_and_word_writes_store_only_selected_byte() {
    let mut bus = Bus::new(sram_cartridge(), 48_000);

    bus.write16(0x0E00_0060, 0xAABB);
    assert_eq!(bus.read8(0x0E00_0060), 0xBB);
    assert_eq!(bus.read8(0x0E00_0061), 0xFF);

    bus.write16(0x0E00_0061, 0xAABB);
    assert_eq!(bus.read8(0x0E00_0061), 0xAA);
    assert_eq!(bus.read8(0x0E00_0062), 0xFF);

    bus.write32(0x0E00_0080, 0xAABB_CCDD);
    assert_eq!(bus.read8(0x0E00_0080), 0xDD);
    assert_eq!(bus.read8(0x0E00_0081), 0xFF);

    bus.write32(0x0E00_0081, 0xAABB_CCDD);
    assert_eq!(bus.read8(0x0E00_0081), 0xCC);
    assert_eq!(bus.read8(0x0E00_0082), 0xFF);

    bus.write32(0x0E00_0082, 0xAABB_CCDD);
    assert_eq!(bus.read8(0x0E00_0082), 0xBB);
    assert_eq!(bus.read8(0x0E00_0083), 0xFF);

    bus.write32(0x0E00_0083, 0xAABB_CCDD);
    assert_eq!(bus.read8(0x0E00_0083), 0xAA);
    assert_eq!(bus.read8(0x0E00_0084), 0xFF);
}

#[test]
fn emerald_gpio_uses_only_halfword_and_word_gamepak_writes() {
    let mut bus = Bus::new(emerald_cartridge(), 48_000);
    let disabled_control = bus.read16(0x0800_00C8);

    bus.write8(0x0800_00C8, 1);
    assert_eq!(bus.read16(0x0800_00C8), disabled_control);

    bus.write16(0x0800_00C8, 1);
    bus.write32(0x0800_00C4, 0x0007_0005);

    assert_eq!(bus.read16(0x0800_00C8), 1);
    assert_eq!(bus.read16(0x0800_00C6), 7);
}

#[path = "tests/bg_window.rs"]
mod bg_window;
#[path = "tests/dma_eeprom_audio.rs"]
mod dma_eeprom_audio;
#[path = "tests/interrupts_misc.rs"]
mod interrupts_misc;
#[path = "tests/memory_bitmap.rs"]
mod memory_bitmap;
#[path = "tests/obj_blend_affine.rs"]
mod obj_blend_affine;
