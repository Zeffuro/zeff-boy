use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, ControlWrite, VdpControl, VdpMemory, VdpRegisterPipeline, VdpWrite,
    VdpWriteError, VdpWriteFifo,
};

pub const ADDRESSES: [u32; 7] = [0, 1, 0xff00, 0xffff, 0x10000, 0x1ff00, 0x1ffff];

pub fn command(address: u32, code: u8) -> ControlCommand {
    ControlCommand { address, code }
}
pub fn entry(address: u32, code: u8, data: u16) -> VdpWrite {
    VdpWrite {
        command: command(address, code),
        data,
    }
}

pub fn upper_address_command_bits() {
    for (first, second, address, code) in [
        (0x4000, 4, 0x10000, 1),
        (0x7fff, 7, 0x1ffff, 1),
        (0xc001, 6, 0x18001, 3),
        (0x4123, 0x15, 0x14123, 5),
        (0xffff, 0xffff, 0x1ffff, 0x3f),
    ] {
        let mut decoder = VdpControl::default();
        decoder.write_control(first);
        assert_eq!(
            decoder.write_control(second),
            ControlWrite::Command(command(address, code))
        );
        assert_eq!(decoder.pending_word(), None);
    }
    for high in 0..8u16 {
        for noise in [0, 8, 0xff08] {
            let mut decoder = VdpControl::default();
            decoder.write_control(0x4567);
            assert_eq!(
                decoder.write_control(high | noise),
                ControlWrite::Command(command((u32::from(high) << 14) | 0x567, 1))
            );
        }
    }
}

pub fn address_increment_partition() {
    for address in ADDRESSES {
        for increment in 0u8..=255 {
            for code in [1, 3, 5] {
                let mut fifo = VdpWriteFifo::default();
                let mut current = command(address, code);
                let next = address + u32::from(increment);
                let expected = entry(address, code, 0xabcd);
                assert_eq!(
                    fifo.push_data(&mut current, Some(increment), 0xabcd),
                    Ok(expected)
                );
                assert_eq!(current, command(next & 0x1ffff, code));
                assert_eq!(fifo.front(), Some(expected));
                assert_eq!(fifo.len(), 1);
            }
        }
    }
}

pub fn rejected_acceptance_preserves_state() {
    let mut fifo = VdpWriteFifo::default();
    let mut current = command(1, 1);
    let saved = fifo.clone();
    assert_eq!(
        fifo.push_data(&mut current, None, 0xffff),
        Err(VdpWriteError::UnknownIncrement)
    );
    assert_eq!(current, command(1, 1));
    assert_eq!(fifo, saved);
    for code in 0u8..=255 {
        if code < 0x20 && !matches!(code & 0xf, 0 | 4 | 8 | 0xc) {
            continue;
        }
        let mut current = command(1, code);
        let expected = if code < 64 {
            VdpWriteError::UnsupportedCommand(current)
        } else {
            VdpWriteError::InvalidCode(code)
        };
        assert_eq!(fifo.push_data(&mut current, Some(2), 0xffff), Err(expected));
        assert_eq!(current, command(1, code));
        assert_eq!(fifo, saved);
    }
    for address in [0x20000, 0x20001, u32::MAX] {
        let mut current = command(address, 1);
        assert_eq!(
            fifo.push_data(&mut current, Some(255), 0xffff),
            Err(VdpWriteError::InvalidAddress(address))
        );
        assert_eq!(
            fifo.push(entry(address, 1, 0xffff)),
            Err(VdpWriteError::InvalidAddress(address))
        );
        assert_eq!(current, command(address, 1));
        assert_eq!(fifo, saved);
    }
    let mut memory = VdpMemory::default();
    for data in 0..4 {
        fifo.push_data(&mut current, Some(2), data).unwrap();
    }
    assert_eq!(current, command(9, 1));
    let saved = fifo.clone();
    assert_eq!(
        fifo.push_data(&mut current, Some(255), 0x1234),
        Err(VdpWriteError::Full)
    );
    assert_eq!(current, command(9, 1));
    assert_eq!(fifo, saved);
    assert_eq!(fifo.complete_write(&mut memory), Ok(Some(entry(1, 1, 0))));
    assert_eq!(
        fifo.push_data(&mut current, Some(255), 0x1234),
        Ok(entry(9, 1, 0x1234))
    );
    assert_eq!(current, command(264, 1));
    for (address, data) in [(3, 1), (5, 2), (7, 3), (9, 0x1234)] {
        assert_eq!(
            fifo.complete_write(&mut memory),
            Ok(Some(entry(address, 1, data)))
        );
    }
    assert!(fifo.is_empty());
    assert_eq!(&memory.vram()[8..10], &[0x34, 0x12]);
}

pub fn published_increment_controls_fifo_capture() {
    let mut registers = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    registers.advance_to(MasterTicks::new(1)).unwrap();
    registers.queue_register(15, 2).unwrap();
    let mut fifo = VdpWriteFifo::default();
    let mut current = command(0, 1);
    assert_eq!(
        fifo.push_data(&mut current, registers.auto_increment(), 0x1234),
        Err(VdpWriteError::UnknownIncrement)
    );
    registers.advance_to(MasterTicks::new(20)).unwrap();
    fifo.push_data(&mut current, registers.auto_increment(), 0x1234)
        .unwrap();
    registers.advance_to(MasterTicks::new(21)).unwrap();
    registers.queue_register(15, 255).unwrap();
    fifo.push_data(&mut current, registers.auto_increment(), 0x5678)
        .unwrap();
    assert_eq!(current.address, 4);
    registers.advance_to(MasterTicks::new(39)).unwrap();
    fifo.push_data(&mut current, registers.auto_increment(), 0xabcd)
        .unwrap();
    assert_eq!(current.address, 6);
    registers.advance_to(MasterTicks::new(40)).unwrap();
    fifo.push_data(&mut current, registers.auto_increment(), 0xef01)
        .unwrap();
    assert_eq!(current.address, 261);
    let mut memory = VdpMemory::default();
    assert_eq!(&memory.vram()[..8], &[0; 8]);
    for (address, data) in [(0, 0x1234), (2, 0x5678), (4, 0xabcd), (6, 0xef01)] {
        assert_eq!(
            fifo.complete_write(&mut memory),
            Ok(Some(entry(address, 1, data)))
        );
        assert_eq!(current.address, 261);
    }
    assert_eq!(
        &memory.vram()[..8],
        &[0x12, 0x34, 0x56, 0x78, 0xab, 0xcd, 0xef, 1]
    );
}

pub fn wide_entries_and_memory_guards() {
    let mut fifo = VdpWriteFifo::default();
    let mut current = command(0xffff, 1);
    fifo.push_data(&mut current, Some(1), 0xabcd).unwrap();
    assert_eq!(current.address, 0x10000);
    fifo.push_data(&mut current, Some(1), 0x1234).unwrap();
    assert_eq!(current.address, 0x10001);
    let mut memory = VdpMemory::default();
    assert_eq!(
        fifo.complete_write(&mut memory),
        Ok(Some(entry(0xffff, 1, 0xabcd)))
    );
    assert_eq!(&memory.vram()[0xfffe..], &[0xcd, 0xab]);
    assert_eq!(
        fifo.complete_write(&mut memory),
        Ok(Some(entry(0x10000, 1, 0x1234)))
    );
    assert_eq!(&memory.vram()[..2], &[0x12, 0x34]);
    assert!(fifo.is_empty());
    let saved_memory = memory.clone();
    for address in [0x10000, 0x1ffff, 0x20000, u32::MAX] {
        for code in [1, 3, 5] {
            if address <= 0x1ffff {
                let mut aliased = memory.clone();
                aliased.write_word(command(address, code), 0xffff).unwrap();
                match code {
                    1 => assert_eq!(
                        &aliased.vram()[(address as usize & 0xffff) & !1..][..2],
                        &[0xff, 0xff]
                    ),
                    3 => assert_eq!(aliased.cram()[((address & 0x7f) >> 1) as usize], 0x0eee),
                    5 if address & 0x7f < 80 => {
                        assert_eq!(aliased.vsram()[((address & 0x7f) >> 1) as usize], 0x07ff)
                    }
                    5 => assert_eq!(aliased, memory),
                    _ => unreachable!(),
                }
                continue;
            }
            let command = command(address, code);
            assert_eq!(
                memory.write_word(command, 0xffff),
                Err(VdpWriteError::UnsupportedCommand(command))
            );
            assert_eq!(memory, saved_memory);
        }
    }
}
