use zeff_md_core::{ControlCommand, VdpMemory, VdpReadCache, VdpReadError, VdpWrite, VdpWriteFifo};

pub const ALIASES: [u32; 4] = [0, 0x80, 0x8000, 0xff80];
pub const ROWS: [(u16, u16, u16, u16); 4] = [
    (0xffff, 0, 0x0eee, 0x07ff),
    (0, 0xffff, 0xf111, 0xf800),
    (0xffff, 0xffff, 0xffff, 0xffff),
    (0x0400, 0x8800, 0x8400, 0x8c00),
];
pub fn command(address: u32, code: u8) -> ControlCommand {
    ControlCommand { address, code }
}
pub fn push(fifo: &mut VdpWriteFifo, data: u16) {
    fifo.push(VdpWrite {
        command: command(0, 3),
        data,
    })
    .unwrap();
}
pub fn history(memory: &mut VdpMemory, data: u16) -> VdpWriteFifo {
    let mut fifo = VdpWriteFifo::default();
    for _ in 0..4 {
        push(&mut fifo, data);
    }
    for _ in 0..4 {
        fifo.complete_write(memory).unwrap();
    }
    fifo
}

pub fn color_word_masks_and_address_aliases() {
    let mut memory = VdpMemory::default();
    for (stored, raw, cram, vsram) in ROWS {
        let fifo = history(&mut memory, raw);
        let saved_fifo = fifo.clone();
        for (read_code, write_code, words, expected) in [(8, 3, 64, cram), (4, 5, 40, vsram)] {
            for index in 0..words {
                memory
                    .write_word(command(index * 2, write_code), stored)
                    .unwrap();
                for alias in ALIASES {
                    for lane in 0..2 {
                        let address = (index * 2) | alias | lane;
                        let mut current = command(address, read_code);
                        let mut cache = VdpReadCache::default();
                        assert_eq!(
                            cache.complete_read(&memory, &fifo, &mut current, Some(2)),
                            Ok(expected)
                        );
                        assert_eq!(current, command(address + 2, read_code));
                        assert_eq!(cache.read(), Ok(expected));
                        assert_eq!(current, command(address + 2, read_code));
                        assert_eq!(cache.buffered_word(), Some(expected));
                        assert_eq!(fifo, saved_fifo);
                    }
                }
            }
        }
    }
}

pub fn defined_and_inherited_bit_basis() {
    let mut memory = VdpMemory::default();
    for (read_code, write_code, mask) in [(8, 3, 0x0eee), (4, 5, 0x07ff)] {
        for bit in 0..16 {
            let data = 1u16 << bit;
            for (stored, raw, expected) in [(data, 0, data & mask), (0, data, data & !mask)] {
                let fifo = history(&mut memory, raw);
                memory.write_word(command(2, write_code), stored).unwrap();
                let mut cache = VdpReadCache::default();
                let mut current = command(3, read_code);
                assert_eq!(
                    cache.complete_read(&memory, &fifo, &mut current, Some(0)),
                    Ok(expected)
                );
                assert_eq!(cache.read(), Ok(expected));
                assert_eq!(current, command(3, read_code));
            }
        }
    }
}

pub fn retained_fifo_ring_and_cache_ownership() {
    let words = [
        0xa111, 0xb222, 0xc333, 0xd444, 0xe555, 0xf666, 0x8777, 0x9888, 0x1999, 0x2aaa, 0x3bbb,
        0x4ccc,
    ];
    let mut memory = VdpMemory::default();
    let mut fifo = VdpWriteFifo::default();
    for data in &words[..4] {
        push(&mut fifo, *data);
    }
    for _ in 0..4 {
        fifo.complete_write(&mut memory).unwrap();
    }
    for index in 4..words.len() {
        assert_eq!(fifo.next_slot_data(), Some(words[index - 4]));
        memory.write_word(command(2, 3), 0x0666).unwrap();
        let expected = 0x0666 | (words[index - 4] & 0xf111);
        let mut current = command(3, 8);
        let mut cache = VdpReadCache::default();
        cache
            .complete_read(&memory, &fifo, &mut current, Some(255))
            .unwrap();
        assert_eq!(current.address, 258);
        let mut copied = cache;
        push(&mut fifo, words[index]);
        fifo.complete_write(&mut memory).unwrap();
        memory.write_word(command(2, 3), 0).unwrap();
        assert_eq!(cache.buffered_word(), Some(expected));
        assert_eq!(copied.read(), Ok(expected));
        assert!(cache.is_ready());
        assert_eq!(cache.read(), Ok(expected));
        assert_eq!(cache.buffered_word(), Some(expected));
        assert_eq!(current.address, 258);
    }
}

pub fn unknown_history_and_pending_fifo_guards() {
    let mut memory = VdpMemory::default();
    for count in 0..4 {
        let mut fifo = VdpWriteFifo::default();
        for _ in 0..count {
            push(&mut fifo, 0xffff);
            fifo.complete_write(&mut memory).unwrap();
        }
        assert_eq!(fifo.next_slot_data(), None);
        for code in [4, 8] {
            let mut cache = VdpReadCache::default();
            let mut current = command(0, code);
            let saved = fifo.clone();
            assert_eq!(
                cache.complete_read(&memory, &fifo, &mut current, Some(2)),
                Err(VdpReadError::UnknownFifoHistory)
            );
            assert_eq!(cache, VdpReadCache::default());
            assert_eq!(current, command(0, code));
            assert_eq!(fifo, saved);
        }
    }
    let mut fifo = history(&mut memory, 0);
    for (read_code, write_code) in [(8, 3), (4, 5)] {
        memory.write_word(command(0, write_code), 0x0400).unwrap();
        let mut current = command(1, read_code);
        let mut cache = VdpReadCache::default();
        assert_eq!(
            cache.complete_read(&memory, &fifo, &mut current, None),
            Err(VdpReadError::UnknownIncrement)
        );
        assert_eq!(current, command(1, read_code));
        assert_eq!(
            cache.complete_read(&memory, &fifo, &mut current, Some(1)),
            Ok(0x0400)
        );
        let saved = cache;
        assert_eq!(
            cache.complete_read(&memory, &fifo, &mut current, Some(255)),
            Err(VdpReadError::BufferReady)
        );
        assert_eq!(current, command(2, read_code));
        assert_eq!(cache, saved);
    }
    for _ in 0..4 {
        push(&mut fifo, 0xffff);
    }
    while !fifo.is_empty() {
        let saved = fifo.clone();
        for code in [4, 8] {
            let mut cache = VdpReadCache::default();
            let mut current = command(0, code);
            assert_eq!(
                cache.complete_read(&memory, &fifo, &mut current, Some(2)),
                Err(VdpReadError::FifoPending)
            );
            assert_eq!(cache, VdpReadCache::default());
            assert_eq!(current, command(0, code));
            assert_eq!(fifo, saved);
        }
        fifo.complete_write(&mut memory).unwrap();
    }
}

pub fn holes_and_unsupported_targets_preserve_state() {
    let mut memory = VdpMemory::default();
    let fifo = history(&mut memory, 0xffff);
    let saved_fifo = fifo.clone();
    let saved_memory = memory.clone();
    let mut cache = VdpReadCache::default();
    let mut current = command(0, 8);
    cache
        .complete_read(&memory, &fifo, &mut current, Some(0))
        .unwrap();
    cache.read().unwrap();
    let saved = cache;
    for index in 40..64 {
        for alias in ALIASES {
            for lane in 0..2 {
                let address = (index * 2) | alias | lane;
                let mut current = command(address, 4);
                assert_eq!(
                    cache.complete_read(&memory, &fifo, &mut current, Some(255)),
                    Err(VdpReadError::UnsupportedCommand(current))
                );
                assert_eq!(current, command(address, 4));
                assert_eq!(cache, saved);
            }
        }
    }
    for code in 0u8..=255 {
        if [0, 4, 8].contains(&code) {
            continue;
        }
        let mut current = command(0, code);
        assert_eq!(
            cache.complete_read(&memory, &fifo, &mut current, Some(255)),
            Err(VdpReadError::UnsupportedCommand(current))
        );
        assert_eq!(current, command(0, code));
        assert_eq!(cache, saved);
    }
    for address in [0x10000, 0x1ffff, 0x20000, u32::MAX] {
        for code in [0, 4, 8] {
            if address <= 0x1ffff && !(code == 4 && (address & 0x7f) >= 80) {
                let mut alias_cache = VdpReadCache::default();
                let mut alias_command = command(address, code);
                let expected = match code {
                    0 => 0,
                    4 => 0xf800,
                    8 => {
                        if address & 0x7f == 0 {
                            0xffff
                        } else {
                            0xf111
                        }
                    }
                    _ => unreachable!(),
                };
                assert_eq!(
                    alias_cache.complete_read(&memory, &fifo, &mut alias_command, Some(255)),
                    Ok(expected)
                );
                assert_eq!(alias_command.address, (address + 255) & 0x1ffff);
                assert_eq!(alias_cache.read(), Ok(expected));
                continue;
            }
            let mut current = command(address, code);
            assert_eq!(
                cache.complete_read(&memory, &fifo, &mut current, Some(255)),
                Err(VdpReadError::UnsupportedCommand(current))
            );
            assert_eq!(current, command(address, code));
            assert_eq!(cache, saved);
        }
    }
    assert_eq!(fifo, saved_fifo);
    assert_eq!(memory, saved_memory);
}
