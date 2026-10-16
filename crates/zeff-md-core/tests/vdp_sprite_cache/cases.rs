use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, VdpMemory, VdpRegisterPipeline, VdpRenderError, VdpSprite, VdpSpriteCache,
    VdpWrite, VdpWriteError, VdpWriteFifo,
};

pub fn publish(bank: &mut VdpRegisterPipeline, value: u8) {
    bank.advance_to(MasterTicks::new(bank.now().get() + 1))
        .unwrap();
    let write = bank.queue_register(5, value).unwrap();
    bank.advance_to(write.publish_at).unwrap();
}

pub fn write(address: u32, data: u16) -> VdpWrite {
    VdpWrite {
        command: ControlCommand { address, code: 1 },
        data,
    }
}

pub fn unknown_and_rejections() {
    let mut memory = VdpMemory::default();
    let mut bank = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    let mut cache = VdpSpriteCache::default();
    for index in 0..64 {
        assert_eq!(cache.entry_words(index), Ok([None, None]));
        assert_eq!(
            cache.sprite_entry(&memory, &bank, index),
            Err(VdpRenderError::UnknownSpriteCache(index))
        );
    }
    let original = memory.clone();
    assert_eq!(
        cache.complete_write(&mut memory, &bank, write(0, 128)),
        Err(VdpWriteError::UnknownSpriteTable)
    );
    assert_eq!(memory, original);
    assert_eq!(cache, VdpSpriteCache::default());
    publish(&mut bank, 0);
    cache
        .complete_write(&mut memory, &bank, write(10, 0x0307))
        .unwrap();
    assert_eq!(cache.entry_words(1), Ok([None, Some(0x0307)]));
    assert_eq!(
        cache.sprite_entry(&memory, &bank, 1),
        Err(VdpRenderError::UnknownSpriteCache(1))
    );
    cache
        .complete_write(&mut memory, &bank, write(0, 128))
        .unwrap();
    assert_eq!(cache.entry_words(0), Ok([Some(128), None]));
    assert_eq!(
        cache.sprite_entry(&memory, &bank, 0),
        Err(VdpRenderError::UnknownSpriteCache(0))
    );
    cache
        .complete_write(&mut memory, &bank, write(2, 0))
        .unwrap();
    let original = memory.clone();
    let saved = cache;
    for code in 0..=255 {
        if code == 1 || code == 0x11 {
            continue;
        }
        let request = VdpWrite {
            command: ControlCommand { address: 0, code },
            data: 0xffff,
        };
        assert_eq!(
            cache.complete_write(&mut memory, &bank, request),
            Err(VdpWriteError::UnsupportedCommand(request.command))
        );
        assert_eq!(memory, original);
        assert_eq!(cache, saved);
    }
    for address in [1, 3, 0x1ff, 0xffff, 0x20000, 0x1ffff, u32::MAX] {
        let request = write(address, 0xffff);
        assert_eq!(
            cache.complete_write(&mut memory, &bank, request),
            Err(VdpWriteError::UnsupportedCommand(request.command))
        );
        assert_eq!(memory, original);
        assert_eq!(cache, saved);
    }
    for index in 64..=255 {
        assert_eq!(
            cache.entry_words(index),
            Err(VdpRenderError::InvalidSpriteIndex(index))
        );
        assert_eq!(
            cache.sprite_entry(&memory, &bank, index),
            Err(VdpRenderError::InvalidSpriteIndex(index))
        );
    }
}

pub fn aperture_and_fields() {
    let mut memory = VdpMemory::default();
    let mut bank = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    for r5 in 0..=255 {
        publish(&mut bank, r5);
        let base = u32::from(r5 & 0x7f) * 512;
        for code in [1, 0x11] {
            let mut cache = VdpSpriteCache::default();
            for offset in (0..512).step_by(2) {
                let saved = cache;
                let request = VdpWrite {
                    command: ControlCommand {
                        address: base + offset,
                        code,
                    },
                    data: 0xffff,
                };
                cache.complete_write(&mut memory, &bank, request).unwrap();
                assert_eq!(
                    &memory.vram()[(base + offset) as usize..(base + offset + 2) as usize],
                    &[255, 255]
                );
                let index = (offset / 8) as u8;
                if r5 >= 128 || offset % 8 >= 4 {
                    assert_eq!(cache, saved);
                } else if offset % 8 == 0 {
                    assert_eq!(cache.entry_words(index), Ok([Some(0x3ff), None]));
                } else {
                    assert_eq!(cache.entry_words(index), Ok([Some(0x3ff), Some(0x0f7f)]));
                    assert_eq!(
                        cache.sprite_entry(&memory, &bank, index).unwrap().height,
                        32
                    );
                }
            }
            for address in [base.checked_sub(2), base.checked_add(512)]
                .into_iter()
                .flatten()
                .filter(|address| *address < 65536)
            {
                let saved = cache;
                cache
                    .complete_write(&mut memory, &bank, write(address, 128))
                    .unwrap();
                assert_eq!(cache, saved);
            }
        }
    }
    publish(&mut bank, 0);
    let mut cache = VdpSpriteCache::default();
    for raw in 0..512 {
        cache
            .complete_write(&mut memory, &bank, write(0, raw | 0xfe00))
            .unwrap();
        cache
            .complete_write(&mut memory, &bank, write(2, 0))
            .unwrap();
        assert_eq!(
            cache.sprite_entry(&memory, &bank, 0).unwrap().y,
            raw as i16 - 128
        );
    }
    for raw in 0..=255u16 {
        cache
            .complete_write(&mut memory, &bank, write(2, raw * 256 + raw))
            .unwrap();
        let sprite = cache.sprite_entry(&memory, &bank, 0).unwrap();
        assert_eq!(sprite.width, ((raw as u8 / 4) % 4 + 1) * 8);
        assert_eq!(sprite.height, (raw as u8 % 4 + 1) * 8);
        assert_eq!(sprite.link, raw as u8 % 128);
    }
}

pub fn stale_table_and_publication() {
    let mut memory = VdpMemory::default();
    let mut bank = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    let mut cache = VdpSpriteCache::default();
    publish(&mut bank, 0);
    for (address, data) in [(0, 144), (2, 0x0603), (4, 0x1234), (6, 160)] {
        cache
            .complete_write(&mut memory, &bank, write(address, data))
            .unwrap();
    }
    for (offset, data) in [(0, 192), (2, 0x0f07), (4, 0xabcd), (6, 208)] {
        memory
            .write_word(write(512 + offset, data).command, data)
            .unwrap();
    }
    let saved = cache;
    bank.advance_to(MasterTicks::new(bank.now().get() + 1))
        .unwrap();
    let pending = bank.queue_register(5, 1).unwrap();
    cache
        .complete_write(&mut memory, &bank, write(0, 152))
        .unwrap();
    assert_eq!(cache.entry_words(0), Ok([Some(152), Some(0x0603)]));
    bank.advance_to(pending.publish_at).unwrap();
    let expected = VdpSprite {
        x: 80,
        y: 24,
        width: 16,
        height: 24,
        link: 3,
        name: 0xabcd,
    };
    assert_eq!(cache.sprite_entry(&memory, &bank, 0), Ok(expected));
    assert_eq!(memory.sprite_entry(&bank, 0).unwrap().y, 64);
    cache
        .complete_write(&mut memory, &bank, write(0, 160))
        .unwrap();
    assert_eq!(cache.sprite_entry(&memory, &bank, 0), Ok(expected));
    cache
        .complete_write(&mut memory, &bank, write(514, 0x0105))
        .unwrap();
    assert_eq!(
        cache.sprite_entry(&memory, &bank, 0),
        Ok(VdpSprite {
            width: 8,
            height: 16,
            link: 5,
            ..expected
        })
    );
    publish(&mut bank, 0x81);
    let before = cache;
    cache
        .complete_write(&mut memory, &bank, write(512, 200))
        .unwrap();
    assert_eq!(cache, before);
    assert_eq!(memory.sprite_entry(&bank, 0).unwrap().y, 72);
    assert_eq!(cache.sprite_entry(&memory, &bank, 0).unwrap().y, 24);
    publish(&mut bank, 0);
    assert_eq!(saved.sprite_entry(&memory, &bank, 0).unwrap().y, 16);
}

pub fn copies_and_raw_memory() {
    let mut memory = VdpMemory::default();
    let mut bank = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    let mut cache = VdpSpriteCache::default();
    publish(&mut bank, 0);
    cache
        .complete_write(&mut memory, &bank, write(0, 128))
        .unwrap();
    let mut copy = cache;
    let mut copied_memory = memory.clone();
    cache
        .complete_write(&mut memory, &bank, write(2, 0))
        .unwrap();
    assert_eq!(copy.entry_words(0), Ok([Some(128), None]));
    copy.complete_write(&mut copied_memory, &bank, write(2, 0x0f7f))
        .unwrap();
    assert_eq!(cache.sprite_entry(&memory, &bank, 0).unwrap().width, 8);
    assert_eq!(
        copy.sprite_entry(&copied_memory, &bank, 0).unwrap().width,
        32
    );
    let original = cache;
    let mut fifo = VdpWriteFifo::default();
    fifo.push(write(0, 200)).unwrap();
    assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
    assert_eq!(cache, original);
    cache.invalidate();
    for index in 0..64 {
        assert_eq!(cache.entry_words(index), Ok([None, None]));
    }
    assert_eq!(
        cache.sprite_entry(&memory, &bank, 0),
        Err(VdpRenderError::UnknownSpriteCache(0))
    );
    assert_eq!(fifo.complete_write(&mut memory), Ok(Some(write(0, 200))));
    assert_eq!(memory.sprite_entry(&bank, 0).unwrap().y, 72);
    cache
        .complete_write(&mut memory, &bank, write(0, 200))
        .unwrap();
    cache
        .complete_write(&mut memory, &bank, write(2, 0))
        .unwrap();
    memory.write_word(write(2, 0x0f7f).command, 0x0f7f).unwrap();
    cache.invalidate();
    assert_eq!(
        cache.sprite_entry(&memory, &bank, 0),
        Err(VdpRenderError::UnknownSpriteCache(0))
    );
    cache
        .complete_write(&mut memory, &bank, write(0, 200))
        .unwrap();
    cache
        .complete_write(&mut memory, &bank, write(2, 0x0f7f))
        .unwrap();
    let unknown_bank = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    assert_eq!(
        cache.sprite_entry(&memory, &unknown_bank, 0),
        Err(VdpRenderError::UnknownRegister(5))
    );
}
