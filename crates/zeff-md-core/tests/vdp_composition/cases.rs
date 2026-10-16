use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{ControlCommand, VdpMemory, VdpRegisterPipeline, VdpRenderError, VdpWriteFifo};

pub const RASTER_WIDTH: usize = 96;
pub const RASTER_HEIGHT: usize = 64;

pub fn publish(bank: &mut VdpRegisterPipeline, index: u8, value: u8) {
    bank.advance_to(MasterTicks::new(bank.now().get() + 1))
        .unwrap();
    let write = bank.queue_register(index, value).unwrap();
    assert_eq!(bank.advance_to(write.publish_at), Ok(Some(write)));
}
pub fn bank() -> VdpRegisterPipeline {
    let mut bank = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    for (index, value) in [
        (1, 4),
        (12, 0),
        (2, 0x30),
        (3, 0x3c),
        (4, 7),
        (11, 0),
        (13, 0x3f),
        (15, 2),
        (16, 0),
        (17, 0),
        (18, 0),
    ] {
        publish(&mut bank, index, value);
    }
    bank
}
pub fn write(memory: &mut VdpMemory, address: u32, code: u8, data: u16) {
    memory
        .write_word(ControlCommand { address, code }, data)
        .unwrap();
}
pub fn cram_word(index: u16) -> u16 {
    0xe00 + (index / 8) * 32 + (index % 8) * 2
}
pub fn palette(memory: &mut VdpMemory) {
    for index in 0..64 {
        write(memory, index * 2, 3, cram_word(index as u16));
    }
}
pub fn solid(memory: &mut VdpMemory, tile: u32, color: u16) {
    for offset in (0..32).step_by(2) {
        write(memory, tile * 32 + offset, 1, color * 0x1111);
    }
}

pub fn priority_matrix_with(
    mut sample: impl FnMut(&VdpMemory, &VdpRegisterPipeline) -> Result<u16, VdpRenderError>,
) {
    let mut bank = bank();
    publish(&mut bank, 7, 63);
    let mut memory = VdpMemory::default();
    palette(&mut memory);
    solid(&mut memory, 3, 15);
    for window in [false, true] {
        publish(&mut bank, 17, if window { 0x80 } else { 0 });
        let foreground = if window { 0xf000 } else { 0xc000 };
        if window {
            write(&mut memory, 0xc000, 1, 0xa003);
        }
        for a in 0..16u16 {
            solid(&mut memory, 1, a);
            for b in 0..16u16 {
                solid(&mut memory, 2, b);
                for ap in 0..4u16 {
                    for bp in 0..4u16 {
                        for priority in 0..4u16 {
                            write(
                                &mut memory,
                                foreground,
                                1,
                                1 + ap * 0x2000 + (priority % 2) * 0x8000,
                            );
                            write(
                                &mut memory,
                                0xe000,
                                1,
                                2 + bp * 0x2000 + (priority / 2) * 0x8000,
                            );
                            let index = match (a != 0, b != 0, priority) {
                                (false, false, _) => 63,
                                (true, false, _) => ap * 16 + a,
                                (false, true, _) | (true, true, 2) => bp * 16 + b,
                                _ => ap * 16 + a,
                            };
                            assert_eq!(sample(&memory, &bank), Ok(cram_word(index)));
                        }
                    }
                }
            }
        }
    }
}
pub fn priority_matrix() {
    priority_matrix_with(|memory, bank| memory.sample_background(bank, 0, 0));
}

pub fn window_replacement() {
    let mut bank = bank();
    publish(&mut bank, 7, 32);
    publish(&mut bank, 17, 0x80);
    let mut memory = VdpMemory::default();
    palette(&mut memory);
    solid(&mut memory, 1, 1);
    solid(&mut memory, 2, 2);
    write(&mut memory, 0xc000, 1, 0xe001);
    write(&mut memory, 0xe000, 1, 0x4002);
    write(&mut memory, 0xf000, 1, 0xa000);
    assert_eq!(memory.sample_background(&bank, 0, 0), Ok(cram_word(34)));
    write(&mut memory, 0xe000, 1, 0xc000);
    assert_eq!(memory.sample_background(&bank, 0, 0), Ok(cram_word(32)));
    let snapshot = memory.clone();
    let saved = bank;
    assert_eq!(memory.sample_background(&bank, 0, 0), Ok(cram_word(32)));
    assert_eq!(memory, snapshot);
    assert_eq!(bank, saved);
    let mut independent = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    for (index, value) in [
        (12, 0),
        (3, 0x3c),
        (4, 7),
        (11, 0),
        (13, 0x3f),
        (16, 0),
        (17, 0x80),
        (18, 0),
    ] {
        publish(&mut independent, index, value);
    }
    write(&mut memory, 0xe000, 1, 0x4002);
    assert_eq!(
        memory.sample_background(&independent, 0, 0),
        Ok(cram_word(34))
    );
    publish(&mut bank, 17, 0);
    assert_eq!(memory.sample_background(&bank, 0, 0), Ok(cram_word(49)));
}

pub fn backdrop_and_publication() {
    let mut bank = bank();
    let mut memory = VdpMemory::default();
    palette(&mut memory);
    assert_eq!(
        memory.sample_background(&bank, 0, 0),
        Err(VdpRenderError::UnknownRegister(7))
    );
    for value in 0..=255u8 {
        publish(&mut bank, 7, value);
        for (x, y) in [(0, 0), (255, 255)] {
            assert_eq!(
                memory.sample_background(&bank, x, y),
                Ok(cram_word(u16::from(value % 64)))
            );
        }
    }
    publish(&mut bank, 7, 16);
    bank.advance_to(MasterTicks::new(bank.now().get() + 1))
        .unwrap();
    let event = bank.queue_register(7, 48).unwrap();
    let mut copied = bank;
    assert_eq!(memory.sample_background(&bank, 0, 0), Ok(cram_word(16)));
    bank.advance_to(MasterTicks::new(event.publish_at.get() - 1))
        .unwrap();
    assert_eq!(memory.sample_background(&bank, 0, 0), Ok(cram_word(16)));
    bank.advance_to(event.publish_at).unwrap();
    copied.advance_to(event.publish_at).unwrap();
    assert_eq!(memory.sample_background(&bank, 0, 0), Ok(cram_word(48)));
    assert_eq!(memory.sample_background(&copied, 0, 0), Ok(cram_word(48)));
    let mut fifo = VdpWriteFifo::default();
    let mut command = ControlCommand {
        address: 96,
        code: 3,
    };
    fifo.push_data(&mut command, bank.auto_increment(), 0xffff)
        .unwrap();
    assert_eq!(memory.sample_background(&bank, 0, 0), Ok(cram_word(48)));
    fifo.complete_write(&mut memory).unwrap();
    assert_eq!(memory.sample_background(&bank, 0, 0), Ok(0xeee));
    assert_eq!(command.address, 98);
    let mut independent = self::bank();
    solid(&mut memory, 1, 1);
    write(&mut memory, 0xc000, 1, 0x8001);
    assert_eq!(
        memory.sample_background(&independent, 0, 0),
        Ok(cram_word(1))
    );
    write(&mut memory, 0xc000, 1, 0);
    write(&mut memory, 0xe000, 1, 0x8001);
    assert_eq!(
        memory.sample_background(&independent, 0, 0),
        Ok(cram_word(1))
    );
    publish(&mut independent, 7, 0);
}

pub fn composition_guards() {
    let memory = VdpMemory::default();
    let unknown = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    assert_eq!(
        memory.sample_background(&unknown, 0, 0),
        Err(VdpRenderError::UnknownRegister(12))
    );
    let mut bank = bank();
    publish(&mut bank, 7, 0);
    let mut allowed = 0;
    let mut rejected = 0;
    for mode in 0..=255u8 {
        if mode & 0x81 != 0 {
            continue;
        }
        publish(&mut bank, 12, mode);
        if matches!(mode & 6, 0 | 6) {
            assert_eq!(memory.sample_background(&bank, 0, 0), Ok(0));
            allowed += 1;
        } else {
            assert_eq!(
                memory.sample_background(&bank, 0, 0),
                Err(VdpRenderError::UnsupportedRenderMode(mode))
            );
            rejected += 1;
        }
    }
    assert_eq!((allowed, rejected), (32, 32));
    publish(&mut bank, 12, 0);
    for (x, y) in [(256, 0), (0, 256), (65535, 65535)] {
        assert_eq!(
            memory.sample_background(&bank, x, y),
            Err(VdpRenderError::CoordinateOutOfBounds { x, y })
        );
    }
    publish(&mut bank, 11, 1);
    assert_eq!(
        memory.sample_background(&bank, 0, 0),
        Err(VdpRenderError::UnsupportedScrollMode(1))
    );
    publish(&mut bank, 11, 4);
    let mut memory = VdpMemory::default();
    write(&mut memory, 0xfc00, 1, 1);
    assert_eq!(
        memory.sample_background(&bank, 0, 0),
        Err(VdpRenderError::UnsupportedScrollCombination)
    );
    publish(&mut bank, 11, 0);
    for split in 1..16u8 {
        publish(&mut bank, 17, split);
        assert_eq!(
            memory.sample_background(&bank, u16::from(split) * 16, 0),
            Err(VdpRenderError::UnsupportedWindowScroll)
        );
        assert_eq!(
            memory.sample_background(&bank, 255, 255),
            Err(VdpRenderError::UnsupportedWindowScroll)
        );
        assert_eq!(memory.sample_background(&bank, 0, 0), Ok(0));
    }
    publish(&mut bank, 17, 0);
    assert_eq!(memory.sample_background(&bank, 255, 0), Ok(0));
    publish(&mut bank, 17, 16);
    assert_eq!(memory.sample_background(&bank, 255, 0), Ok(0));
    publish(&mut bank, 17, 0x81);
    assert_eq!(memory.sample_background(&bank, 0, 0), Ok(0));
    publish(&mut bank, 17, 1);
    write(&mut memory, 0xfc00, 1, 16);
    assert_eq!(memory.sample_background(&bank, 16, 0), Ok(0));
    for mode in [2, 3] {
        publish(&mut bank, 11, mode);
        write(&mut memory, 0xfc00, 1, 1);
        write(&mut memory, 0xfc20, 1, 16);
        assert_eq!(memory.sample_background(&bank, 16, 8), Ok(0));
        write(&mut memory, 0xfc00, 1, 16);
        write(&mut memory, 0xfc20, 1, 1);
        assert_eq!(
            memory.sample_background(&bank, 16, 8),
            Err(VdpRenderError::UnsupportedWindowScroll)
        );
        publish(&mut bank, 18, 2);
        assert_eq!(memory.sample_background(&bank, 16, 8), Ok(0));
        write(&mut memory, 0xfc40, 1, 1);
        assert_eq!(
            memory.sample_background(&bank, 16, 16),
            Err(VdpRenderError::UnsupportedWindowScroll)
        );
        publish(&mut bank, 18, 0);
    }
    publish(&mut bank, 17, 0);
    publish(&mut bank, 11, 4);
    write(&mut memory, 0xfc00, 1, 16);
    write(&mut memory, 0xfc02, 1, 1);
    assert_eq!(
        memory.sample_background(&bank, 0, 0),
        Err(VdpRenderError::UnsupportedScrollCombination)
    );
}

pub fn raster_scene() -> (VdpMemory, VdpRegisterPipeline) {
    let mut bank = bank();
    publish(&mut bank, 7, 0);
    publish(&mut bank, 18, 2);
    let mut memory = VdpMemory::default();
    for (index, color) in [(0, 0xe00), (1, 0x00e), (2, 0x0e0), (35, 0xeee)] {
        write(&mut memory, index * 2, 3, color);
    }
    for row in 0..8 {
        write(&mut memory, 32 + row * 4, 1, 0x1010);
        write(&mut memory, 34 + row * 4, 1, 0x1010);
        write(&mut memory, 64 + row * 4, 1, 0x2222);
        write(&mut memory, 66 + row * 4, 1, 0x2222);
        write(
            &mut memory,
            96 + row * 4,
            1,
            if row < 4 { 0x3333 } else { 0 },
        );
    }
    for row in 0..8u32 {
        for column in 0..12 {
            let offset = (row * 32 + column) * 2;
            write(
                &mut memory,
                0xc000 + offset,
                1,
                if (4..8).contains(&column) { 0x8001 } else { 1 },
            );
            write(
                &mut memory,
                0xe000 + offset,
                1,
                if column < 8 { 0x8002 } else { 0 },
            );
            write(&mut memory, 0xf000 + offset, 1, 0xc003);
        }
    }
    (memory, bank)
}
pub fn expected_raster(x: usize, y: usize) -> u16 {
    if y < 16 {
        if x % 8 < 4 && y % 8 < 4 {
            0xeee
        } else if x < 64 {
            0x0e0
        } else {
            0xe00
        }
    } else if x < 32 {
        0x0e0
    } else if x.is_multiple_of(2) {
        0x00e
    } else if x < 64 {
        0x0e0
    } else {
        0xe00
    }
}
pub fn raster_pixels() -> [u16; RASTER_WIDTH * RASTER_HEIGHT] {
    let (memory, bank) = raster_scene();
    std::array::from_fn(|index| {
        let (x, y) = (index % RASTER_WIDTH, index / RASTER_WIDTH);
        let actual = memory.sample_background(&bank, x as u16, y as u16).unwrap();
        assert_eq!(actual, expected_raster(x, y));
        actual
    })
}
pub fn static_raster() {
    assert_eq!(raster_pixels().len(), 6144);
}
