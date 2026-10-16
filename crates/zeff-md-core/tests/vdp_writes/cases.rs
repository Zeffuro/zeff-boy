use zeff_md_core::{ControlCommand, VdpMemory, VdpWrite, VdpWriteError, VdpWriteFifo};

pub const VRAM_ADDRESSES: [u16; 10] = [
    0, 1, 0xfe, 0xff, 0x3ffe, 0x3fff, 0x8000, 0x8001, 0xfffe, 0xffff,
];
pub const ALIASES: [u16; 4] = [0, 0x80, 0x8000, 0xff80];

pub fn data_values() -> [u16; 18] {
    let mut values = [0; 18];
    for (bit, value) in values[..16].iter_mut().enumerate() {
        *value = 1 << bit;
    }
    values[17] = 0xffff;
    values
}

pub fn entry(code: u8, address: u16, data: u16) -> VdpWrite {
    VdpWrite {
        command: ControlCommand {
            address: u32::from(address),
            code,
        },
        data,
    }
}

pub fn vram_word_lanes_and_edges() {
    let mut memory = VdpMemory::default();
    assert_eq!(memory.vram().len(), 65536);
    for (address, data, expected) in [
        (0x0000, 0x1234, [0x12, 0x34]),
        (0x0001, 0x1234, [0x34, 0x12]),
        (0x00ff, 0xabcd, [0xcd, 0xab]),
        (0x3fff, 0x55aa, [0xaa, 0x55]),
        (0x8000, 0x6789, [0x67, 0x89]),
        (0xffff, 0xabcd, [0xcd, 0xab]),
    ] {
        let saved = memory.clone();
        memory
            .write_word(entry(1, address, data).command, data)
            .unwrap();
        let index = usize::from(address & !1);
        assert_eq!(&memory.vram()[index..index + 2], &expected);
        assert_eq!(&memory.vram()[..index], &saved.vram()[..index]);
        assert_eq!(&memory.vram()[index + 2..], &saved.vram()[index + 2..]);
        assert_eq!(memory.cram(), saved.cram());
        assert_eq!(memory.vsram(), saved.vsram());
    }
    assert_eq!(&memory.vram()[..2], &[0x34, 0x12]);
    for address in VRAM_ADDRESSES {
        for data in data_values() {
            memory
                .write_word(entry(1, address, data).command, data)
                .unwrap();
            let bytes = data.to_be_bytes();
            let expected = if address & 1 == 0 {
                bytes
            } else {
                [bytes[1], bytes[0]]
            };
            let index = usize::from(address & !1);
            assert_eq!(&memory.vram()[index..index + 2], &expected);
        }
    }
}

pub fn color_word_masks_and_aliases() {
    let mut memory = VdpMemory::default();
    assert_eq!(memory.cram().len(), 64);
    assert_eq!(memory.vsram().len(), 40);
    for (data, cram, vsram) in [
        (0xffff, 0x0eee, 0x07ff),
        (0x0400, 0x0400, 0x0400),
        (0x0111, 0, 0x0111),
        (0xf000, 0, 0),
        (0, 0, 0),
    ] {
        for index in 0..64 {
            for alias in ALIASES {
                for lane in 0..2 {
                    let address = (index * 2) | alias | lane;
                    memory
                        .write_word(entry(3, address, data).command, data)
                        .unwrap();
                    assert_eq!(memory.cram()[usize::from(index)], cram);
                    if index < 40 {
                        memory
                            .write_word(entry(5, address, data).command, data)
                            .unwrap();
                        assert_eq!(memory.vsram()[usize::from(index)], vsram);
                    }
                }
            }
        }
    }
    memory
        .write_word(entry(5, 0x4e, 0x0456).command, 0x0456)
        .unwrap();
    let saved = memory.clone();
    for index in 40..64 {
        for alias in ALIASES {
            for lane in 0..2 {
                let command = entry(5, (index * 2) | alias | lane, 0xffff).command;
                assert_eq!(memory.write_word(command, 0xffff), Ok(()));
                assert_eq!(memory, saved);
            }
        }
    }
}

pub fn fifo_capture_order_and_reuse() {
    let mut memory = VdpMemory::default();
    let mut fifo = VdpWriteFifo::default();
    assert_eq!(fifo.front(), None);
    assert_eq!(fifo.next_slot_data(), None);
    assert_eq!(fifo.complete_write(&mut memory), Ok(None));
    let writes = [
        entry(1, 0, 0x1234),
        entry(3, 0x83, 0xffff),
        entry(5, 0x4f, 0x0400),
        entry(1, 0xffff, 0xabcd),
    ];
    let saved = memory.clone();
    for (index, write) in writes.iter().enumerate() {
        fifo.push(*write).unwrap();
        assert_eq!(fifo.len(), index + 1);
        assert_eq!(fifo.front(), Some(writes[0]));
        assert_eq!(memory, saved);
    }
    assert!(fifo.is_full());
    assert_eq!(fifo.next_slot_data(), Some(0x1234));
    assert_eq!(fifo.complete_write(&mut memory), Ok(Some(writes[0])));
    assert_eq!(&memory.vram()[..2], &[0x12, 0x34]);
    assert_eq!(memory.cram()[1], 0);
    assert_eq!(memory.vsram()[39], 0);
    assert_eq!(fifo.front(), Some(writes[1]));
    assert_eq!(fifo.next_slot_data(), Some(0x1234));
    let fifth = entry(1, 2, 0x9876);
    fifo.push(fifth).unwrap();
    assert_eq!(fifo.next_slot_data(), Some(0xffff));
    for write in [writes[1], writes[2], writes[3], fifth] {
        assert_eq!(fifo.complete_write(&mut memory), Ok(Some(write)));
    }
    assert!(fifo.is_empty());
    assert_eq!(fifo.len(), 0);
    assert_eq!(fifo.front(), None);
    assert_eq!(fifo.next_slot_data(), Some(0xffff));
    assert_eq!(memory.cram()[1], 0x0eee);
    assert_eq!(memory.vsram()[39], 0x0400);
    assert_eq!(&memory.vram()[2..4], &[0x98, 0x76]);
    assert_eq!(&memory.vram()[0xfffe..], &[0xcd, 0xab]);
    let saved = memory.clone();
    assert_eq!(fifo.complete_write(&mut memory), Ok(None));
    assert_eq!(memory, saved);
    let writes = [
        entry(1, 2, 0x1357),
        entry(3, 2, 0xf246),
        entry(5, 2, 0x0456),
        entry(1, 2, 0xa1b2),
    ];
    for write in writes {
        fifo.push(write).unwrap();
    }
    assert_eq!(memory, saved);
    assert_eq!(fifo.complete_write(&mut memory), Ok(Some(writes[0])));
    assert_eq!(&memory.vram()[2..4], &[0x13, 0x57]);
    assert_eq!(memory.cram()[1], saved.cram()[1]);
    assert_eq!(memory.vsram()[1], saved.vsram()[1]);
    assert_eq!(fifo.complete_write(&mut memory), Ok(Some(writes[1])));
    assert_eq!(memory.cram()[1], 0x0246);
    assert_eq!(&memory.vram()[2..4], &[0x13, 0x57]);
    assert_eq!(memory.vsram()[1], saved.vsram()[1]);
    assert_eq!(fifo.complete_write(&mut memory), Ok(Some(writes[2])));
    assert_eq!(memory.vsram()[1], 0x0456);
    assert_eq!(memory.cram()[1], 0x0246);
    assert_eq!(&memory.vram()[2..4], &[0x13, 0x57]);
    assert_eq!(fifo.complete_write(&mut memory), Ok(Some(writes[3])));
    assert_eq!(&memory.vram()[2..4], &[0xa1, 0xb2]);
    assert_eq!(memory.cram()[1], 0x0246);
    assert_eq!(memory.vsram()[1], 0x0456);
    assert!(fifo.is_empty());
}

pub fn full_and_invalid_pushes_are_atomic() {
    let mut fifo = VdpWriteFifo::default();
    let mut memory = VdpMemory::default();
    for index in 0..4 {
        fifo.push(entry(1, index * 2, 0x1111 + index)).unwrap();
    }
    let saved = fifo.clone();
    for _ in 0..3 {
        assert_eq!(fifo.push(entry(1, 8, 0xffff)), Err(VdpWriteError::Full));
        assert_eq!(fifo, saved);
    }
    for code in [0x40, 0x80, 0xff] {
        assert_eq!(
            fifo.push(entry(code, 8, 0xffff)),
            Err(VdpWriteError::InvalidCode(code))
        );
        assert_eq!(fifo, saved);
    }
    assert_eq!(
        fifo.complete_write(&mut memory),
        Ok(Some(entry(1, 0, 0x1111)))
    );
    fifo.push(entry(1, 8, 0xffff)).unwrap();
    assert!(fifo.is_full());
    assert_eq!(fifo.front(), Some(entry(1, 2, 0x1112)));
}

pub fn unsupported_completions_preserve_memory_and_queue() {
    for write in [
        entry(0, 0, 0xffff),
        entry(4, 0, 0xffff),
        entry(8, 0, 0xffff),
        entry(0x0c, 0, 0xffff),
        entry(0x21, 0, 0xffff),
        entry(0x3f, 0, 0xffff),
    ] {
        let mut memory = VdpMemory::default();
        memory
            .write_word(entry(1, 0, 0x1234).command, 0x1234)
            .unwrap();
        let mut fifo = VdpWriteFifo::default();
        fifo.push(write).unwrap();
        fifo.push(entry(1, 2, 0xabcd)).unwrap();
        let saved = (memory.clone(), fifo.clone());
        for _ in 0..2 {
            assert_eq!(
                fifo.complete_write(&mut memory),
                Err(VdpWriteError::UnsupportedCommand(write.command))
            );
            assert_eq!(memory, saved.0);
            assert_eq!(fifo, saved.1);
            assert_eq!(fifo.front(), Some(write));
            assert_eq!(fifo.len(), 2);
        }
    }
    for code in [5, 0x15] {
        for address in [0x50, 0x51, 0x7e, 0x7f, 0xffff] {
            let mut memory = VdpMemory::default();
            memory
                .write_word(entry(5, 0x4e, 0x0123).command, 0x0123)
                .unwrap();
            let saved = memory.clone();
            let discarded = entry(code, address, 0xffff);
            let next = entry(1, 2, 0xabcd);
            let mut fifo = VdpWriteFifo::default();
            fifo.push(discarded).unwrap();
            fifo.push(next).unwrap();
            assert_eq!(fifo.complete_write(&mut memory), Ok(Some(discarded)));
            assert_eq!(memory, saved);
            assert_eq!(fifo.len(), 1);
            assert_eq!(fifo.front(), Some(next));
            assert_eq!(fifo.complete_write(&mut memory), Ok(Some(next)));
            assert!(fifo.is_empty());
            assert_eq!(&memory.vram()[2..4], &[0xab, 0xcd]);
            assert_eq!(memory.vsram(), saved.vsram());
        }
    }
}

pub fn copied_pending_writes_and_retained_slots() {
    let mut memory = VdpMemory::default();
    let mut fifo = VdpWriteFifo::default();
    let mut write = entry(1, 1, 0x1234);
    fifo.push(write).unwrap();
    write.command.address = 0x8000;
    write.data = 0xabcd;
    fifo.push(write).unwrap();
    assert_eq!(fifo.front(), Some(entry(1, 1, 0x1234)));
    let mut copied_fifo = fifo.clone();
    let mut copied_memory = memory.clone();
    for index in 0..2 {
        assert_eq!(
            fifo.complete_write(&mut memory),
            copied_fifo.complete_write(&mut copied_memory)
        );
        assert_eq!(fifo, copied_fifo);
        assert_eq!(memory, copied_memory);
        if index == 0 {
            copied_fifo = fifo.clone();
            copied_memory = memory.clone();
        }
    }
    assert_eq!(&memory.vram()[..2], &[0x34, 0x12]);
    assert_eq!(&memory.vram()[0x8000..0x8002], &[0xab, 0xcd]);
    memory
        .write_word(entry(1, 0, 0xffff).command, 0xffff)
        .unwrap();
    assert_eq!(&copied_memory.vram()[..2], &[0x34, 0x12]);
}

pub fn fifo_history_retains_unmasked_data() {
    let mut memory = VdpMemory::default();
    let mut fifo = VdpWriteFifo::default();
    for round in 0..32u16 {
        for index in 0..4 {
            let data = 0xf111 + round * 4 + index;
            assert_eq!(
                fifo.next_slot_data(),
                if round == 0 { None } else { Some(data - 4) }
            );
            fifo.push(entry(3, index * 2, data)).unwrap();
        }
        for index in 0..4 {
            let data = 0xf111 + round * 4 + index;
            assert_eq!(
                fifo.complete_write(&mut memory),
                Ok(Some(entry(3, index * 2, data)))
            );
            assert_eq!(memory.cram()[usize::from(index)], data & 0x0eee);
        }
        assert_eq!(fifo.next_slot_data(), Some(0xf111 + round * 4));
    }
}
