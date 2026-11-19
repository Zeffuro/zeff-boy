pub(crate) use crate::vdp_sprite_fetch::support as fetch;
pub(crate) use super::{previous, support as legacy};
pub use fetch::{Fixture, color, patterns};
pub use zeff_md_core::{
    ControlCommand, VdpMemory, VdpPixel, VdpRegisterPipeline, VdpRenderError, VdpSpriteCache,
    VdpSpriteLine, VdpSpritePattern, VdpWrite,
};

pub fn prepare(memory: &VdpMemory, bank: &VdpRegisterPipeline, y: u16) -> Vec<VdpSpritePattern> {
    let base = u32::from(bank.sprite_table_base().unwrap());
    let mut written = memory.clone();
    let mut cache = VdpSpriteCache::default();
    for index in 0..64 {
        for field in [0, 2] {
            let address = base + index * 8 + field;
            let data = u16::from_be_bytes([
                memory.vram()[address as usize],
                memory.vram()[address as usize + 1],
            ]);
            cache
                .complete_write(
                    &mut written,
                    bank,
                    VdpWrite {
                        command: ControlCommand { address, code: 1 },
                        data,
                    },
                )
                .unwrap();
        }
    }
    let row = cache.select_row(y).unwrap();
    (0..row.indices().len())
        .map(|slot| {
            row.prepare_sprite(&cache, &written, bank, slot as u8)
                .unwrap()
                .unwrap()
        })
        .collect()
}

pub fn fixture_pattern(fixture: &Fixture, y: u16) -> VdpSpritePattern {
    fixture
        .cache
        .select_row(y)
        .unwrap()
        .prepare_sprite(&fixture.cache, &fixture.memory, &fixture.bank, 0)
        .unwrap()
        .unwrap()
}

pub fn clear() -> VdpPixel {
    VdpPixel {
        color: 0,
        palette: 0,
        priority: false,
    }
}
