use super::*;
use crate::hardware::bus::Bus;
use crate::hardware::cartridge::Cartridge;
fn bus_with_rom(rom_body: &[u8]) -> Bus {
    let mut rom = vec![0; 0xC0.max(rom_body.len())];
    rom[..rom_body.len()].copy_from_slice(rom_body);
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xB2] = 0x96;
    Bus::new(Cartridge::load(&rom).unwrap(), 48_000)
}

#[path = "tests/arm.rs"]
mod arm;
#[path = "tests/fetch_memory.rs"]
mod fetch_memory;
#[path = "tests/frame_direct.rs"]
mod frame_direct;
#[path = "tests/frame_direct_block.rs"]
mod frame_direct_block;
#[path = "tests/frame_fetch.rs"]
mod frame_fetch;
#[path = "tests/frame_fetch_ram.rs"]
mod frame_fetch_ram;
#[path = "tests/frame_mixed.rs"]
mod frame_mixed;
#[path = "tests/frame_multiply.rs"]
mod frame_multiply;
#[path = "tests/frame_pure.rs"]
mod frame_pure;
#[path = "tests/frame_thumb.rs"]
mod frame_thumb;
#[path = "tests/irq.rs"]
mod irq;
#[path = "tests/phases.rs"]
mod phases;
#[path = "tests/swi.rs"]
mod swi;
#[path = "tests/thumb.rs"]
mod thumb;
#[path = "tests/timing.rs"]
mod timing;
