use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, VdpMemory, VdpPixel, VdpPlane, VdpRegisterPipeline, VdpRenderError,
    VdpWriteFifo,
};

pub fn publish(bank: &mut VdpRegisterPipeline, index: u8, value: u8) {
    bank.advance_to(MasterTicks::new(bank.now().get() + 1))
        .unwrap();
    let event = bank.queue_register(index, value).unwrap();
    assert_eq!(bank.advance_to(event.publish_at), Ok(Some(event)));
}
pub fn bank() -> VdpRegisterPipeline {
    let mut bank = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    for (index, value) in [
        (1, 4),
        (12, 0),
        (2, 0x30),
        (3, 0x3e),
        (4, 7),
        (11, 0),
        (13, 0x3f),
        (15, 2),
        (16, 0),
        (17, 0x80),
        (18, 0),
    ] {
        publish(&mut bank, index, value);
    }
    bank
}
pub fn write(memory: &mut VdpMemory, address: u32, data: u16) {
    memory
        .write_word(ControlCommand { address, code: 1 }, data)
        .unwrap();
}
pub fn vertical(memory: &mut VdpMemory, word: u32, data: u16) {
    memory
        .write_word(
            ControlCommand {
                address: word * 2,
                code: 5,
            },
            data,
        )
        .unwrap();
}
pub fn pixel(color: u8, palette: u8, priority: bool) -> VdpPixel {
    VdpPixel {
        color,
        palette,
        priority,
    }
}
pub fn pattern(memory: &mut VdpMemory) {
    for (row, words) in [
        [0x1234, 0x5678],
        [0x89ab, 0xcdef],
        [0xf0e1, 0xd2c3],
        [0x1357, 0x9bdf],
        [0x2468, 0xace0],
        [0x9876, 0x5432],
        [0x5a6b, 0x7c8d],
        [0x9876, 0x5432],
    ]
    .into_iter()
    .enumerate()
    {
        for (word, value) in words.into_iter().enumerate() {
            write(memory, 0xc80 + (row * 4 + word * 2) as u32, value);
        }
    }
}

pub fn window_regions_and_guards() {
    let memory = VdpMemory::default();
    let mut unknown = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    assert_eq!(
        memory.sample_window(&unknown, 0, 0),
        Err(VdpRenderError::UnknownRegister(17))
    );
    publish(&mut unknown, 17, 0x80);
    assert_eq!(
        memory.sample_window(&unknown, 0, 0),
        Err(VdpRenderError::UnknownRegister(18))
    );
    publish(&mut unknown, 18, 0);
    assert_eq!(
        memory.sample_window(&unknown, 0, 0),
        Err(VdpRenderError::UnknownRegister(3))
    );
    publish(&mut unknown, 17, 0);
    assert_eq!(memory.sample_window(&unknown, 0, 0), Ok(None));
    let mut bank = bank();
    for (horizontal, vertical, expected) in [
        (2, 1, [true, true, true, false]),
        (0x82, 1, [true, true, false, true]),
        (2, 0x81, [true, false, true, true]),
        (0x82, 0x81, [false, true, true, true]),
    ] {
        publish(&mut bank, 17, horizontal);
        publish(&mut bank, 18, vertical);
        for ((x, y), selected) in [(31, 7), (32, 7), (31, 8), (32, 8)]
            .into_iter()
            .zip(expected)
        {
            assert_eq!(
                memory.sample_window(&bank, x, y),
                Ok(selected.then_some(pixel(0, 0, false)))
            );
        }
    }
    for value in 0..=255u8 {
        publish(&mut bank, 17, value);
        publish(&mut bank, 18, 0);
        for x in [0, 15, 16, 31, 32, 247, 248, 255] {
            let before = x / 16 < u16::from(value % 32);
            let selected = if value < 128 { before } else { !before };
            assert_eq!(
                memory.sample_window(&bank, x, 0),
                Ok(selected.then_some(pixel(0, 0, false)))
            );
        }
        publish(&mut bank, 17, 0);
        publish(&mut bank, 18, value);
        for y in [0, 7, 8, 15, 16, 239, 247, 248, 255] {
            let before = y / 8 < u16::from(value % 32);
            let selected = if value < 128 { before } else { !before };
            assert_eq!(
                memory.sample_window(&bank, 255, y),
                Ok(selected.then_some(pixel(0, 0, false)))
            );
        }
    }
    for (x, y) in [(256, 0), (0, 256), (u16::MAX, u16::MAX)] {
        assert_eq!(
            memory.sample_window(&bank, x, y),
            Err(VdpRenderError::CoordinateOutOfBounds { x, y })
        );
    }
    publish(&mut bank, 17, 16);
    publish(&mut bank, 18, 0);
    assert_eq!(
        memory.sample_window(&bank, 255, 255),
        Ok(Some(pixel(0, 0, false)))
    );
    publish(&mut bank, 17, 0x90);
    assert_eq!(memory.sample_window(&bank, 255, 255), Ok(None));
}

pub fn window_table_addresses() {
    let mut bank = bank();
    publish(&mut bank, 16, 0x22);
    for value in 0..=255u8 {
        publish(&mut bank, 3, value);
        let address = u32::from(value / 2 % 32) * 2048;
        let mut memory = VdpMemory::default();
        pattern(&mut memory);
        for (offset, name) in [(0, 0x8064), (2, 0x2864), (64, 0x1064), (2046, 0xf064)] {
            write(&mut memory, address + offset, name);
        }
        for (x, y, expected) in [
            (0, 0, pixel(1, 0, true)),
            (8, 0, pixel(8, 1, false)),
            (0, 8, pixel(9, 0, false)),
            (255, 255, pixel(8, 3, true)),
        ] {
            assert_eq!(memory.sample_window(&bank, x, y), Ok(Some(expected)));
        }
        if value == 0x3e {
            assert_eq!(&memory.vram()[0xfffe..], &[0xf0, 0x64]);
        }
    }
    let mut independent = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    for (index, value) in [(3, 0x3e), (17, 0x80), (18, 0)] {
        publish(&mut independent, index, value);
    }
    assert_eq!(
        VdpMemory::default().sample_window(&independent, 255, 255),
        Ok(Some(pixel(0, 0, false)))
    );
}

pub fn scroll_tables_and_modes() {
    let mut bank = bank();
    let mut memory = VdpMemory::default();
    for line in 0..256u32 {
        write(&mut memory, 0xfc00 + line * 4, 0xf000 + line as u16);
        write(&mut memory, 0xfc02 + line * 4, 0xa400 + 255 - line as u16);
    }
    for column in 0..16u32 {
        vertical(&mut memory, column * 2, 0x400 + column as u16 * 31);
        vertical(&mut memory, column * 2 + 1, 0x7ff - column as u16 * 17);
    }
    let saved = memory.clone();
    for mode in 0..=255u8 {
        publish(&mut bank, 11, mode);
        for line in [0, 7, 8, 239, 255] {
            for column in [0, 15] {
                for (plane, b) in [(VdpPlane::A, false), (VdpPlane::B, true)] {
                    if mode % 4 == 1 {
                        assert_eq!(
                            memory.scroll_offsets(&bank, plane, line, column),
                            Err(VdpRenderError::UnsupportedScrollMode(mode))
                        );
                    } else {
                        let row = match mode % 4 {
                            0 => 0,
                            2 => line / 8 * 8,
                            _ => line,
                        };
                        let col = if mode / 4 % 2 == 0 { 0 } else { column };
                        let expected = if b {
                            (u16::from(255 - row), 1023 - u16::from(col) * 17)
                        } else {
                            (u16::from(row), u16::from(col) * 31)
                        };
                        assert_eq!(
                            memory.scroll_offsets(&bank, plane, line, column),
                            Ok(expected)
                        );
                    }
                }
            }
        }
    }
    for mode in [0, 2, 3, 4, 6, 7] {
        publish(&mut bank, 11, mode);
        for line in 0..=255u8 {
            let row = match mode % 4 {
                0 => 0,
                2 => line / 8 * 8,
                _ => line,
            };
            for column in 0..16 {
                let col = if mode < 4 { 0 } else { column };
                assert_eq!(
                    memory.scroll_offsets(&bank, VdpPlane::A, line, column),
                    Ok((u16::from(row), u16::from(col) * 31))
                );
                assert_eq!(
                    memory.scroll_offsets(&bank, VdpPlane::B, line, column),
                    Ok((u16::from(255 - row), 1023 - u16::from(col) * 17))
                );
            }
        }
    }
    assert_eq!(memory, saved);
}

pub fn scrolled_coordinates_and_columns() {
    let mut bank = bank();
    for (size, width, height) in [
        (0, 32, 32),
        (0x10, 32, 64),
        (0x30, 32, 128),
        (1, 64, 32),
        (0x11, 64, 64),
        (3, 128, 32),
    ] {
        publish(&mut bank, 16, size);
        publish(&mut bank, 11, 0);
        let mut memory = VdpMemory::default();
        pattern(&mut memory);
        for (plane, base, word) in [(VdpPlane::A, 0xc000, 0), (VdpPlane::B, 0xe000, 1)] {
            write(&mut memory, 0xfc00 + word * 2, 17);
            vertical(&mut memory, word, 9);
            write(&mut memory, base + (2 * width + width - 1) * 2, 0x8064);
            assert_eq!(
                memory.sample_scrolled_plane(&bank, plane, 16, 7),
                Ok(pixel(8, 0, true))
            );
            write(&mut memory, 0xfc00 + word * 2, 0xffff);
            vertical(&mut memory, word, 0x7ff);
            write(&mut memory, base + (height - 1) * width * 2, 0x4064);
            assert_eq!(
                memory.sample_scrolled_plane(&bank, plane, 0, 0),
                Ok(pixel(8, 2, false))
            );
        }
    }
    publish(&mut bank, 16, 0x11);
    publish(&mut bank, 11, 4);
    let mut memory = VdpMemory::default();
    for (plane, base, lane) in [(VdpPlane::A, 0xc000, 0), (VdpPlane::B, 0xe000, 1)] {
        for horizontal in [0u16, 16, 32, 1008] {
            let mut memory = VdpMemory::default();
            pattern(&mut memory);
            write(&mut memory, 0xfc00 + lane * 2, horizontal);
            for column in 0..16u32 {
                vertical(&mut memory, column * 2 + lane, (column + lane) as u16 * 8);
                let source_column = (column * 2 + 64 - u32::from(horizontal / 8) % 64) % 64;
                let cell = (column + lane) * 64 + source_column;
                write(&mut memory, base + cell * 2, 0xa064);
            }
            for column in 0..16u16 {
                assert_eq!(
                    memory.sample_scrolled_plane(&bank, plane, column * 16 + 3, 5),
                    Ok(pixel(6, 1, true))
                );
            }
            if plane == VdpPlane::A && horizontal == 16 {
                assert_eq!(
                    memory.sample_plane(&bank, plane, 3, 5),
                    Ok(pixel(0, 0, false))
                );
                assert_eq!(
                    memory.sample_plane(&bank, plane, 19, 13),
                    Ok(pixel(0, 0, false))
                );
            }
        }
        for horizontal in [1, 15, 17, 1023] {
            write(&mut memory, 0xfc00 + lane * 2, horizontal);
            assert_eq!(
                memory.sample_scrolled_plane(&bank, plane, 255, 255),
                Err(VdpRenderError::UnsupportedScrollCombination)
            );
        }
    }
}

pub fn scroll_guards() {
    let memory = VdpMemory::default();
    let mut unknown = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    assert_eq!(
        memory.scroll_offsets(&unknown, VdpPlane::A, 0, 0),
        Err(VdpRenderError::UnknownRegister(11))
    );
    publish(&mut unknown, 11, 0);
    assert_eq!(
        memory.scroll_offsets(&unknown, VdpPlane::A, 0, 0),
        Err(VdpRenderError::UnknownRegister(13))
    );
    publish(&mut unknown, 13, 0);
    assert_eq!(
        memory.scroll_offsets(&unknown, VdpPlane::A, 0, 0),
        Ok((0, 0))
    );
    assert_eq!(
        memory.sample_scrolled_plane(&unknown, VdpPlane::A, 0, 0),
        Err(VdpRenderError::UnknownRegister(16))
    );
    publish(&mut unknown, 16, 0);
    for (plane, index) in [(VdpPlane::A, 2), (VdpPlane::B, 4)] {
        assert_eq!(
            memory.sample_scrolled_plane(&unknown, plane, 0, 0),
            Err(VdpRenderError::UnknownRegister(index))
        );
    }
    let mut bank = bank();
    for column in 16..=255 {
        for plane in [VdpPlane::A, VdpPlane::B] {
            assert_eq!(
                memory.scroll_offsets(&bank, plane, 255, column),
                Err(VdpRenderError::InvalidScrollColumn(column))
            );
        }
    }
    for (x, y) in [(256, 0), (0, 256), (65535, 65535)] {
        assert_eq!(
            memory.sample_scrolled_plane(&bank, VdpPlane::A, x, y),
            Err(VdpRenderError::CoordinateOutOfBounds { x, y })
        );
    }
    publish(&mut bank, 11, 1);
    assert_eq!(
        memory.sample_scrolled_plane(&bank, VdpPlane::B, 0, 0),
        Err(VdpRenderError::UnsupportedScrollMode(1))
    );
    publish(&mut bank, 11, 0);
    publish(&mut bank, 16, 0x22);
    assert_eq!(
        memory.sample_scrolled_plane(&bank, VdpPlane::A, 0, 0),
        Err(VdpRenderError::UnsupportedPlaneSize(0x22))
    );
}

pub fn publication_and_fifo() {
    let mut bank = bank();
    let mut memory = VdpMemory::default();
    pattern(&mut memory);
    write(&mut memory, 0xf800, 0x8064);
    publish(&mut bank, 17, 0);
    bank.advance_to(MasterTicks::new(bank.now().get() + 1))
        .unwrap();
    let event = bank.queue_register(17, 0x80).unwrap();
    assert_eq!(memory.sample_window(&bank, 0, 0), Ok(None));
    bank.advance_to(MasterTicks::new(event.publish_at.get() - 1))
        .unwrap();
    assert_eq!(memory.sample_window(&bank, 0, 0), Ok(None));
    let mut copied = bank;
    let snapshot = memory.clone();
    bank.advance_to(event.publish_at).unwrap();
    copied.advance_to(event.publish_at).unwrap();
    assert_eq!(
        memory.sample_window(&bank, 0, 0),
        Ok(Some(pixel(1, 0, true)))
    );
    assert_eq!(
        snapshot.sample_window(&copied, 0, 0),
        Ok(Some(pixel(1, 0, true)))
    );
    let mut fifo = VdpWriteFifo::default();
    let mut command = ControlCommand {
        address: u32::from(bank.hscroll_table_base().unwrap()),
        code: 1,
    };
    fifo.push_data(&mut command, bank.auto_increment(), 16)
        .unwrap();
    assert_eq!(memory.scroll_offsets(&bank, VdpPlane::A, 0, 0), Ok((0, 0)));
    fifo.complete_write(&mut memory).unwrap();
    command = ControlCommand {
        address: 0,
        code: 5,
    };
    fifo.push_data(&mut command, bank.auto_increment(), 8)
        .unwrap();
    fifo.complete_write(&mut memory).unwrap();
    assert_eq!(memory.scroll_offsets(&bank, VdpPlane::A, 0, 0), Ok((16, 8)));
    command = ControlCommand {
        address: u32::from(bank.plane_a_base().unwrap()) + 64,
        code: 1,
    };
    fifo.push_data(&mut command, bank.auto_increment(), 0xc064)
        .unwrap();
    fifo.complete_write(&mut memory).unwrap();
    assert_eq!(
        memory.sample_scrolled_plane(&bank, VdpPlane::A, 16, 0),
        Ok(pixel(1, 2, true))
    );
    bank.advance_to(MasterTicks::new(bank.now().get() + 1))
        .unwrap();
    let event = bank.queue_register(13, 0x3e).unwrap();
    assert_eq!(memory.scroll_offsets(&bank, VdpPlane::A, 0, 0), Ok((16, 8)));
    bank.advance_to(event.publish_at).unwrap();
    assert_eq!(
        memory.scroll_offsets(&bank, VdpPlane::A, 0, 0),
        Ok((100, 8))
    );
    assert_eq!(
        memory.sample_scrolled_plane(&bank, VdpPlane::A, 16, 0),
        Ok(pixel(0, 0, false))
    );
    assert!(fifo.is_empty());
}
