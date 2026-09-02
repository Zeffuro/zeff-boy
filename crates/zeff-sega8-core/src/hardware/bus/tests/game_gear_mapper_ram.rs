use super::*;
use crate::hardware::cartridge::GameGearStandardMapperRam;

#[test]
fn known_absent_reads_open_bus_and_ignores_writes() {
    let cart = Cartridge::load_with_hint_mapper_and_game_gear_ram(
        &banked_rom(4),
        SystemHint::GameGear,
        None,
        Some(GameGearStandardMapperRam::Absent),
    )
    .unwrap();
    let mut bus = Bus::new(cart);

    bus.cpu_write(MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);
    bus.cpu_write(0x8000, 0x5A);

    assert_eq!(bus.cpu_read(0x8000), IO_OPEN_BUS_VALUE);
    assert_eq!(bus.cartridge_ram_visible(), []);
}

#[test]
fn known_battery_ram_uses_its_exact_visible_size() {
    let cart = Cartridge::load_with_hint_mapper_and_game_gear_ram(
        &banked_rom(4),
        SystemHint::GameGear,
        None,
        Some(GameGearStandardMapperRam::BatteryBacked8KiB),
    )
    .unwrap();
    let mut bus = Bus::new(cart);
    let saved = vec![0xA5; 8 * 1024];
    bus.load_cartridge_ram(&saved).unwrap();

    bus.cpu_write(MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_ENABLE);
    assert_eq!(bus.cpu_read(0x8000), 0xA5);
    assert_eq!(bus.cartridge_ram_visible(), saved);
    assert!(bus.load_cartridge_ram(&vec![0; 32 * 1024]).is_err());
}
