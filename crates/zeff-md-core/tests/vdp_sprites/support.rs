#[allow(dead_code)]
#[path = "../vdp_composition/cases.rs"]
pub mod background;
pub use background::{cram_word, palette, publish, solid, write};
pub use zeff_emu_common::time::MasterTicks;
pub use zeff_md_core::{
    ControlCommand, VdpMemory, VdpPixel, VdpRegisterPipeline, VdpRenderError, VdpSprite,
    VdpWriteFifo,
};

pub const SAT: u32 = 0xb800;
pub fn bank() -> VdpRegisterPipeline {
    let mut bank = background::bank();
    publish(&mut bank, 5, 0x5c);
    bank
}
pub fn entry(memory: &mut VdpMemory, base: u32, index: u8, words: [u16; 4]) {
    for (word, data) in words.into_iter().enumerate() {
        write(
            memory,
            base + u32::from(index) * 8 + word as u32 * 2,
            1,
            data,
        );
    }
}
pub fn at(memory: &mut VdpMemory, index: u8, position: (i16, i16), size: u8, link: u8, name: u16) {
    let (x, y) = position;
    entry(
        memory,
        SAT,
        index,
        [
            (y + 128) as u16,
            u16::from(size) * 256 + u16::from(link),
            name,
            (x + 128) as u16,
        ],
    );
}
pub fn pixel(color: u8, palette: u8, priority: bool) -> VdpPixel {
    VdpPixel {
        color,
        palette,
        priority,
    }
}
pub fn pattern_color(tile: u16, row: u16, column: u16) -> u8 {
    ((tile + row * 3 + column * 5 + 1) % 16) as u8
}
pub fn patterns(memory: &mut VdpMemory) {
    for tile in 0..16u16 {
        for row in 0..8u16 {
            for word in 0..2u16 {
                let mut data = 0;
                for column in word * 4..word * 4 + 4 {
                    data = data * 16 + u16::from(pattern_color(tile, row, column));
                }
                write(
                    memory,
                    u32::from(100 + tile) * 32 + u32::from(row) * 4 + u32::from(word) * 2,
                    1,
                    data,
                );
            }
        }
    }
}
