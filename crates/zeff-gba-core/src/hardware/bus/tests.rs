use crate::hardware::cartridge::EEPROM_WRITE_BUSY_CYCLES;

use super::*;
use crate::hardware::cartridge::RomHeader;
use crate::hardware::constants::{CYCLES_PER_FRAME, SCREEN_WIDTH};

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
