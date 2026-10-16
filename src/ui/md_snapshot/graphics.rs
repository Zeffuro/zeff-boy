use crate::debug::{
    ConsoleGraphicsData, MdGraphicsData, PaletteDebugInfo, PaletteGroupDebug, PaletteRowDebug,
};
use zeff_md_core::Emulator;

pub(super) fn snapshot(emu: &Emulator) -> ConsoleGraphicsData {
    let registers = emu.vdp().registers().registers();
    ConsoleGraphicsData::Md(Box::new(MdGraphicsData {
        vram: emu.vdp().memory().vram().to_vec(),
        // Canonical backing words in bus byte order, without port-read FIFO history.
        cram: emu
            .vdp()
            .memory()
            .cram()
            .iter()
            .flat_map(|word| word.to_be_bytes())
            .collect(),
        vsram: emu
            .vdp()
            .memory()
            .vsram()
            .iter()
            .flat_map(|word| word.to_be_bytes())
            .collect(),
        palette: colors(emu),
        tile_height: tile_height(registers[1], registers[12]),
    }))
}

fn tile_height(mode: Option<u8>, size: Option<u8>) -> Option<usize> {
    if mode? & 0x84 != 4 {
        return None;
    }
    match size? & 6 {
        0 => Some(8),
        6 => Some(16),
        _ => None,
    }
}

pub(super) fn palette(emu: &Emulator) -> PaletteDebugInfo {
    let colors = colors(emu);
    PaletteDebugInfo {
        groups: vec![PaletteGroupDebug {
            title: "Mega Drive CRAM (normal intensity)".into(),
            rows: colors
                .as_chunks::<16>()
                .0
                .iter()
                .enumerate()
                .map(|(index, colors)| PaletteRowDebug {
                    label: format!("Palette {index}"),
                    colors: colors.to_vec(),
                })
                .collect(),
        }],
    }
}

fn colors(emu: &Emulator) -> [[u8; 4]; 64] {
    std::array::from_fn(|index| rgba(emu.vdp().memory().cram()[index]))
}

fn rgba(color: u16) -> [u8; 4] {
    let channel = |shift: u32| (((color >> shift) & 7_u16) * 255 / 7) as u8;
    [channel(1), channel(5), channel(9), 255]
}

#[cfg(test)]
mod tests;
