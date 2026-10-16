use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, VdpMemory, VdpRegisterPipeline, VdpRenderError, VdpSpriteCache, VdpWrite,
};

pub struct Fixture {
    pub memory: VdpMemory,
    pub bank: VdpRegisterPipeline,
    pub cache: VdpSpriteCache,
}

impl Fixture {
    pub fn new() -> Self {
        let mut fixture = Self {
            memory: VdpMemory::default(),
            bank: VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0)),
            cache: VdpSpriteCache::default(),
        };
        fixture.publish(0);
        fixture
    }

    pub fn publish(&mut self, r5: u8) {
        self.bank
            .advance_to(MasterTicks::new(self.bank.now().get() + 1))
            .unwrap();
        let write = self.bank.queue_register(5, r5).unwrap();
        self.bank.advance_to(write.publish_at).unwrap();
    }

    pub fn word(&mut self, address: u32, data: u16) {
        self.cache
            .complete_write(
                &mut self.memory,
                &self.bank,
                VdpWrite {
                    command: ControlCommand { address, code: 1 },
                    data,
                },
            )
            .unwrap();
    }

    pub fn entry(&mut self, index: u8, y: u16, size: u8, link: u8) {
        self.word(u32::from(index) * 8, y);
        self.word(
            u32::from(index) * 8 + 2,
            u16::from(size) * 256 + u16::from(link),
        );
    }
}

fn expect(cache: &VdpSpriteCache, y: u16, expected: &[u8]) {
    assert_eq!(cache.select_row(y).unwrap().indices(), expected);
}

pub fn vertical_bounds() {
    let mut fixture = Fixture::new();
    for position in 0..512 {
        for height in 1..=4u8 {
            fixture.entry(0, position | 0xfe00, height - 1, 0);
            for row in 0..256 {
                let biased_row = row + 128;
                let visible =
                    biased_row >= position && biased_row < position + u16::from(height) * 8;
                expect(&fixture.cache, row, if visible { &[0] } else { &[] });
            }
        }
    }
    for size in 0..=255 {
        fixture.entry(0, 128, size, 0);
        let height = u16::from(size % 4 + 1) * 8;
        expect(&fixture.cache, 0, &[0]);
        expect(&fixture.cache, height - 1, &[0]);
        expect(&fixture.cache, height, &[]);
    }
}

pub fn linked_capacity() {
    let mut fixture = Fixture::new();
    fixture.entry(0, 400, 15, 63);
    fixture.entry(63, 128, 15, 7);
    fixture.entry(7, 128, 15, 0);
    expect(&fixture.cache, 0, &[63, 7]);
    fixture.entry(0, 128, 15, 0);
    expect(&fixture.cache, 0, &[0]);
    for visible_count in 0..=64 {
        for step in 0..64u8 {
            let index = step.wrapping_mul(17) % 64;
            let link = if step == 63 {
                0
            } else {
                (step + 1).wrapping_mul(17) % 64
            };
            let position = if step < visible_count { 128 } else { 400 };
            fixture.entry(index, position, 15, link);
        }
        let mut expected = [0; 16];
        for (step, index) in expected.iter_mut().enumerate() {
            *index = (step as u8 * 17) % 64;
        }
        expect(
            &fixture.cache,
            0,
            &expected[..usize::from(visible_count.min(16))],
        );
    }
    for shift in 0..64u8 {
        for index in 0..64u8 {
            fixture.entry(
                index,
                if index.wrapping_sub(shift) % 64 < 16 {
                    128
                } else {
                    400
                },
                15,
                if index == 63 { 0 } else { index + 1 },
            );
        }
        let mut expected = [0; 16];
        let mut count = 0;
        for index in 0..64u8 {
            if index.wrapping_sub(shift) % 64 < 16 {
                expected[count] = index;
                count += 1;
            }
        }
        expect(&fixture.cache, 0, &expected);
    }
}

pub fn unknown_and_malformed() {
    let mut fixture = Fixture::new();
    for missing in 0..64u8 {
        for field in 0..2 {
            fixture.cache.invalidate();
            for index in 0..64u8 {
                let layout = if index == 63 { 0 } else { u16::from(index + 1) };
                if index != missing || field != 0 {
                    fixture.word(u32::from(index) * 8, 128);
                }
                if index != missing || field != 1 {
                    fixture.word(u32::from(index) * 8 + 2, layout);
                }
            }
            let saved = fixture.cache;
            assert_eq!(
                fixture.cache.select_row(0),
                Err(VdpRenderError::UnknownSpriteCache(missing))
            );
            assert_eq!(fixture.cache, saved);
        }
    }
    for link in 64..128 {
        fixture.entry(0, 128, 0, link);
        assert_eq!(
            fixture.cache.select_row(0),
            Err(VdpRenderError::InvalidSpriteIndex(link))
        );
    }
    for index in 0..64u8 {
        fixture.entry(index, 128, 0, if index == 63 { 1 } else { index + 1 });
    }
    assert_eq!(
        fixture.cache.select_row(0),
        Err(VdpRenderError::SpriteLinkCycle(1))
    );
    fixture.entry(63, 128, 0, 63);
    assert_eq!(
        fixture.cache.select_row(0),
        Err(VdpRenderError::SpriteLinkCycle(63))
    );
    fixture.entry(63, 128, 0, 127);
    assert_eq!(
        fixture.cache.select_row(0),
        Err(VdpRenderError::InvalidSpriteIndex(127))
    );
    fixture.entry(0, 400, 0, 1);
    fixture.entry(1, 400, 0, 1);
    assert_eq!(
        fixture.cache.select_row(0),
        Err(VdpRenderError::SpriteLinkCycle(1))
    );
    for row in [256, 511, 512, u16::MAX] {
        assert_eq!(
            fixture.cache.select_row(row),
            Err(VdpRenderError::CoordinateOutOfBounds { x: 0, y: row })
        );
    }
}

pub fn snapshot_and_fetch() {
    let mut fixture = Fixture::new();
    fixture.entry(0, 128, 1, 0);
    let selected = fixture.cache.select_row(8).unwrap();
    assert_eq!(selected.indices(), &[0]);
    let copy = fixture.cache;
    fixture.entry(0, 216, 1, 0);
    expect(&fixture.cache, 8, &[]);
    expect(&copy, 8, &[0]);
    assert_eq!(selected.indices(), &[0]);
    fixture.word(516, 0xabcd);
    fixture.word(518, 200);
    let cache_before = fixture.cache;
    fixture.publish(1);
    assert_eq!(fixture.cache, cache_before);
    let fetched = fixture
        .cache
        .sprite_entry(&fixture.memory, &fixture.bank, selected.indices()[0])
        .unwrap();
    assert_eq!((fetched.y, fetched.x, fetched.name), (88, 72, 0xabcd));
    expect(&fixture.cache, 8, &[]);
    fixture.cache.invalidate();
    assert_eq!(selected.indices(), &[0]);
    assert_eq!(
        fixture.cache.select_row(8),
        Err(VdpRenderError::UnknownSpriteCache(0))
    );
}
