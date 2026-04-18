use crate::debug::{ConsoleGraphicsData, NesGraphicsData};

pub(super) fn nes_disasm_peek_byte(bus: &zeff_nes_core::hardware::bus::Bus, addr: u16) -> u8 {
    match addr {
        0x0000..=0x1FFF => bus.ram[(addr & 0x07FF) as usize],
        0x4020..=0xFFFF => bus.cartridge.cpu_peek(addr),
        _ => 0,
    }
}

pub(super) fn nes_graphics_snapshot(
    emu: &mut zeff_nes_core::emulator::Emulator,
    reusable_chr: Option<Vec<u8>>,
    reusable_nametable: Option<Vec<u8>>,
) -> ConsoleGraphicsData {
    let palette_mode = emu.palette_mode();
    let palette_ram = *emu.ppu_palette_ram();
    let ctrl = emu.ppu_ctrl();
    let scroll_t = emu.ppu_scroll_t();
    let fine_x = emu.ppu_fine_x();
    let mirroring = emu.bus().cartridge.mirroring();

    let bus = emu.bus_mut();

    let mut chr_data = reusable_chr.unwrap_or_else(|| vec![0u8; 0x2000]);
    chr_data.resize(0x2000, 0);
    for addr in 0..0x2000u16 {
        chr_data[addr as usize] = bus.cartridge.chr_read(addr);
    }

    let mut nametable_data = reusable_nametable.unwrap_or_else(|| vec![0u8; 0x1000]);
    nametable_data.resize(0x1000, 0);
    for offset in 0..0x1000u16 {
        let addr = 0x2000 + offset;
        nametable_data[offset as usize] = bus.ppu_bus_read(addr);
    }

    ConsoleGraphicsData::Nes(NesGraphicsData {
        chr_data,
        nametable_data,
        palette_ram,
        palette_mode,
        ctrl,
        mirroring,
        scroll_t,
        fine_x,
    })
}
