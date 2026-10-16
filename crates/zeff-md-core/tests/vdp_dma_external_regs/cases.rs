use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, VdpDmaError, VdpDmaRegisters, VdpMemory, VdpRegisterPipeline, VdpWrite,
    VdpWriteFifo,
};

pub type Step<'a> = dyn FnMut(
        &mut VdpDmaRegisters,
        &mut ControlCommand,
        u8,
        u16,
    ) -> Result<Option<VdpWrite>, VdpDmaError>
    + 'a;

fn command(address: u32, code: u8) -> ControlCommand {
    ControlCommand { address, code }
}

fn publish(registers: &mut VdpRegisterPipeline, index: u8, value: u8) {
    registers
        .advance_to(MasterTicks::new(registers.now().get() + 1))
        .unwrap();
    let write = registers.queue_register(index, value).unwrap();
    registers.advance_to(write.publish_at).unwrap();
}

fn registers(length: u16, source: u16, high: u8) -> VdpRegisterPipeline {
    let mut registers = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    let [length_low, length_high] = length.to_le_bytes();
    let [source_low, source_high] = source.to_le_bytes();
    for (index, value) in [length_low, length_high, source_low, source_high, high]
        .into_iter()
        .enumerate()
    {
        publish(&mut registers, 19 + index as u8, value);
    }
    registers
}

fn image(length: u16, source: u16, high: u8) -> VdpDmaRegisters {
    VdpDmaRegisters::from_registers(&registers(length, source, high)).unwrap()
}

pub fn external_source_images_and_rewrites() {
    for (high, source, expected) in [
        (0x00, 0x0000, 0x00_0000),
        (0x00, 0x1234, 0x00_2468),
        (0x02, 0xffff, 0x05_fffe),
        (0x40, 0xabcd, 0x81_579a),
        (0x7f, 0xffff, 0xff_fffe),
    ] {
        let mut registers = registers(2, source, high);
        let dma = VdpDmaRegisters::from_registers(&registers).unwrap();
        assert_eq!(dma.external_source_address(), Ok(expected));
        publish(&mut registers, 21, 0x37);
        let dma = VdpDmaRegisters::from_registers(&registers).unwrap();
        assert_eq!(dma.source(), (source & 0xff00) | 0x37);
        assert_eq!(dma.source_high(), high);
        publish(&mut registers, 22, 0x56);
        let dma = VdpDmaRegisters::from_registers(&registers).unwrap();
        assert_eq!(dma.source(), 0x5637);
        assert_eq!(dma.source_high(), high);
        assert_eq!(
            dma.external_source_address(),
            Ok((u32::from(high) << 17) | 0xac6e)
        );
    }
    for high in [0x80, 0xbf, 0xc0, 0xff] {
        assert_eq!(
            image(1, 0, high).external_source_address(),
            Err(VdpDmaError::WrongOperation(high))
        );
    }
}

pub fn external_counts_and_page_wrap() {
    external_counts_and_page_wrap_with(&mut VdpDmaRegisters::external_word);
}

pub fn external_counts_and_page_wrap_with(step: &mut Step<'_>) {
    for high in [0x00, 0x02, 0x40, 0x7f] {
        for (length, count) in [(1, 1u32), (2, 2), (0, 65536)] {
            for increment in [0, 1, 2, 255] {
                let mut dma = image(length, 0xffff, high);
                let mut current = command(0x1ffff, 0x31);
                for index in 0..count {
                    let address = (0x1ffff + index * u32::from(increment)) & 0x1ffff;
                    assert_eq!(
                        dma.external_source_address(),
                        Ok((u32::from(high) << 17)
                            | (u32::from(0xffffu16.wrapping_add(index as u16)) << 1))
                    );
                    assert_eq!(
                        step(&mut dma, &mut current, increment, index as u16),
                        Ok(Some(VdpWrite {
                            command: command(address, 0x11),
                            data: index as u16,
                        }))
                    );
                    assert_eq!(dma.length(), length.wrapping_sub((index + 1) as u16));
                    assert_eq!(dma.source(), 0xffffu16.wrapping_add((index + 1) as u16));
                    assert_eq!(dma.source_high(), high);
                    assert_eq!(current.code, if index + 1 == count { 0x11 } else { 0x31 });
                }
                assert_eq!(
                    current.address,
                    (0x1ffff + count * u32::from(increment)) & 0x1ffff
                );
                let saved = (dma, current);
                assert_eq!(step(&mut dma, &mut current, increment, 0xbeef), Ok(None));
                assert_eq!((dma, current), saved);
            }
        }
    }
}

pub fn external_destinations_and_retirement() {
    external_destinations_and_retirement_with(&mut VdpDmaRegisters::external_word);
}

pub fn external_destinations_and_retirement_with(step: &mut Step<'_>) {
    for cd4 in [0, 0x10] {
        for (address, expected) in [(0, [0x12, 0x34]), (1, [0x34, 0x12])] {
            let mut dma = image(1, 0, 0);
            let mut current = command(address, 0x21 | cd4);
            let mut memory = VdpMemory::default();
            let mut fifo = VdpWriteFifo::default();
            let write = step(&mut dma, &mut current, 2, 0x1234).unwrap().unwrap();
            assert_eq!(dma.length(), 0);
            assert_eq!(&memory.vram()[..2], &[0, 0]);
            fifo.push(write).unwrap();
            assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
            assert_eq!(&memory.vram()[..2], &[0, expected[1]]);
            assert_eq!(dma.source(), 1);
            assert_eq!(fifo.len(), 1);
            assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(write)));
            assert_eq!(&memory.vram()[..2], &expected);
        }
        for code in [3, 5] {
            let mut dma = image(3, 0xffff, 0x7f);
            let mut current = command(0x7e, 0x20 | cd4 | code);
            let mut memory = VdpMemory::default();
            let mut fifo = VdpWriteFifo::default();
            for data in [0xffff, 0x1234, 0x5678] {
                let write = step(&mut dma, &mut current, 2, data).unwrap().unwrap();
                fifo.push(write).unwrap();
                assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(write)));
            }
            assert_eq!(current.address, 0x84);
            assert_eq!(dma.source(), 2);
            assert_eq!(dma.source_high(), 0x7f);
            if code == 3 {
                assert_eq!(memory.cram()[63], 0x0eee);
                assert_eq!(&memory.cram()[..2], &[0x0224, 0x0668]);
            } else {
                assert_eq!(&memory.vsram()[..2], &[0x0234, 0x0678]);
                assert!(memory.vsram()[2..].iter().all(|word| *word == 0));
            }
        }
        let mut dma = image(26, 0, 0);
        let mut current = command(0x4e, 0x25 | cd4);
        let mut memory = VdpMemory::default();
        let mut fifo = VdpWriteFifo::default();
        for index in 0..26 {
            let write = step(&mut dma, &mut current, 2, 0x1800 | index)
                .unwrap()
                .unwrap();
            fifo.push(write).unwrap();
            assert_eq!(fifo.complete_write(&mut memory), Ok(Some(write)));
        }
        assert_eq!(current.address, 0x82);
        assert_eq!(current.code, 5 | cd4);
        assert_eq!(dma.source(), 26);
        assert_eq!(memory.vsram()[0], 25);
        assert!(memory.vsram()[1..].iter().all(|word| *word == 0));
    }
}

pub fn external_failures_are_atomic() {
    external_failures_are_atomic_with(&mut VdpDmaRegisters::external_word);
}

pub fn external_failures_are_atomic_with(step: &mut Step<'_>) {
    for high in [0x00, 0x40, 0x7f] {
        for invalid in [0, 2, 4, 0x20, 0x28, 0x30, 0x40, 0x61, 0xff] {
            let mut dma = image(0, 0xffff, high);
            let mut current = command(0, invalid);
            let saved = (dma, current);
            assert_eq!(
                step(&mut dma, &mut current, 255, 0x1234),
                Err(VdpDmaError::UnsupportedCommand(saved.1))
            );
            assert_eq!((dma, current), saved);
        }
        for address in [0x20000, u32::MAX] {
            let mut dma = image(1, 0, high);
            let mut current = command(address, 0x21);
            let saved = (dma, current);
            assert_eq!(
                step(&mut dma, &mut current, 255, 0x1234),
                Err(VdpDmaError::UnsupportedCommand(saved.1))
            );
            assert_eq!((dma, current), saved);
        }
        let mut memory = VdpMemory::default();
        let mut dma = image(2, 0xffff, high);
        for code in [0x21, 0x30] {
            let mut current = command(0, code);
            let saved = (dma, current, memory.clone());
            assert_eq!(
                dma.fill_byte(&mut current, 1, 0x1234, &mut memory),
                Err(VdpDmaError::WrongOperation(high))
            );
            assert_eq!((dma, current, &memory), (saved.0, saved.1, &saved.2));
            assert_eq!(
                dma.copy_byte(&mut current, 1, &mut memory),
                Err(VdpDmaError::WrongOperation(high))
            );
            assert_eq!((dma, current, &memory), (saved.0, saved.1, &saved.2));
        }
    }
    for high in [0x80, 0xbf, 0xc0, 0xff] {
        let mut dma = image(2, 0xffff, high);
        let mut current = command(u32::MAX, 0xff);
        let saved = (dma, current);
        assert_eq!(
            step(&mut dma, &mut current, 1, 0xabcd),
            Err(VdpDmaError::WrongOperation(high))
        );
        assert_eq!((dma, current), saved);
    }
}
