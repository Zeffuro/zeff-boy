use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, VdpMemory, VdpPixel, VdpPlane, VdpRegisterPipeline, VdpRenderError, VdpTileRow,
    VdpWriteFifo,
};

pub fn write(memory: &mut VdpMemory, address: u32, data: u16) {
    memory
        .write_word(ControlCommand { address, code: 1 }, data)
        .unwrap();
}

pub fn publish(bank: &mut VdpRegisterPipeline, index: u8, value: u8) {
    bank.advance_to(MasterTicks::new(bank.now().get() + 1))
        .unwrap();
    let event = bank.queue_register(index, value).unwrap();
    assert_eq!(bank.advance_to(event.publish_at), Ok(Some(event)));
}

pub fn bank() -> VdpRegisterPipeline {
    let mut bank = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    for (index, value) in [(1, 4), (12, 0), (2, 0x30), (4, 7), (16, 0x11), (15, 2)] {
        publish(&mut bank, index, value);
    }
    bank
}

pub fn pixel(color: u8, palette: u8, priority: bool) -> VdpPixel {
    VdpPixel {
        color,
        palette,
        priority,
    }
}

fn byte_at(address: u32) -> u8 {
    (address * 17 + address / 256 + address / 4096) as u8
}

pub fn tile_rows_cover_vram() {
    let mut memory = VdpMemory::default();
    for address in (0..65536).step_by(2) {
        write(
            &mut memory,
            address,
            u16::from_be_bytes([byte_at(address), byte_at(address + 1)]),
        );
    }
    for tile in 0..2048u16 {
        for row in 0..8u8 {
            let address = u32::from(tile) * 32 + u32::from(row) * 4;
            let expected = std::array::from_fn(|x| {
                let byte = byte_at(address + (x / 2) as u32);
                if x % 2 == 0 { byte / 16 } else { byte % 16 }
            });
            assert_eq!(
                memory.tile_row(tile, row),
                Ok(VdpTileRow {
                    colors: expected,
                    palette: 0,
                    priority: false,
                })
            );
        }
    }
    write(&mut memory, 0xfffc, 0x0123);
    write(&mut memory, 0xfffe, 0x4567);
    assert_eq!(
        memory.tile_row(0xffff, 0),
        Ok(VdpTileRow {
            colors: [7, 6, 5, 4, 3, 2, 1, 0],
            palette: 3,
            priority: true,
        })
    );
}

pub fn tile_attributes_and_nibbles() {
    let mut memory = VdpMemory::default();
    for byte in 0..=255u16 {
        for address in [32, 34] {
            write(&mut memory, address, byte * 257);
        }
        assert_eq!(
            memory.tile_row(1, 0).unwrap().colors,
            [byte as u8 / 16, byte as u8 % 16].repeat(4).as_slice()
        );
    }
    let words = [
        [0x0123, 0x4567],
        [0x89ab, 0xcdef],
        [0xf0e1, 0xd2c3],
        [0x1357, 0x9bdf],
        [0x2468, 0xace0],
        [0x9876, 0x5432],
        [0x5a6b, 0x7c8d],
        [0xe4d3, 0xc2b1],
    ];
    let rows = [
        [0, 1, 2, 3, 4, 5, 6, 7],
        [8, 9, 10, 11, 12, 13, 14, 15],
        [15, 0, 14, 1, 13, 2, 12, 3],
        [1, 3, 5, 7, 9, 11, 13, 15],
        [2, 4, 6, 8, 10, 12, 14, 0],
        [9, 8, 7, 6, 5, 4, 3, 2],
        [5, 10, 6, 11, 7, 12, 8, 13],
        [14, 4, 13, 3, 12, 2, 11, 1],
    ];
    for (row, words) in words.into_iter().enumerate() {
        for (word, data) in words.into_iter().enumerate() {
            write(&mut memory, 32 + (row * 4 + word * 2) as u32, data);
        }
    }
    for attributes in 0..32u16 {
        let name = 1 + attributes * 2048;
        for row in 0..8u8 {
            let source_row = if attributes & 2 != 0 { 7 - row } else { row };
            let expected = std::array::from_fn(|x| {
                rows[usize::from(source_row)][if attributes & 1 != 0 { 7 - x } else { x }]
            });
            assert_eq!(
                memory.tile_row(name, row),
                Ok(VdpTileRow {
                    colors: expected,
                    palette: (attributes / 4 % 4) as u8,
                    priority: attributes >= 16,
                })
            );
        }
    }
}

pub fn invalid_rows_preserve_memory() {
    let mut memory = VdpMemory::default();
    write(&mut memory, 0, 0x1357);
    write(&mut memory, 65534, 0x9bdf);
    let saved = memory.clone();
    for row in 8..=255 {
        for name in [0, 0x7ff, 0xffff] {
            assert_eq!(
                memory.tile_row(name, row),
                Err(VdpRenderError::InvalidRow(row))
            );
        }
    }
    assert_eq!(memory, saved);
}

pub fn dimensions(value: u8) -> Option<(u16, u16)> {
    match value & 0x33 {
        0x00 => Some((32, 32)),
        0x10 => Some((32, 64)),
        0x30 => Some((32, 128)),
        0x01 => Some((64, 32)),
        0x11 => Some((64, 64)),
        0x03 => Some((128, 32)),
        _ => None,
    }
}

pub fn plane_sizes_and_bounds() {
    let memory = VdpMemory::default();
    let mut unknown = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    for (plane, index, value) in [(VdpPlane::A, 2, 0x30), (VdpPlane::B, 4, 7)] {
        assert_eq!(
            memory.sample_plane(&unknown, plane, 0, 0),
            Err(VdpRenderError::UnknownRegister(index))
        );
        publish(&mut unknown, index, value);
        assert_eq!(
            memory.sample_plane(&unknown, plane, 0, 0),
            Err(VdpRenderError::UnknownRegister(16))
        );
    }
    let mut bank = bank();
    let mut accepted = 0;
    let mut unsupported = 0;
    for value in 0..=255u8 {
        publish(&mut bank, 16, value);
        let saved = bank;
        for plane in [VdpPlane::A, VdpPlane::B] {
            if let Some((width, height)) = dimensions(value) {
                for (x, y) in [(0, 0), (width * 8 - 1, height * 8 - 1)] {
                    assert_eq!(
                        memory.sample_plane(&bank, plane, x, y),
                        Ok(pixel(0, 0, false))
                    );
                }
                for (x, y) in [(width * 8, 0), (0, height * 8), (u16::MAX, u16::MAX)] {
                    assert_eq!(
                        memory.sample_plane(&bank, plane, x, y),
                        Err(VdpRenderError::CoordinateOutOfBounds { x, y })
                    );
                }
            } else {
                assert_eq!(
                    memory.sample_plane(&bank, plane, 0, 0),
                    Err(VdpRenderError::UnsupportedPlaneSize(value))
                );
                assert_eq!(
                    memory.sample_plane(&bank, plane, u16::MAX, u16::MAX),
                    Err(VdpRenderError::UnsupportedPlaneSize(value))
                );
            }
        }
        assert_eq!(bank, saved);
        if dimensions(value).is_some() {
            accepted += 1;
        } else {
            unsupported += 1;
        }
    }
    assert_eq!((accepted, unsupported), (96, 160));
}

pub fn plane_table_strides_and_limits() {
    let mut bank = bank();
    for size in [0x00, 0x10, 0x30, 0x01, 0x11, 0x03] {
        let (width, height) = dimensions(size).unwrap();
        publish(&mut bank, 16, size);
        for base in 0..8u8 {
            publish(&mut bank, 2, base * 8);
            publish(&mut bank, 4, base);
            let mut memory = VdpMemory::default();
            write(&mut memory, 0xffa0, 0x1234);
            write(&mut memory, 0xffa2, 0x5678);
            write(&mut memory, 0xffc0, 0x9876);
            write(&mut memory, 0xffc2, 0x5432);
            let address = u32::from(base) * 8192;
            let last = u32::from(width) * u32::from(height) * 2 - 2;
            for offset in [0, 2, u32::from(width) * 2, last] {
                write(
                    &mut memory,
                    address + offset,
                    if offset == 2 { 0x27fe } else { 0x87fd },
                );
            }
            for plane in [VdpPlane::A, VdpPlane::B] {
                for (x, y, expected) in [
                    (0, 0, pixel(1, 0, true)),
                    (8, 0, pixel(9, 1, false)),
                    (0, 8, pixel(1, 0, true)),
                    ((width - 1) * 8, (height - 1) * 8, pixel(1, 0, true)),
                ] {
                    assert_eq!(memory.sample_plane(&bank, plane, x, y), Ok(expected));
                }
            }
            if size == 0x11 && base == 7 {
                assert_eq!(&memory.vram()[0xfffe..], &[0x87, 0xfd]);
                assert_eq!(
                    memory.sample_plane(&bank, VdpPlane::B, 504, 504),
                    Ok(pixel(1, 0, true))
                );
            }
        }
    }
}

pub fn published_bank_and_fifo_feed_pixels() {
    let mut bank = bank();
    let mut memory = VdpMemory::default();
    for (address, data) in [
        (0x28, 0x0123),
        (0x2a, 0x4567),
        (0x34, 0x9876),
        (0x36, 0x5432),
    ] {
        write(&mut memory, address, data);
    }
    let mut fifo = VdpWriteFifo::default();
    let mut command = ControlCommand {
        address: u32::from(bank.plane_a_base().unwrap()) + 0x82,
        code: 1,
    };
    fifo.push_data(&mut command, bank.auto_increment(), 0xe801)
        .unwrap();
    assert_eq!(command.address, 0xc084);
    assert_eq!(
        memory.sample_plane(&bank, VdpPlane::A, 8, 10),
        Ok(pixel(0, 0, false))
    );
    assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
    assert_eq!(&memory.vram()[0xc082..0xc084], &[0, 1]);
    assert_eq!(
        memory.sample_plane(&bank, VdpPlane::A, 8, 10),
        Ok(pixel(0, 0, false))
    );
    assert!(fifo.service_write_slot(&mut memory).unwrap().is_some());
    assert_eq!(
        memory.sample_plane(&bank, VdpPlane::A, 8, 10),
        Ok(pixel(7, 3, true))
    );
    assert_eq!(
        memory.sample_plane(&bank, VdpPlane::A, 15, 10),
        Ok(pixel(0, 3, true))
    );
    command.address = u32::from(bank.plane_b_base().unwrap()) + 0x82;
    fifo.push_data(&mut command, bank.auto_increment(), 0x3801)
        .unwrap();
    fifo.complete_write(&mut memory).unwrap();
    assert_eq!(
        memory.sample_plane(&bank, VdpPlane::B, 8, 10),
        Ok(pixel(2, 1, false))
    );
    assert_eq!(
        memory.sample_plane(&bank, VdpPlane::B, 15, 10),
        Ok(pixel(9, 1, false))
    );
    bank.advance_to(MasterTicks::new(121)).unwrap();
    let event = bank.queue_register(2, 0x38).unwrap();
    let mut cloned_bank = bank;
    let cloned_memory = memory.clone();
    assert_eq!(
        memory.sample_plane(&bank, VdpPlane::A, 8, 10),
        Ok(pixel(7, 3, true))
    );
    bank.advance_to(MasterTicks::new(139)).unwrap();
    assert_eq!(
        memory.sample_plane(&bank, VdpPlane::A, 8, 10),
        Ok(pixel(7, 3, true))
    );
    bank.advance_to(event.publish_at).unwrap();
    cloned_bank.advance_to(event.publish_at).unwrap();
    assert_eq!(
        memory.sample_plane(&bank, VdpPlane::A, 8, 10),
        Ok(pixel(2, 1, false))
    );
    assert_eq!(
        cloned_memory.sample_plane(&cloned_bank, VdpPlane::A, 8, 10),
        Ok(pixel(2, 1, false))
    );
    publish(&mut bank, 16, 0);
    assert_eq!(
        memory.sample_plane(&bank, VdpPlane::A, 8, 10),
        Ok(pixel(0, 0, false))
    );
    assert_eq!(memory, cloned_memory);
}

pub fn transparent_pixels_keep_attributes() {
    let mut bank = bank();
    let mut memory = VdpMemory::default();
    for color in 0..16u16 {
        write(&mut memory, 32, color * 0x1111);
        for palette in 0..4u16 {
            for priority in [false, true] {
                let name = 1 + palette * 0x2000 + if priority { 0x8000 } else { 0 };
                write(&mut memory, 0xc000, name);
                for x in 0..4 {
                    let sampled = memory.sample_plane(&bank, VdpPlane::A, x, 0).unwrap();
                    assert_eq!(sampled, pixel(color as u8, palette as u8, priority));
                    assert_eq!(sampled.is_transparent(), color == 0);
                }
            }
        }
    }
    publish(&mut bank, 2, 0x38);
    let saved = bank;
    assert_eq!(
        memory.sample_plane(&bank, VdpPlane::A, 0, 0),
        Ok(pixel(0, 0, false))
    );
    assert_eq!(bank, saved);
}
