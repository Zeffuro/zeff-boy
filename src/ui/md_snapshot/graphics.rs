use crate::debug::{
    ConsoleGraphicsData, MdGraphicsData, PaletteDebugInfo, PaletteGroupDebug, PaletteRowDebug,
};
use zeff_md_core::Emulator;

pub(super) fn snapshot(emu: &Emulator) -> ConsoleGraphicsData {
    let registers = emu.vdp_registers();
    ConsoleGraphicsData::Md(Box::new(MdGraphicsData {
        vram: emu.vdp().memory().vram().to_vec(),
        cram: (0..emu.vdp().memory().cram_word_count())
            .flat_map(|index| match emu.vdp().memory().cram_word(index) {
                Some(word) => word.to_be_bytes().map(Some),
                None => [None; 2],
            })
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
                .flat_map(|(index, colors)| {
                    if colors.iter().all(Option::is_some) {
                        vec![PaletteRowDebug {
                            label: format!("Palette {index}"),
                            colors: colors.iter().flatten().copied().collect(),
                        }]
                    } else {
                        colors
                            .iter()
                            .enumerate()
                            .map(|(color, rgba)| PaletteRowDebug {
                                label: format!(
                                    "Palette {index}, color {color}{}",
                                    if rgba.is_none() { ": unknown" } else { "" }
                                ),
                                colors: rgba.iter().copied().collect(),
                            })
                            .collect()
                    }
                })
                .collect(),
        }],
    }
}

fn colors(emu: &Emulator) -> [Option<[u8; 4]>; 64] {
    std::array::from_fn(|index| emu.vdp().memory().cram_word(index).map(rgba))
}

fn rgba(color: u16) -> [u8; 4] {
    let channel = |shift: u32| (((color >> shift) & 7_u16) * 255 / 7) as u8;
    [channel(1), channel(5), channel(9), 255]
}

#[cfg(test)]
mod tests;
