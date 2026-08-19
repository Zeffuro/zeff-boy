use super::*;
use crate::hardware::cartridge::{Sega8MapperKind, SystemHint};
use crate::hardware::constants::{
    CODEMASTERS_CARTRIDGE_RAM_SIZE, CODEMASTERS_HEADER_OFFSET, CODEMASTERS_HEADER_SIZE,
    IO_PORT_CONTROL, IO_PORT_GG_EXT_DATA, IO_PORT_GG_EXT_DIRECTION, IO_PORT_GG_SERIAL_CONTROL,
    IO_PORT_GG_SERIAL_RX, IO_PORT_GG_SERIAL_TX, MAPPER_FRAME_CONTROL,
    MAPPER_FRAME_CONTROL_CART_RAM_BANK_SELECT, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE,
    MAPPER_SLOT0_BANK, MAPPER_SLOT1_BANK, MAPPER_SLOT2_BANK, ROM_BANK_SIZE, ROM_PAGE_8K_SIZE,
    SG_WORK_RAM_SIZE, SMS_WORK_RAM_SIZE,
};

const CODEMASTERS_TEST_HEADER_BANK_COUNT: usize = 0x00;
const CODEMASTERS_TEST_HEADER_DAY: usize = 0x01;
const CODEMASTERS_TEST_HEADER_MONTH: usize = 0x02;
const CODEMASTERS_TEST_HEADER_YEAR: usize = 0x03;
const CODEMASTERS_TEST_HEADER_HOUR: usize = 0x04;
const CODEMASTERS_TEST_HEADER_MINUTE: usize = 0x05;
const CODEMASTERS_TEST_HEADER_CHECKSUM: usize = 0x06;
const CODEMASTERS_TEST_HEADER_COMPLEMENT: usize = 0x08;
const CODEMASTERS_TEST_HEADER_ZERO_PADDING_START: usize = 0x0A;

fn banked_rom(bank_count: usize) -> Vec<u8> {
    let mut rom = vec![0; bank_count * ROM_BANK_SIZE];
    for bank in 0..bank_count {
        rom[bank * ROM_BANK_SIZE..(bank + 1) * ROM_BANK_SIZE].fill(bank as u8);
    }
    rom
}

fn paged_rom_8k(page_count: usize) -> Vec<u8> {
    let mut rom = vec![0; page_count * ROM_PAGE_8K_SIZE];
    for page in 0..page_count {
        rom[page * ROM_PAGE_8K_SIZE..(page + 1) * ROM_PAGE_8K_SIZE].fill(page as u8);
    }
    rom
}

fn codemasters_banked_rom(bank_count: usize) -> Vec<u8> {
    let mut rom = banked_rom(bank_count);
    let offset = CODEMASTERS_HEADER_OFFSET;
    assert!(rom.len() >= offset + CODEMASTERS_HEADER_SIZE);
    rom[offset + CODEMASTERS_TEST_HEADER_BANK_COUNT] = bank_count as u8;
    rom[offset + CODEMASTERS_TEST_HEADER_DAY] = 0x31;
    rom[offset + CODEMASTERS_TEST_HEADER_MONTH] = 0x08;
    rom[offset + CODEMASTERS_TEST_HEADER_YEAR] = 0x93;
    rom[offset + CODEMASTERS_TEST_HEADER_HOUR] = 0x10;
    rom[offset + CODEMASTERS_TEST_HEADER_MINUTE] = 0x59;
    rom[offset + CODEMASTERS_TEST_HEADER_CHECKSUM..offset + CODEMASTERS_TEST_HEADER_CHECKSUM + 2]
        .copy_from_slice(&0x1234u16.to_le_bytes());
    rom[offset + CODEMASTERS_TEST_HEADER_COMPLEMENT
        ..offset + CODEMASTERS_TEST_HEADER_COMPLEMENT + 2]
        .copy_from_slice(&0xEDCCu16.to_le_bytes());
    rom[offset + CODEMASTERS_TEST_HEADER_ZERO_PADDING_START..offset + CODEMASTERS_HEADER_SIZE]
        .fill(0);
    rom
}

fn bus_with_banked_rom(bank_count: usize) -> Bus {
    let cart = Cartridge::load_with_hint(&banked_rom(bank_count), SystemHint::MasterSystem)
        .expect("banked ROM should load");
    Bus::new(cart)
}

fn game_gear_bus_with_banked_rom(bank_count: usize) -> Bus {
    let cart = Cartridge::load_with_hint(&banked_rom(bank_count), SystemHint::GameGear)
        .expect("banked ROM should load");
    Bus::new(cart)
}

fn sg1000_bus_with_banked_rom(bank_count: usize) -> Bus {
    let cart = Cartridge::load_with_hint(&banked_rom(bank_count), SystemHint::Sg1000)
        .expect("banked ROM should load");
    Bus::new(cart)
}

fn bus_with_codemasters_banked_rom(bank_count: usize) -> Bus {
    let cart = Cartridge::load_with_hint(
        &codemasters_banked_rom(bank_count),
        SystemHint::MasterSystem,
    )
    .expect("Codemasters banked ROM should load");
    Bus::new(cart)
}

fn bus_with_korean_banked_rom(bank_count: usize) -> Bus {
    let cart = Cartridge::load_with_hint_and_mapper_kind(
        &banked_rom(bank_count),
        SystemHint::MasterSystem,
        Some(Sega8MapperKind::Korean),
    )
    .expect("Korean banked ROM should load");
    Bus::new(cart)
}

fn bus_with_forced_mapper_paged_rom(mapper_kind: Sega8MapperKind, page_count: usize) -> Bus {
    let cart = Cartridge::load_with_hint_and_mapper_kind(
        &paged_rom_8k(page_count),
        SystemHint::MasterSystem,
        Some(mapper_kind),
    )
    .expect("forced mapper paged ROM should load");
    Bus::new(cart)
}

#[test]
fn default_mapper_exposes_first_three_rom_banks() {
    let bus = bus_with_banked_rom(4);

    assert_eq!(bus.cpu_read(0x0000), 0);
    assert_eq!(bus.cpu_read(0x0400), 0);
    assert_eq!(bus.cpu_read(0x4000), 1);
    assert_eq!(bus.cpu_read(0x8000), 2);
}

#[test]
fn write_trace_skips_reads() {
    let mut bus = bus_with_banked_rom(4);
    bus.begin_cpu_write_trace();

    bus.cpu_read(0);
    bus.io_read(IO_PORT_CONTROL);
    bus.cpu_write(0xC000, 0x12);
    bus.io_write(IO_PORT_CONTROL, 0x34);

    let events = bus.drain_cpu_access_trace();
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .all(|event| matches!(event, CpuAccessTraceEvent::Write { .. }))
    );
}

#[test]
fn rom_offset_tracks_sega_mapper_slots() {
    let mut bus = bus_with_banked_rom(4);
    let before = bus.rom_mapping_token();

    assert_eq!(bus.rom_offset_for_cpu_address(0x4000), Some(0x4000));
    bus.cpu_write(MAPPER_SLOT1_BANK, 3);

    assert_eq!(bus.rom_offset_for_cpu_address(0x4000), Some(0xC000));
    assert_ne!(bus.rom_mapping_token(), before);
}

#[test]
fn rom_offset_tracks_mapped_eight_kib_pages() {
    let mut bus = bus_with_forced_mapper_paged_rom(Sega8MapperKind::Msx, 8);
    bus.cpu_write(0x0002, 5);

    assert_eq!(bus.rom_offset_for_cpu_address(0x4000), Some(0xA000));
}

#[test]
fn mapper_registers_switch_slots_but_keep_first_kilobyte_fixed() {
    let mut bus = bus_with_banked_rom(4);

    bus.cpu_write(MAPPER_SLOT0_BANK, 3);
    bus.cpu_write(MAPPER_SLOT1_BANK, 2);
    bus.cpu_write(MAPPER_SLOT2_BANK, 1);

    assert_eq!(bus.cpu_read(0x0000), 0);
    assert_eq!(bus.cpu_read(0x0400), 3);
    assert_eq!(bus.cpu_read(0x4000), 2);
    assert_eq!(bus.cpu_read(0x8000), 1);
    assert_eq!(bus.mapper().slot_banks(), [3, 2, 1]);
}

#[test]
fn standard_sega_mapper_ignores_codemasters_register_addresses() {
    let mut bus = bus_with_banked_rom(4);

    bus.cpu_write(SLOT2_START, 3);

    assert_eq!(bus.mapper().kind(), Sega8MapperKind::Sega);
    assert_eq!(bus.mapper().slot_banks(), [0, 1, 2]);
    assert_eq!(bus.cpu_read(SLOT2_START), 2);
}

#[test]
fn codemasters_mapper_uses_detected_initial_banks() {
    let bus = bus_with_codemasters_banked_rom(4);

    assert_eq!(bus.mapper().kind(), Sega8MapperKind::Codemasters);
    assert_eq!(bus.mapper().kind_label(), "codemasters");
    assert_eq!(bus.mapper().slot_banks(), [0, 1, 0]);
    assert_eq!(bus.cpu_read(SLOT0_START), 0);
    assert_eq!(bus.cpu_read(SLOT1_START), 1);
    assert_eq!(bus.cpu_read(SLOT2_START), 0);
}

#[test]
fn codemasters_mapper_switches_all_three_slots_without_fixed_boot_window() {
    let mut bus = bus_with_codemasters_banked_rom(4);

    bus.cpu_write(SLOT0_START, 3);
    bus.cpu_write(SLOT1_START + 1, 2);
    bus.cpu_write(SLOT2_START + 2, 1);

    assert_eq!(bus.mapper().slot_banks(), [3, 2, 1]);
    assert_eq!(bus.cpu_read(SLOT0_START), 3);
    assert_eq!(bus.cpu_read(SLOT1_START), 2);
    assert_eq!(bus.cpu_read(SLOT2_START), 1);
}

#[test]
fn codemasters_high_slot1_bit_maps_ram_into_upper_half_of_slot2() {
    let mut bus = bus_with_codemasters_banked_rom(4);

    bus.cpu_write(SLOT1_START, 0x82);

    assert_eq!(bus.mapper().slot1_bank(), 2);
    assert!(bus.mapper().codemasters_cartridge_ram_enabled());
    assert_eq!(bus.cpu_read(SLOT1_START), 2);
    assert_eq!(bus.cpu_read(SLOT2_START), 0);
    assert_eq!(
        bus.cartridge_ram_visible().len(),
        CODEMASTERS_CARTRIDGE_RAM_SIZE
    );

    bus.cpu_write(0xA000, 0x5A);

    assert_eq!(bus.cpu_read(0xA000), 0x5A);
    assert_eq!(bus.cartridge_ram_visible()[0], 0x5A);
    assert_eq!(
        bus.mapper().slot_banks(),
        [0, 0x82, 0],
        "writing mapped RAM must not also switch the slot-2 ROM bank"
    );

    bus.cpu_write(SLOT2_START, 3);

    assert_eq!(bus.cpu_read(SLOT2_START), 3);
    assert_eq!(bus.cpu_read(0xA000), 0x5A);

    bus.cpu_write(SLOT1_START, 0x02);

    assert!(!bus.mapper().codemasters_cartridge_ram_enabled());
    assert_eq!(bus.cpu_read(0xA000), 3);

    bus.cpu_write(SLOT1_START, 0x82);

    assert_eq!(bus.cpu_read(0xA000), 0x5A);
}

#[test]
fn korean_mapper_switches_slot2_with_a000_register_only() {
    let mut bus = bus_with_korean_banked_rom(4);

    assert_eq!(bus.mapper().kind(), Sega8MapperKind::Korean);
    assert_eq!(bus.cpu_read(SLOT0_START), 0);
    assert_eq!(bus.cpu_read(SLOT1_START), 1);
    assert_eq!(bus.cpu_read(SLOT2_START), 2);

    bus.cpu_write(0xA000, 3);

    assert_eq!(bus.mapper().slot_banks(), [0, 1, 3]);
    assert_eq!(bus.cpu_read(SLOT0_START), 0);
    assert_eq!(bus.cpu_read(SLOT1_START), 1);
    assert_eq!(bus.cpu_read(SLOT2_START), 3);

    bus.cpu_write(MAPPER_SLOT1_BANK, 2);
    bus.cpu_write(MAPPER_SLOT2_BANK, 1);

    assert_eq!(bus.mapper().slot_banks(), [0, 1, 3]);
    assert_eq!(bus.cpu_read(SLOT2_START), 3);
}

#[test]
fn msx_mapper_uses_four_switchable_eight_kib_pages_after_fixed_first_16k() {
    let mut bus = bus_with_forced_mapper_paged_rom(Sega8MapperKind::Msx, 8);

    assert_eq!(bus.mapper().kind(), Sega8MapperKind::Msx);
    assert_eq!(bus.cpu_read(0x0000), 0);
    assert_eq!(bus.cpu_read(0x2000), 1);
    assert_eq!(bus.cpu_read(0x4000), 2);
    assert_eq!(bus.cpu_read(0x6000), 3);
    assert_eq!(bus.cpu_read(0x8000), 4);
    assert_eq!(bus.cpu_read(0xA000), 5);

    bus.cpu_write(0x0000, 7);
    bus.cpu_write(0x0001, 6);
    bus.cpu_write(0x0002, 5);
    bus.cpu_write(0x0003, 4);

    assert_eq!(bus.cpu_read(0x4000), 5);
    assert_eq!(bus.cpu_read(0x6000), 4);
    assert_eq!(bus.cpu_read(0x8000), 7);
    assert_eq!(bus.cpu_read(0xA000), 6);
}

#[test]
fn nemesis_mapper_fixes_first_page_to_last_rom_page() {
    let mut bus = bus_with_forced_mapper_paged_rom(Sega8MapperKind::Nemesis, 8);

    assert_eq!(bus.mapper().kind(), Sega8MapperKind::Nemesis);
    assert_eq!(bus.cpu_read(0x0000), 7);
    assert_eq!(bus.cpu_read(0x2000), 1);
    assert_eq!(bus.cpu_read(0x4000), 2);
    assert_eq!(bus.cpu_read(0x6000), 3);
    assert_eq!(bus.cpu_read(0x8000), 4);
    assert_eq!(bus.cpu_read(0xA000), 5);

    bus.cpu_write(0x0000, 0);
    bus.cpu_write(0x0001, 2);
    bus.cpu_write(0x0002, 3);
    bus.cpu_write(0x0003, 4);

    assert_eq!(bus.cpu_read(0x0000), 7);
    assert_eq!(bus.cpu_read(0x2000), 1);
    assert_eq!(bus.cpu_read(0x4000), 3);
    assert_eq!(bus.cpu_read(0x6000), 4);
    assert_eq!(bus.cpu_read(0x8000), 0);
    assert_eq!(bus.cpu_read(0xA000), 2);
}

#[test]
fn janggun_mapper_supports_8k_pages_and_bit_reversed_reads() {
    let mut bus = bus_with_forced_mapper_paged_rom(Sega8MapperKind::Janggun, 16);

    assert_eq!(bus.mapper().kind(), Sega8MapperKind::Janggun);
    assert_eq!(bus.cpu_read(0x0000), 0);
    assert_eq!(bus.cpu_read(0x2000), 1);
    assert_eq!(bus.cpu_read(0x4000), 2);
    assert_eq!(bus.cpu_read(0x6000), 3);
    assert_eq!(bus.cpu_read(0x8000), 4);
    assert_eq!(bus.cpu_read(0xA000), 5);

    bus.cpu_write(0x4000, 0x46);
    bus.cpu_write(0x6000, 0x07);
    bus.cpu_write(0x8000, 0x48);
    bus.cpu_write(0xA000, 0x09);

    assert_eq!(bus.cpu_read(0x4000), 6u8.reverse_bits());
    assert_eq!(bus.cpu_read(0x6000), 7);
    assert_eq!(bus.cpu_read(0x8000), 8u8.reverse_bits());
    assert_eq!(bus.cpu_read(0xA000), 9);
}

#[test]
fn janggun_mapper_fffe_ffff_switch_16k_pairs_and_remain_ram_backed() {
    let mut bus = bus_with_forced_mapper_paged_rom(Sega8MapperKind::Janggun, 16);

    bus.cpu_write(0xFFFE, 0x42);
    bus.cpu_write(0xFFFF, 0x04);

    assert_eq!(bus.cpu_read(0x4000), 2u8.reverse_bits());
    assert_eq!(bus.cpu_read(0x6000), 3u8.reverse_bits());
    assert_eq!(bus.cpu_read(0x8000), 4);
    assert_eq!(bus.cpu_read(0xA000), 5);
    assert_eq!(bus.cpu_read(0xFFFE), 0x42);
    assert_eq!(bus.cpu_read(0xFFFF), 0x04);
}

#[test]
fn work_ram_is_mirrored_and_mapper_registers_are_ram_backed() {
    let mut bus = bus_with_banked_rom(4);

    bus.cpu_write(0xC123, 0x5A);
    assert_eq!(bus.cpu_read(0xC123), 0x5A);
    assert_eq!(bus.cpu_read(0xE123), 0x5A);

    bus.cpu_write(MAPPER_FRAME_CONTROL, 0x08);
    assert_eq!(bus.cpu_read(MAPPER_FRAME_CONTROL), 0x08);
    assert_eq!(bus.mapper().frame_control(), 0x08);
}

#[test]
fn sg1000_work_ram_is_one_kilobyte_mirrored_through_c000_ffff() {
    let mut bus = sg1000_bus_with_banked_rom(4);

    assert_eq!(bus.work_ram().len(), SG_WORK_RAM_SIZE);

    bus.cpu_write(0xC123, 0x5A);

    assert_eq!(bus.cpu_read(0xC123), 0x5A);
    assert_eq!(bus.cpu_read(0xC523), 0x5A);
    assert_eq!(bus.cpu_read(0xD123), 0x5A);
    assert_eq!(bus.cpu_read(0xE123), 0x5A);
    assert_eq!(bus.cpu_read(0xFD23), 0x5A);
}

#[test]
fn sg1000_type_b_extension_maps_eight_kilobytes_at_c000() {
    let mut bus = sg1000_bus_with_banked_rom(4);
    bus.sg_type_b_ram_extension = true;

    assert_eq!(bus.work_ram().len(), SMS_WORK_RAM_SIZE);

    bus.cpu_write(0xC123, 0x5A);
    bus.cpu_write(0xC523, 0xA5);

    assert_eq!(bus.cpu_read(0xC123), 0x5A);
    assert_eq!(bus.cpu_read(0xC523), 0xA5);
    assert_eq!(bus.cpu_read(0xE123), 0x5A);
    assert_eq!(bus.cpu_read(0xE523), 0xA5);
}

#[test]
fn sg1000_work_ram_mirror_does_not_write_sms_mapper_registers() {
    let mut bus = sg1000_bus_with_banked_rom(4);

    bus.cpu_write(MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);
    bus.cpu_write(MAPPER_SLOT0_BANK, 3);
    bus.cpu_write(MAPPER_SLOT1_BANK, 2);
    bus.cpu_write(MAPPER_SLOT2_BANK, 1);

    assert_eq!(bus.mapper().frame_control(), 0);
    assert_eq!(bus.mapper().slot_banks(), [0, 1, 2]);
    assert_eq!(
        bus.cpu_read(MAPPER_FRAME_CONTROL),
        MAPPER_FRAME_CONTROL_CART_RAM_ENABLE
    );
    assert_eq!(bus.cpu_read(0xC3FC), MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);
    assert_eq!(bus.cpu_read(0xC3FD), 3);
    assert_eq!(bus.cpu_read(0xC3FE), 2);
    assert_eq!(bus.cpu_read(0xC3FF), 1);
}

#[test]
fn sega_mapper_can_map_cartridge_ram_into_slot2() {
    let mut bus = bus_with_banked_rom(4);

    assert_eq!(bus.cpu_read(0x8000), 2);
    bus.cpu_write(MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);
    bus.cpu_write(0x8000, 0x5A);

    assert!(bus.mapper().slot2_cartridge_ram_enabled());
    assert_eq!(bus.cpu_read(0x8000), 0x5A);
    assert_eq!(bus.cartridge_ram()[0], 0x5A);

    bus.cpu_write(MAPPER_FRAME_CONTROL, 0);

    assert!(!bus.mapper().slot2_cartridge_ram_enabled());
    assert_eq!(bus.cpu_read(0x8000), 2);
}

#[test]
fn sega_mapper_cartridge_ram_bank_select_switches_slot2_ram_page() {
    let mut bus = bus_with_banked_rom(4);

    bus.cpu_write(MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);
    bus.cpu_write(0x8000, 0x11);
    bus.cpu_write(
        MAPPER_FRAME_CONTROL,
        MAPPER_FRAME_CONTROL_CART_RAM_ENABLE | MAPPER_FRAME_CONTROL_CART_RAM_BANK_SELECT,
    );
    bus.cpu_write(0x8000, 0x22);

    assert_eq!(bus.mapper().cartridge_ram_bank(), 1);
    assert_eq!(bus.cpu_read(0x8000), 0x22);

    bus.cpu_write(MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);

    assert_eq!(bus.mapper().cartridge_ram_bank(), 0);
    assert_eq!(bus.cpu_read(0x8000), 0x11);
}

#[test]
fn rom_patches_apply_to_cpu_rom_reads() {
    let mut bus = bus_with_banked_rom(4);

    assert_eq!(bus.cpu_read(0x4001), 1);
    bus.add_rom_patch(zeff_emu_common::cheats::CheatPatch::RomWrite {
        address: 0x4001,
        value: zeff_emu_common::cheats::CheatValue::Constant(0xAA),
    });

    assert_eq!(bus.cpu_read(0x4001), 0xAA);
    assert_eq!(bus.cpu_read_raw(0x4001), 1);
}

#[test]
fn conditional_rom_patches_compare_unpatched_rom_byte() {
    let mut bus = bus_with_banked_rom(4);

    bus.add_rom_patch(zeff_emu_common::cheats::CheatPatch::RomWriteIfEquals {
        address: 0x4001,
        value: zeff_emu_common::cheats::CheatValue::Constant(0xAA),
        compare: zeff_emu_common::cheats::CheatValue::Constant(0x02),
    });
    assert_eq!(bus.cpu_read(0x4001), 1);

    bus.clear_rom_patches();
    bus.add_rom_patch(zeff_emu_common::cheats::CheatPatch::RomWriteIfEquals {
        address: 0x4001,
        value: zeff_emu_common::cheats::CheatValue::Constant(0xAA),
        compare: zeff_emu_common::cheats::CheatValue::Constant(0x01),
    });
    assert_eq!(bus.cpu_read(0x4001), 0xAA);
}

#[test]
fn rom_patches_do_not_override_mapped_cartridge_ram() {
    let mut bus = bus_with_banked_rom(4);

    bus.add_rom_patch(zeff_emu_common::cheats::CheatPatch::RomWrite {
        address: 0x8000,
        value: zeff_emu_common::cheats::CheatValue::Constant(0xAA),
    });
    assert_eq!(bus.cpu_read(0x8000), 0xAA);

    bus.cpu_write(MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);
    bus.cpu_write(0x8000, 0x5A);

    assert_eq!(bus.cpu_read(0x8000), 0x5A);
}

#[test]
fn rom_patches_do_not_override_codemasters_mapped_ram() {
    let mut bus = bus_with_codemasters_banked_rom(4);

    bus.add_rom_patch(zeff_emu_common::cheats::CheatPatch::RomWrite {
        address: 0xA000,
        value: zeff_emu_common::cheats::CheatValue::Constant(0xAA),
    });

    assert_eq!(bus.cpu_read(0xA000), 0xAA);

    bus.cpu_write(SLOT1_START, 0x80);
    bus.cpu_write(0xA000, 0x5A);

    assert_eq!(bus.cpu_read(0xA000), 0x5A);
}

#[test]
fn reset_restores_mapper_and_clears_ram() {
    let mut bus = bus_with_banked_rom(4);
    bus.cpu_write(MAPPER_SLOT1_BANK, 3);
    bus.cpu_write(0xC000, 0xAA);
    bus.cpu_write(MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);
    bus.cpu_write(0x8000, 0x5A);
    bus.io_write(IO_PORT_PSG, 0x9F);
    bus.io_write(IO_PORT_VDP_CONTROL, 0xE0);
    bus.io_write(IO_PORT_VDP_CONTROL, 0x80);
    bus.add_rom_patch(zeff_emu_common::cheats::CheatPatch::RomWrite {
        address: 0,
        value: zeff_emu_common::cheats::CheatValue::Constant(0xAA),
    });

    bus.reset();

    assert_eq!(bus.mapper().slot_banks(), [0, 1, 2]);
    assert_eq!(bus.cpu_read(0xC000), 0);
    assert_eq!(bus.cartridge_ram()[0], 0x5A);
    assert_eq!(bus.apu().last_write(), None);
    assert_eq!(bus.vdp().registers()[0], 0);
    assert!(bus.rom_patches().is_empty());
}

#[test]
fn reset_preserves_codemasters_mapper_kind_and_default_banks() {
    let mut bus = bus_with_codemasters_banked_rom(4);
    bus.cpu_write(SLOT2_START, 3);

    bus.reset();

    assert_eq!(bus.mapper().kind(), Sega8MapperKind::Codemasters);
    assert_eq!(bus.mapper().slot_banks(), [0, 1, 0]);
    assert_eq!(bus.cpu_read(SLOT2_START), 0);
}

#[test]
fn io_ports_route_to_vdp_psg_and_controllers() {
    let mut bus = bus_with_banked_rom(4);

    bus.io_write(IO_PORT_VDP_CONTROL, 0x34);
    bus.io_write(IO_PORT_VDP_CONTROL, 0x41);
    bus.io_write(IO_PORT_VDP_DATA, 0xAA);
    bus.io_write(IO_PORT_PSG, 0x9F);
    bus.input_mut()
        .set_controller_raw(ControllerPort::One, 0xF7);

    assert_eq!(bus.vdp().vram()[0x0134], 0xAA);
    assert_eq!(bus.apu().last_write(), Some(0x9F));
    assert_eq!(bus.io_read(IO_PORT_CONTROLLER_1), 0xF7);
}

#[test]
fn export_sms_region_detector_reads_back_th_outputs_from_port_3f() {
    let mut bus = bus_with_banked_rom(4);
    bus.set_console_region(Sega8Region::Export);

    bus.io_write(IO_PORT_CONTROL, 0xF5);
    assert_eq!(bus.io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0xC0);

    bus.io_write(IO_PORT_CONTROL, 0x55);
    assert_eq!(bus.io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0x00);
}

#[test]
fn japanese_sms_region_detector_does_not_read_back_th_outputs() {
    let mut bus = bus_with_banked_rom(4);
    bus.set_console_region(Sega8Region::Japanese);
    bus.input_mut()
        .set_controller_raw(ControllerPort::Two, 0xC0);

    bus.io_write(IO_PORT_CONTROL, 0x55);

    assert_eq!(bus.io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0xC0);
}

#[test]
fn japanese_power_base_converter_region_detector_inverts_th_outputs() {
    let mut bus = bus_with_banked_rom(4);
    bus.set_console_region(Sega8Region::JapanesePowerBaseConverter);

    bus.io_write(IO_PORT_CONTROL, 0xF5);
    assert_eq!(bus.io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0x00);

    bus.io_write(IO_PORT_CONTROL, 0x55);
    assert_eq!(bus.io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0xC0);
}

#[test]
fn game_gear_start_port_reports_region_bit() {
    let cart = Cartridge::load_with_hint(&[0x00], SystemHint::GameGear)
        .expect("Game Gear ROM should load");
    let mut bus = Bus::new(cart);

    bus.set_console_region(Sega8Region::Export);
    assert_eq!(bus.io_read(IO_PORT_GG_START) & 0x40, 0x40);

    bus.set_console_region(Sega8Region::Japanese);
    assert_eq!(bus.io_read(IO_PORT_GG_START) & 0x40, 0x00);

    bus.set_console_region(Sega8Region::JapanesePowerBaseConverter);
    assert_eq!(bus.io_read(IO_PORT_GG_START) & 0x40, 0x00);
}

#[test]
fn psg_write_mirrors_and_game_gear_stereo_port_are_decoded() {
    let mut bus = bus_with_banked_rom(4);

    bus.io_write(0x40, 0x90);
    bus.io_write(0x7E, 0x80);

    assert_eq!(bus.apu().last_write(), Some(0x80));
    assert_eq!(bus.apu().write_count(), 2);

    let mut gg = game_gear_bus_with_banked_rom(4);
    gg.io_write(IO_PORT_GG_PSG_STEREO, 0x10);

    assert_eq!(gg.apu().stereo_control(), 0x10);
}

#[test]
fn game_gear_serial_status_port_reports_idle_disconnected_link() {
    let mut bus = game_gear_bus_with_banked_rom(4);

    bus.io_write(IO_PORT_GG_SERIAL_CONTROL, 0x30);

    assert_eq!(bus.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x01, 0);
    assert_eq!(bus.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x02, 0);
    assert_ne!(bus.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x04, 0);
}

#[test]
fn game_gear_serial_sync_transfers_pending_tx_to_peer_rx() {
    let mut left = game_gear_bus_with_banked_rom(4);
    let mut right = game_gear_bus_with_banked_rom(4);

    left.io_write(IO_PORT_GG_SERIAL_CONTROL, 0x30);
    right.io_write(IO_PORT_GG_SERIAL_CONTROL, 0x30);
    left.io_write(IO_PORT_GG_SERIAL_TX, 0x5A);
    left.sync_game_gear_link_peer(&mut right);

    assert_ne!(left.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x01, 0);
    assert_eq!(right.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x02, 0);

    left.step_cycles(20_000);
    right.step_cycles(20_000);
    left.sync_game_gear_link_peer(&mut right);

    assert_eq!(left.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x05, 0);
    assert_ne!(right.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x02, 0);
    assert_eq!(right.io_read(IO_PORT_GG_SERIAL_RX), 0x5A);
    assert_eq!(right.io_read(IO_PORT_GG_SERIAL_CONTROL) & 0x02, 0);
}

#[test]
fn game_gear_parallel_ext_inputs_can_read_peer_outputs() {
    let mut left = game_gear_bus_with_banked_rom(4);
    let mut right = game_gear_bus_with_banked_rom(4);

    left.io_write(IO_PORT_GG_EXT_DIRECTION, 0x7F);
    right.io_write(IO_PORT_GG_EXT_DIRECTION, 0x00);
    right.io_write(IO_PORT_GG_EXT_DATA, 0x2A);
    left.sync_game_gear_link_peer(&mut right);

    assert_eq!(left.io_read(IO_PORT_GG_EXT_DATA), 0x9A);
}

#[test]
fn sms_low_odd_ports_mirror_io_control_write() {
    let mut bus = bus_with_banked_rom(4);
    bus.set_console_region(Sega8Region::Export);

    bus.io_write(0x01, 0x55);

    assert_eq!(bus.io_read(IO_PORT_CONTROLLER_2) & 0xC0, 0x00);
}

#[test]
fn sms_memory_control_can_hide_and_restore_work_ram() {
    let mut bus = bus_with_banked_rom(4);

    bus.cpu_write(0xC000, 0x5A);
    assert_eq!(bus.cpu_read(0xC000), 0x5A);

    bus.io_write(IO_PORT_MEMORY_CONTROL, MEMORY_CONTROL_WORK_RAM_DISABLE);
    assert_eq!(bus.memory_control(), MEMORY_CONTROL_WORK_RAM_DISABLE);
    assert_eq!(bus.cpu_read(0xC000), IO_OPEN_BUS_VALUE);
    bus.cpu_write(0xC000, 0xA5);
    assert_eq!(bus.cpu_read(0xC000), IO_OPEN_BUS_VALUE);

    bus.io_write(IO_PORT_MEMORY_CONTROL, MEMORY_CONTROL_DEFAULT);
    assert_eq!(bus.cpu_read(0xC000), 0x5A);
}

#[test]
fn sms_memory_control_can_hide_cartridge_without_losing_mapper_state() {
    let mut bus = bus_with_banked_rom(4);

    bus.cpu_write(MAPPER_SLOT1_BANK, 3);
    assert_eq!(bus.cpu_read(SLOT1_START), 3);

    bus.io_write(IO_PORT_MEMORY_CONTROL, MEMORY_CONTROL_CARTRIDGE_DISABLE);
    assert_eq!(bus.cpu_read(SLOT1_START), IO_OPEN_BUS_VALUE);
    bus.cpu_write(MAPPER_SLOT1_BANK, 2);
    assert_eq!(bus.mapper().slot1_bank(), 3);

    bus.io_write(IO_PORT_MEMORY_CONTROL, MEMORY_CONTROL_DEFAULT);
    assert_eq!(bus.cpu_read(SLOT1_START), 3);
}

#[test]
fn sms_memory_control_can_disable_io_but_still_accept_port_3e_writes() {
    let mut bus = bus_with_banked_rom(4);
    bus.input_mut()
        .set_controller_raw(ControllerPort::One, 0xEE);

    bus.io_write(IO_PORT_MEMORY_CONTROL, MEMORY_CONTROL_IO_DISABLE);
    assert_eq!(bus.io_read(IO_PORT_CONTROLLER_1), IO_OPEN_BUS_VALUE);
    bus.io_write(IO_PORT_CONTROL, 0x55);
    assert_eq!(bus.input().io_control(), IO_CONTROL_DEFAULT);

    bus.io_write(IO_PORT_MEMORY_CONTROL, MEMORY_CONTROL_DEFAULT);
    assert_eq!(bus.io_read(IO_PORT_CONTROLLER_1), 0xEE);
}

#[test]
fn sms_low_even_ports_mirror_memory_control_write() {
    let mut bus = bus_with_banked_rom(4);

    bus.io_write(0x00, MEMORY_CONTROL_WORK_RAM_DISABLE);

    assert_eq!(bus.memory_control(), MEMORY_CONTROL_WORK_RAM_DISABLE);
    assert_eq!(bus.cpu_read(0xC000), IO_OPEN_BUS_VALUE);
}

#[test]
fn non_sms_systems_ignore_sms_memory_control_port() {
    let mut gg = game_gear_bus_with_banked_rom(4);
    let mut sg = sg1000_bus_with_banked_rom(4);

    gg.cpu_write(0xC000, 0x5A);
    gg.io_write(IO_PORT_MEMORY_CONTROL, MEMORY_CONTROL_WORK_RAM_DISABLE);
    assert_eq!(gg.memory_control(), MEMORY_CONTROL_DEFAULT);
    assert_eq!(gg.cpu_read(0xC000), 0x5A);

    sg.cpu_write(0xC000, 0xA5);
    sg.io_write(IO_PORT_MEMORY_CONTROL, MEMORY_CONTROL_WORK_RAM_DISABLE);
    assert_eq!(sg.memory_control(), MEMORY_CONTROL_DEFAULT);
    assert_eq!(sg.cpu_read(0xC000), 0xA5);
}

#[test]
fn counter_and_controller_read_mirrors_are_decoded() {
    let mut bus = bus_with_banked_rom(4);
    bus.step_cycles(crate::hardware::constants::SMS_SCANLINE_Z80_CYCLES);
    bus.input_mut()
        .set_controller_raw(ControllerPort::One, 0xEE);
    bus.input_mut()
        .set_controller_raw(ControllerPort::Two, 0xDD);

    assert_eq!(bus.io_read(0x40), bus.vdp().v_counter());
    assert_eq!(bus.io_read(0x41), bus.vdp().h_counter());
    assert_eq!(bus.io_read(0xC0), 0xEE);
    assert_eq!(bus.io_read(0xC1), 0xDD);
}

#[test]
fn mirrored_vdp_ports_are_decoded() {
    let mut bus = bus_with_banked_rom(4);

    bus.io_write(0x81, 0x02);
    bus.io_write(0x81, 0x40);
    bus.io_write(0x80, 0x55);

    assert_eq!(bus.vdp().vram()[0x0002], 0x55);
}

#[test]
fn stepping_bus_advances_vdp_timing() {
    let mut bus = bus_with_banked_rom(4);
    bus.io_write(IO_PORT_PSG, 0x90);

    bus.step_cycles(crate::hardware::constants::SMS_SCANLINE_Z80_CYCLES);

    assert_eq!(bus.vdp().scanline(), 1);
    assert_eq!(bus.io_read(IO_PORT_V_COUNTER), 1);
    assert!(bus.apu().buffered_sample_count() > 0);
}

#[test]
fn maskable_interrupt_line_follows_vdp_frame_interrupt() {
    let mut bus = bus_with_banked_rom(4);

    bus.vdp_mut()
        .set_status_bits(crate::hardware::constants::VDP_STATUS_VBLANK);
    assert!(!bus.maskable_interrupt_pending());

    bus.io_write(
        IO_PORT_VDP_CONTROL,
        crate::hardware::constants::VDP_REG1_FRAME_IRQ_ENABLE,
    );
    bus.io_write(
        IO_PORT_VDP_CONTROL,
        crate::hardware::constants::VDP_CONTROL_REGISTER_WRITE_VALUE
            | crate::hardware::constants::VDP_REGISTER_MODE_CONTROL_2 as u8,
    );
    assert!(bus.maskable_interrupt_pending());

    assert_ne!(bus.io_read(IO_PORT_VDP_CONTROL), 0);
    assert!(!bus.maskable_interrupt_pending());
}
