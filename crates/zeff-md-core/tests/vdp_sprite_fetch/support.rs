use crate::vdp_sprite_row::cases as selection;
pub use selection::Fixture;
pub use zeff_emu_common::time::MasterTicks;
pub use zeff_md_core::{VdpRegisterPipeline, VdpRenderError, VdpSpriteCache};

pub fn color(tile: u16, row: u16, column: u16) -> u8 {
    ((tile + row * 3 + column * 5 + 1) % 16) as u8
}

pub fn patterns(fixture: &mut Fixture, base: u16) {
    for tile in 0..16u16 {
        for row in 0..8u16 {
            for word in 0..2u16 {
                let mut data = 0;
                for column in word * 4..word * 4 + 4 {
                    data = data * 16 + u16::from(color(tile, row, column));
                }
                fixture.word(
                    u32::from((base + tile) % 2048) * 32 + u32::from(row) * 4 + u32::from(word) * 2,
                    data,
                );
            }
        }
    }
}

pub fn attributes(value: u16) -> u16 {
    (value % 4) * 0x2000
        + ((value / 4) % 2) * 0x8000
        + ((value / 8) % 2) * 0x800
        + (value / 16) * 0x1000
}
