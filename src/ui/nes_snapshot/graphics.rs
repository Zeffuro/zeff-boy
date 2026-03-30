use crate::debug::{ConsoleGraphicsData, DisassemblyView, NesGraphicsData, nes_disassemble_around};

pub(super) fn nes_disassembly_view(emu: &zeff_nes_core::emulator::Emulator) -> DisassemblyView {
    let mut breakpoints: Vec<u16> = emu.iter_breakpoints().collect();
    breakpoints.sort_unstable();
    DisassemblyView {
        pc: emu.cpu_pc(),
        lines: nes_disassemble_around(
            |addr| nes_disasm_peek_byte(emu.bus(), addr),
            emu.cpu_pc(),
            12,
            26,
        ),
        breakpoints,
    }
}

fn nes_disasm_peek_byte(bus: &zeff_nes_core::hardware::bus::Bus, addr: u16) -> u8 {
    match addr {
        0x0000..=0x1FFF => bus.ram[(addr & 0x07FF) as usize],
        0x4020..=0xFFFF => bus.cartridge.cpu_read(addr),
        _ => 0,
    }
}

pub(super) fn nes_graphics_snapshot(
    emu: &zeff_nes_core::emulator::Emulator,
) -> ConsoleGraphicsData {
    let bus = emu.bus();

    let mut chr_data = vec![0u8; 0x2000];
    for addr in 0..0x2000u16 {
        chr_data[addr as usize] = bus.cartridge.chr_read(addr);
    }

    let mut nametable_data = vec![0u8; 0x1000];
    for offset in 0..0x1000u16 {
        let addr = 0x2000 + offset;
        nametable_data[offset as usize] = bus.ppu_bus_read(addr);
    }

    let palette_ram = *emu.ppu_palette_ram();

    ConsoleGraphicsData::Nes(NesGraphicsData {
        chr_data,
        nametable_data,
        palette_ram,
        ctrl: emu.ppu_ctrl(),
        mirroring: bus.cartridge.mirroring(),
        scroll_t: emu.ppu_scroll_t(),
        fine_x: emu.ppu_fine_x(),
    })
}

