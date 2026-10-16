use zeff_md_core::{
    ControlCommand, ControlWrite, VdpControl, VdpMemory, VdpReadCache, VdpReadError, VdpWriteError,
    VdpWriteFifo,
};

pub fn command(address: u32, code: u8) -> ControlCommand {
    ControlCommand { address, code }
}

pub fn first_words_replace_low_fields() {
    for old_code in 0..64 {
        for upper in 0..8 {
            for word in [0x0000, 0x4567, 0xc020, 0x8f02, 0xaf02] {
                let mut decoder = VdpControl::default();
                let mut current = command((upper << 14) | 0x1234, old_code);
                let event = decoder
                    .complete_control_write(&mut current, word, false)
                    .unwrap();
                assert_eq!(current.address, (upper << 14) | u32::from(word & 0x3fff));
                assert_eq!(current.code, (old_code & 0x3c) | (word >> 14) as u8 | 0x10);
                if word & 0xc000 == 0x8000 {
                    assert_eq!(
                        event,
                        ControlWrite::Register {
                            index: 15,
                            value: 2
                        }
                    );
                    assert_eq!(decoder.pending_word(), None);
                } else {
                    assert_eq!(
                        event,
                        ControlWrite::FirstHalf {
                            address_low: word & 0x3fff,
                            code_low: (word >> 14) as u8,
                        }
                    );
                    assert_eq!(decoder.pending_word(), Some(word));
                }
            }
        }
    }
    for (old, word, expected) in [
        (command(0x18000, 0x2d), 0x4567, command(0x18567, 0x3d)),
        (command(0x18000, 0x2d), 0xc020, command(0x18020, 0x3f)),
        (command(0x18000, 0x2d), 0x8f02, command(0x18f02, 0x3e)),
        (command(0x18000, 0x2d), 0xaf02, command(0x1af02, 0x3e)),
        (command(0x12345, 0x3f), 0, command(0x10000, 0x3c)),
    ] {
        let mut current = old;
        VdpControl::default()
            .complete_control_write(&mut current, word, true)
            .unwrap();
        assert_eq!(current, expected);
    }
}

pub fn second_words_gate_dma_bit() {
    for old_code in 0..64 {
        for dma_enabled in [false, true] {
            for code in 0..64u8 {
                if code & 3 == 2 {
                    continue;
                }
                let first = (u16::from(code & 3) << 14) | 0x1234;
                let second = (u16::from(code & 0x3c) << 2) | 7;
                let mut decoder = VdpControl::default();
                let mut current = command(0x18000, old_code);
                decoder
                    .complete_control_write(&mut current, first, dma_enabled)
                    .unwrap();
                assert_eq!(
                    decoder.complete_control_write(&mut current, second, dma_enabled),
                    Ok(ControlWrite::Command(command(0x1d234, code)))
                );
                let applied_code = if dma_enabled {
                    code
                } else {
                    (code & 0x1f) | (old_code & 0x20)
                };
                assert_eq!(current, command(0x1d234, applied_code));
                assert_eq!(decoder.pending_word(), None);
            }
        }
    }
    let mut decoder = VdpControl::default();
    let mut current = command(0, 0);
    decoder
        .complete_control_write(&mut current, 0x4000, false)
        .unwrap();
    assert_eq!(
        decoder.complete_control_write(&mut current, 0x8f02, false),
        Ok(ControlWrite::Command(command(0x8000, 1)))
    );
    assert_eq!(current, command(0x8000, 1));
    assert_eq!(decoder.pending_word(), None);
}

pub fn invalid_images_preserve_pending_command() {
    for pending in [false, true] {
        for old in [
            command(0, 0x40),
            command(0, 0x80),
            command(0, 0xff),
            command(0x20000, 1),
            command(u32::MAX, 1),
        ] {
            let mut decoder = VdpControl::default();
            if pending {
                decoder.write_control(0x4567);
            }
            let saved = decoder;
            let mut current = old;
            let error = if old.code >= 64 {
                VdpWriteError::InvalidCode(old.code)
            } else {
                VdpWriteError::InvalidAddress(old.address)
            };
            assert_eq!(
                decoder.complete_control_write(&mut current, 0x8f02, false),
                Err(error)
            );
            assert_eq!(decoder, saved);
            assert_eq!(current, old);
        }
    }
}

pub fn write_targets_ignore_only_cd4() {
    for (code, expected) in [(0x11, 0xabcd), (0x13, 0x0acc), (0x15, 0x03cd)] {
        let mut memory = VdpMemory::default();
        let mut fifo = VdpWriteFifo::default();
        let mut current = command(0, code);
        let entry = fifo.push_data(&mut current, Some(2), 0xabcd).unwrap();
        assert_eq!(entry.command, command(0, code));
        assert_eq!(current, command(2, code));
        assert_eq!(fifo.complete_write(&mut memory), Ok(Some(entry)));
        match code {
            0x11 => assert_eq!(memory.read_word(command(0, 0)), Ok(expected)),
            0x13 => assert_eq!(memory.cram()[0], expected),
            0x15 => assert_eq!(memory.vsram()[0], expected),
            _ => unreachable!(),
        }
        for index in 0..4 {
            fifo.push_data(&mut current, Some(2), index).unwrap();
        }
        let saved = fifo.clone();
        let before = current;
        assert_eq!(
            fifo.push_data(&mut current, Some(2), 0xffff),
            Err(VdpWriteError::Full)
        );
        assert_eq!(fifo, saved);
        assert_eq!(current, before);
    }
    let mut memory = VdpMemory::default();
    let saved_memory = memory.clone();
    for code in 0..=255u8 {
        if code < 0x20 && !matches!(code & 0xf, 0 | 4 | 8 | 0xc) {
            continue;
        }
        let mut fifo = VdpWriteFifo::default();
        let mut current = command(0, code);
        let expected = if code < 64 {
            VdpWriteError::UnsupportedCommand(current)
        } else {
            VdpWriteError::InvalidCode(code)
        };
        assert_eq!(fifo.push_data(&mut current, Some(2), 0xffff), Err(expected));
        assert_eq!(current, command(0, code));
        assert!(fifo.is_empty());
        assert_eq!(
            memory.write_word(current, 0xffff),
            Err(VdpWriteError::UnsupportedCommand(current))
        );
        assert_eq!(memory, saved_memory);
    }
    for code in [0x10, 0x14, 0x18] {
        let mut current = command(0, code);
        let mut cache = VdpReadCache::default();
        assert_eq!(
            cache.complete_read(&memory, &VdpWriteFifo::default(), &mut current, Some(2)),
            Err(VdpReadError::UnsupportedCommand(current))
        );
        assert_eq!(cache, VdpReadCache::default());
        assert_eq!(current, command(0, code));
    }
}

pub fn register_write_preserves_vsram_target() {
    for register in [false, true] {
        let mut memory = VdpMemory::default();
        let mut fifo = VdpWriteFifo::default();
        for address in [0x8000, 0x8002, 0x8004, 0x8006] {
            memory.write_word(command(address, 1), 0xffff).unwrap();
            let mut current = command(address, 5);
            fifo.push_data(&mut current, Some(2), 0xffff).unwrap();
        }
        for _ in 0..4 {
            fifo.complete_write(&mut memory).unwrap();
        }
        let mut decoder = VdpControl::default();
        let mut current = command(0, 0);
        decoder
            .complete_control_write(&mut current, 0x4000, false)
            .unwrap();
        decoder
            .complete_control_write(&mut current, 0x0012, false)
            .unwrap();
        assert_eq!(current, command(0x8000, 5));
        if register {
            assert_eq!(
                decoder.complete_control_write(&mut current, 0x8f02, false),
                Ok(ControlWrite::Register {
                    index: 15,
                    value: 2
                })
            );
            assert_eq!(current, command(0x8f02, 0x16));
            decoder
                .complete_control_write(&mut current, 0x4000, false)
                .unwrap();
            assert_eq!(current, command(0x8000, 0x15));
        }
        for data in [0x0123, 0x4567] {
            fifo.push_data(&mut current, Some(2), data).unwrap();
            decoder.cancel_pending();
        }
        for _ in 0..2 {
            fifo.complete_write(&mut memory).unwrap();
        }
        for address in [0x8000, 0x8002] {
            assert_eq!(memory.read_word(command(address, 0)), Ok(0xffff));
        }
        decoder
            .complete_control_write(&mut current, 0x0000, false)
            .unwrap();
        decoder
            .complete_control_write(&mut current, 0x0012, false)
            .unwrap();
        assert_eq!(current, command(0x8000, 4));
        let mut cache = VdpReadCache::default();
        for expected in [0xf923, 0xfd67] {
            cache
                .complete_read(&memory, &fifo, &mut current, Some(2))
                .unwrap();
            assert_eq!(cache.read(), Ok(expected));
        }
    }
}
