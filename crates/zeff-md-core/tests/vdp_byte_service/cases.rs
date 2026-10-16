use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, VdpMemory, VdpReadCache, VdpReadError, VdpRegisterPipeline, VdpWrite,
    VdpWriteError, VdpWriteFifo,
};

pub fn command(address: u32, code: u8) -> ControlCommand {
    ControlCommand { address, code }
}
pub fn entry(address: u32, code: u8, data: u16) -> VdpWrite {
    VdpWrite {
        command: command(address, code),
        data,
    }
}

pub fn vram_bytes_publish_in_order() {
    for code in [1, 0x11] {
        for (address, first, last) in [
            (0, [0xaa, 0x34], [0x12, 0x34]),
            (1, [0xaa, 0x12], [0x34, 0x12]),
            (0xfffe, [0xaa, 0x34], [0x12, 0x34]),
            (0xffff, [0xaa, 0x12], [0x34, 0x12]),
        ] {
            let mut memory = VdpMemory::default();
            let base = address as usize & !1;
            memory.write_word(command(base as u32, 1), 0xaabb).unwrap();
            memory
                .write_word(command((base as u32) ^ 2, 1), 0x5678)
                .unwrap();
            let before = memory.clone();
            let mut fifo = VdpWriteFifo::default();
            let mut current = command(address, code);
            let mut registers = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
            registers.advance_to(MasterTicks::new(1)).unwrap();
            registers.queue_register(15, 2).unwrap();
            registers.advance_to(MasterTicks::new(20)).unwrap();
            let captured = fifo
                .push_data(&mut current, registers.auto_increment(), 0x1234)
                .unwrap();
            assert_eq!(current, command(address + 2, code));
            current = command(0x10000, 0x3f);
            let history = fifo.next_slot_data();
            assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
            assert_eq!(&memory.vram()[base..base + 2], &first);
            assert_eq!(&memory.vram()[..base], &before.vram()[..base]);
            assert_eq!(&memory.vram()[base + 2..], &before.vram()[base + 2..]);
            assert_eq!(memory.cram(), before.cram());
            assert_eq!(memory.vsram(), before.vsram());
            registers.advance_to(MasterTicks::new(21)).unwrap();
            registers.queue_register(15, 255).unwrap();
            registers.advance_to(MasterTicks::new(40)).unwrap();
            assert_eq!(registers.auto_increment(), Some(255));
            assert_eq!(fifo.front(), Some(captured));
            assert_eq!(fifo.len(), 1);
            assert_eq!(fifo.status_bits(), 0);
            assert_eq!(fifo.next_slot_data(), history);
            let mut read = command(address, 0);
            let mut cache = VdpReadCache::default();
            assert_eq!(
                cache.complete_read(&memory, &fifo, &mut read, Some(2)),
                Err(VdpReadError::FifoPending)
            );
            assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(captured)));
            assert_eq!(&memory.vram()[base..base + 2], &last);
            assert_eq!(&memory.vram()[..base], &before.vram()[..base]);
            assert_eq!(&memory.vram()[base + 2..], &before.vram()[base + 2..]);
            assert_eq!(memory.cram(), before.cram());
            assert_eq!(memory.vsram(), before.vsram());
            assert_eq!(fifo.status_bits(), 0x0200);
            assert_eq!(current, command(0x10000, 0x3f));
            assert_eq!(
                cache.complete_read(&memory, &fifo, &mut read, Some(2)),
                Ok(u16::from_be_bytes(last))
            );
            assert_eq!(cache.read(), Ok(u16::from_be_bytes(last)));
            assert_eq!(read.address, address + 2);
            let saved = memory.clone();
            assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
            assert_eq!(memory, saved);
        }
    }
}

pub fn partial_heads_preserve_occupancy_and_clones() {
    for code in [3, 0x13, 5, 0x15] {
        let mut memory = VdpMemory::default();
        let mut fifo = VdpWriteFifo::default();
        for address in [0, 1, 78, 79] {
            for data in [0, 0xffff] {
                let write = entry(address, code, data);
                fifo.push(write).unwrap();
                assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(write)));
                assert!(fifo.is_empty());
                let index = (address >> 1) as usize;
                if code & 0x0f == 3 {
                    assert_eq!(memory.cram()[index], data & 0x0eee);
                } else {
                    assert_eq!(memory.vsram()[index], data & 0x07ff);
                }
            }
        }
    }
    for warmup in 0..4 {
        let mut memory = VdpMemory::default();
        let mut fifo = VdpWriteFifo::default();
        for _ in 0..warmup {
            let write = entry(0, 3, 0xffff);
            fifo.push(write).unwrap();
            assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(write)));
        }
        for round in 0..2 {
            let writes = [
                entry(0x20, 1, 0x1234 + round),
                entry(2, 0x13, 0xfabc),
                entry(0x23, 0x11, 0x5678 + round),
                entry(6, 0x15, 0xfd67),
            ];
            for write in writes {
                fifo.push(write).unwrap();
            }
            assert_eq!(fifo.next_slot_data(), Some(writes[0].data));
            for (index, write) in writes.into_iter().enumerate() {
                if write.command.code & 0x0f == 1 {
                    let count = fifo.len();
                    let flags = fifo.status_bits();
                    let history = fifo.next_slot_data();
                    assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
                    assert_eq!(fifo.len(), count);
                    assert_eq!(fifo.status_bits(), flags);
                    assert_eq!(fifo.next_slot_data(), history);
                    if index == 0 {
                        let saved = fifo.clone();
                        assert_eq!(fifo.push(entry(0x40, 1, 0)), Err(VdpWriteError::Full));
                        assert_eq!(fifo, saved);
                    }
                    let mut copied_fifo = fifo.clone();
                    let mut copied_memory = memory.clone();
                    assert_eq!(
                        copied_fifo.complete_write(&mut copied_memory),
                        Ok(Some(write))
                    );
                    assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(write)));
                    assert_eq!(copied_fifo, fifo);
                    assert_eq!(copied_memory, memory);
                } else {
                    assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(write)));
                }
                assert_eq!(fifo.len(), 3 - index);
            }
            assert_eq!(fifo.next_slot_data(), Some(writes[0].data));
            assert_eq!(memory.cram()[1], 0x0aac);
            assert_eq!(memory.vsram()[3], 0x0567);
            assert_eq!(memory.read_word(command(0x20, 0)), Ok(0x1234 + round));
            assert_eq!(
                memory.read_word(command(0x22, 0)),
                Ok((0x5678u16 + round).swap_bytes())
            );
        }
    }
    let mut fifo = VdpWriteFifo::default();
    let mut memory = VdpMemory::default();
    let writes = [
        entry(0, 1, 0x1111),
        entry(2, 1, 0x2222),
        entry(4, 1, 0x3333),
        entry(6, 1, 0x4444),
    ];
    for write in writes {
        fifo.push(write).unwrap();
    }
    assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
    assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(writes[0])));
    assert_eq!(fifo.len(), 3);
    assert_eq!(fifo.next_slot_data(), Some(0x1111));
    let fifth = entry(8, 0x11, 0x5555);
    fifo.push(fifth).unwrap();
    assert_eq!(fifo.status_bits(), 0x0100);
    assert_eq!(fifo.front(), Some(writes[1]));
    for write in [writes[1], writes[2], writes[3], fifth] {
        assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
        assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(write)));
        assert_eq!(
            memory.read_word(command(write.command.address, 0)),
            Ok(write.data)
        );
    }
    assert_eq!(fifo.next_slot_data(), Some(0x2222));
    assert_eq!(fifo.status_bits(), 0x0200);
}

pub fn completed_api_does_not_replay_first_byte() {
    for finish_with_slot in [false, true] {
        for code in [1, 0x11] {
            for (address, expected) in [(0, 0x1299), (1, 0x3499)] {
                let mut memory = VdpMemory::default();
                let mut fifo = VdpWriteFifo::default();
                let write = entry(address, code, 0x1234);
                fifo.push(write).unwrap();
                assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
                memory.write_word(command(0, 1), 0xee99).unwrap();
                let result = if finish_with_slot {
                    fifo.service_write_slot(&mut memory)
                } else {
                    fifo.complete_write(&mut memory)
                };
                assert_eq!(result, Ok(Some(write)));
                assert_eq!(memory.read_word(command(0, 0)), Ok(expected));
                assert!(fifo.is_empty());
                let next = entry(2, 1, 0xabcd);
                fifo.push(next).unwrap();
                assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
                assert_eq!(memory.read_word(command(2, 0)), Ok(0x00cd));
                assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(next)));
                assert_eq!(memory.read_word(command(2, 0)), Ok(0xabcd));
            }
        }
    }
}

pub fn rejected_service_preserves_state() {
    let mut memory = VdpMemory::default();
    memory.write_word(command(0, 1), 0xaabb).unwrap();
    for code in 0..64 {
        if code < 0x20 && !matches!(code & 0xf, 0 | 4 | 8 | 0xc) {
            continue;
        }
        let write = entry(0, code, 0xffff);
        let mut fifo = VdpWriteFifo::default();
        fifo.push(write).unwrap();
        let saved_fifo = fifo.clone();
        let saved_memory = memory.clone();
        for _ in 0..2 {
            assert_eq!(
                fifo.service_write_slot(&mut memory),
                Err(VdpWriteError::UnsupportedCommand(write.command))
            );
            assert_eq!(fifo, saved_fifo);
            assert_eq!(memory, saved_memory);
        }
    }
    for write in [
        entry(0x10000, 1, 0xffff),
        entry(0x1ffff, 0x11, 0xffff),
        entry(0x10000, 3, 0xffff),
        entry(0x10000, 5, 0xffff),
        entry(80, 5, 0xffff),
        entry(126, 0x15, 0xffff),
    ] {
        let mut fifo = VdpWriteFifo::default();
        fifo.push(write).unwrap();
        let saved_memory = memory.clone();
        if write.command.code & !0x10 == 1 {
            assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
        }
        assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(write)));
        assert!(fifo.is_empty());
        if write.command.address > 0xffff {
            match write.command.code & !0x10 {
                1 => assert_eq!(
                    &memory.vram()[(write.command.address as usize & 0xffff) & !1..][..2],
                    &[0xff, 0xff]
                ),
                3 => assert_eq!(memory.cram()[0], 0x0eee),
                5 => assert_eq!(memory.vsram()[0], 0x07ff),
                _ => unreachable!(),
            }
            continue;
        }
        assert_eq!(memory, saved_memory);
        assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
    }
    let mut fifo = VdpWriteFifo::default();
    fifo.push(entry(0, 1, 0x1234)).unwrap();
    fifo.service_write_slot(&mut memory).unwrap();
    let saved = fifo.clone();
    for (write, error) in [
        (entry(0, 0x80, 0), VdpWriteError::InvalidCode(0x80)),
        (entry(0x20000, 1, 0), VdpWriteError::InvalidAddress(0x20000)),
    ] {
        assert_eq!(fifo.push(write), Err(error));
        assert_eq!(fifo, saved);
    }
    fifo.push(entry(2, 3, 0xffff)).unwrap();
    fifo.service_write_slot(&mut memory).unwrap();
    assert_eq!(
        fifo.service_write_slot(&mut memory),
        Ok(Some(entry(2, 3, 0xffff)))
    );
    assert_eq!(memory.cram()[1], 0x0eee);
}
