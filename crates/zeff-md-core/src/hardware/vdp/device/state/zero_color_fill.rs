use super::*;

#[path = "zero_fill_port.rs"]
mod zero_fill_port;

pub(super) fn retirement_contract() {
    zero_fill_port::receipt_contract();
    for region in [Region::NtscUs, Region::Pal] {
        for increment in [0, 1, 2] {
            let base = fixture(region, increment);
            for code in [CRAM_WRITE_CODE, VSRAM_WRITE_CODE] {
                for alias in [0, COMMAND_CD4] {
                    for parity in [0, 1] {
                        for head in 0..FIFO_CAPACITY {
                            for live in [0, 0x321] {
                                let saved = if code == CRAM_WRITE_CODE { 0x7e } else { 0x4c };
                                let entry = write(saved | parity, code | alias, 0);
                                let mut vdp = base.clone();
                                vdp.state.fifo = history(head);
                                vdp.state.fifo.push(entry).unwrap();
                                prepare_memory(&mut vdp, entry, live);
                                command(&mut vdp, live, 0x21, region);
                                continue_fill(vdp, entry, live, increment, region);
                            }
                        }
                    }
                }
            }
            mixed_queue(&base, increment, region);
        }
        rejected_histories(region);
        empty_waiting(region);
        pending_input_guard(region);
    }
    byte_history_guard();
}

fn fixture(region: Region, increment: u8) -> Vdp {
    let mut vdp = transfer_fixture(region, 0, false);
    for (index, value) in [
        (AUTO_INCREMENT_REGISTER, increment),
        (DMA_LENGTH_LOW_REGISTER, 0xff),
        (DMA_LENGTH_HIGH_REGISTER, 0xff),
        (DMA_SOURCE_LOW_REGISTER, 0xff),
        (DMA_SOURCE_HIGH_REGISTER, 0xff),
        (DMA_SOURCE_REGISTER, DMA_FILL_MODE),
    ] {
        register(&mut vdp, index, value, region);
    }
    vdp
}

fn write(address: u32, code: u8, data: u16) -> VdpWrite {
    VdpWrite {
        command: ControlCommand { address, code },
        data,
    }
}

fn history(head: usize) -> VdpWriteFifo {
    let mut fifo = VdpWriteFifo::default();
    let mut scratch = VdpMemory::default();
    for index in 0..FIFO_CAPACITY + head {
        let code = [CRAM_WRITE_CODE, VSRAM_WRITE_CODE, VRAM_WRITE_CODE][index % 3];
        fifo.push(write((index * 2) as u32, code, 0)).unwrap();
        fifo.complete_write(&mut scratch).unwrap();
    }
    assert!(fifo.is_empty());
    fifo
}

fn prepare_memory(vdp: &mut Vdp, entry: VdpWrite, live: u32) {
    vdp.memory.write_word(entry.command, 0xffff).unwrap();
    let index = usize::from((entry.command.address as u16 & COLOR_WORD_ADDRESS_MASK) >> 1);
    if entry.command.code & !COMMAND_CD4 == CRAM_WRITE_CODE {
        assert_eq!(vdp.memory.cram_word(index), Some(CRAM_DATA_MASK));
    } else {
        assert_eq!(vdp.memory.vsram()[index], VSRAM_DATA_MASK);
    }
    for address in [entry.command.address & !1, live & !1] {
        for offset in 0..8 {
            vdp.memory.write_dma_byte((address + offset) as u16, 0xa5);
        }
    }
}

fn fifo_slots(fifo: &VdpWriteFifo) -> Vec<u8> {
    let mut writer = StateWriter::new();
    fifo.write_state(&mut writer);
    let mut bytes = writer.into_bytes();
    bytes.truncate(bytes.len() - 4);
    bytes
}

fn counters(vdp: &Vdp, length: u16, source: u16) {
    let registers = VdpDmaRegisters::from_registers(vdp.registers()).unwrap();
    assert_eq!(registers.length(), length);
    assert_eq!(registers.source(), source);
    assert_eq!(registers.source_high(), DMA_FILL_MODE);
}

fn advance(vdp: &mut Vdp, copies: &mut [Vdp], region: Region) {
    let at = next_slot(vdp, region);
    vdp.advance_to(at, region).unwrap();
    for copy in copies {
        copy.advance_to(at, region).unwrap();
        assert_eq!(copy, vdp);
    }
}

fn continue_fill(mut vdp: Vdp, entry: VdpWrite, live: u32, increment: u8, region: Region) {
    assert_eq!(vdp.fifo().len(), 1);
    assert_eq!(vdp.state.dma, Some(dma::Phase::FillWaiting));
    let slots = fifo_slots(vdp.fifo());
    let mut expected = vdp.memory.clone();
    let mut copies = vec![vdp.clone(), restored(&vdp, region)];
    counters(&vdp, 0xffff, 0xffff);
    advance(&mut vdp, &mut copies, region);
    expected.write_word(entry.command, 0).unwrap();
    assert_eq!(vdp.memory, expected);
    assert_eq!(
        vdp.command(),
        ControlCommand {
            address: live,
            code: 0x21
        }
    );
    assert!(vdp.fifo().is_empty());
    assert_eq!(fifo_slots(vdp.fifo()), slots);
    assert_eq!(vdp.state.dma, Some(dma::Phase::Fill(0)));
    counters(&vdp, 0xffff, 0xffff);
    copies.extend([vdp.clone(), restored(&vdp, region)]);
    let before = vdp.clone();
    assert_eq!(
        vdp.write_data(0x1234, region),
        Err(VdpPortError::Dma(VdpDmaError::Busy))
    );
    assert_eq!(vdp, before);
    for step in 1..=3u16 {
        let previous = (live + u32::from(step - 1) * u32::from(increment)) & COMMAND_ADDRESS_MASK;
        expected.write_dma_byte(previous as u16 ^ 1, 0);
        advance(&mut vdp, &mut copies, region);
        assert_eq!(vdp.memory, expected);
        assert_eq!(
            vdp.command().address,
            (live + u32::from(step) * u32::from(increment)) & COMMAND_ADDRESS_MASK
        );
        assert_eq!(vdp.command().code, 0x21);
        assert_eq!(vdp.state.dma, Some(dma::Phase::Fill(0)));
        assert_eq!(fifo_slots(vdp.fifo()), slots);
        counters(&vdp, 0xffff - step, 0xffffu16.wrapping_add(step));
        copies.extend([vdp.clone(), restored(&vdp, region)]);
    }
}

fn mixed_queue(base: &Vdp, increment: u8, region: Region) {
    for previous_code in [VRAM_WRITE_CODE, CRAM_WRITE_CODE, VSRAM_WRITE_CODE] {
        let mut vdp = base.clone();
        vdp.state.fifo = history(3);
        let previous = write(0x31, previous_code, 0);
        let entry = write(0x2b, CRAM_WRITE_CODE, 0);
        vdp.state.fifo.push(previous).unwrap();
        vdp.state.fifo.push(entry).unwrap();
        prepare_memory(&mut vdp, entry, 0x200);
        vdp.memory.write_word(previous.command, 0xffff).unwrap();
        command(&mut vdp, 0x200, 0x21, region);
        let mut copies = [vdp.clone(), restored(&vdp, region)];
        let slots = if previous_code == VRAM_WRITE_CODE {
            2
        } else {
            1
        };
        for _ in 0..slots {
            advance(&mut vdp, &mut copies, region);
            counters(&vdp, 0xffff, 0xffff);
            assert_eq!(vdp.state.dma, Some(dma::Phase::FillWaiting));
        }
        assert_eq!(vdp.fifo().front(), Some(entry));
        if previous_code == VRAM_WRITE_CODE {
            assert_eq!(&vdp.memory.vram()[0x30..0x32], &[0, 0]);
        } else if previous_code == CRAM_WRITE_CODE {
            assert_eq!(vdp.memory.cram_word(0x18), Some(0));
        } else {
            assert_eq!(vdp.memory.vsram()[0x18], 0);
        }
        continue_fill(vdp, entry, 0x200, increment, region);
    }
}

fn raw_fifo(slots: [Option<VdpWrite>; FIFO_CAPACITY], head: usize) -> VdpWriteFifo {
    let mut writer = StateWriter::new();
    for entry in slots {
        write_optional(&mut writer, entry, |writer, entry| {
            write_command(writer, entry.command);
            writer.write_u16(entry.data);
        });
    }
    writer.write_u8(head as u8);
    writer.write_u8(1);
    writer.write_bool(false);
    writer.write_u8(0);
    let bytes = writer.into_bytes();
    let mut reader = StateReader::new(&bytes);
    let fifo = VdpWriteFifo::read_state(&mut reader).unwrap();
    assert!(reader.is_exhausted());
    fifo
}

fn rejected_histories(region: Region) {
    let base = fixture(region, 1);
    for code in [CRAM_WRITE_CODE, VSRAM_WRITE_CODE] {
        for head in 0..FIFO_CAPACITY {
            let entry = write(0x23, code, 0);
            for poison in 0..FIFO_CAPACITY {
                for data in [None, Some(1), Some(0x1111), Some(0xabab), Some(0xffff)] {
                    if poison == head && data.is_none() {
                        continue;
                    }
                    let mut slots = [Some(write(0x40, VRAM_WRITE_CODE, 0)); FIFO_CAPACITY];
                    slots[head] = Some(entry);
                    slots[poison] = data.map(|data| VdpWrite {
                        data,
                        ..slots[poison].unwrap()
                    });
                    let mut vdp = base.clone();
                    vdp.state.fifo = raw_fifo(slots, head);
                    prepare_memory(&mut vdp, entry, 0);
                    command(&mut vdp, 0, 0x21, region);
                    let before = vdp.clone();
                    let bytes = encoded(&vdp, region);
                    let at = next_slot(&vdp, region);
                    for mut candidate in [vdp, before.clone(), restored(&before, region)] {
                        assert_eq!(
                            candidate.advance_to(at, region),
                            Err(VdpScheduleError::Dma(VdpDmaError::PendingInputs))
                        );
                        assert_eq!(candidate, before);
                        assert_eq!(encoded(&candidate, region), bytes);
                    }
                }
            }
        }
    }
}

fn empty_waiting(region: Region) {
    let mut vdp = fixture(region, 1);
    vdp.state.fifo = history(2);
    command(&mut vdp, 0, 0x21, region);
    let memory = vdp.memory.clone();
    let fifo = vdp.fifo().clone();
    let mut copies = [vdp.clone(), restored(&vdp, region)];
    for _ in 0..3 {
        advance(&mut vdp, &mut copies, region);
        assert_eq!(vdp.state.dma, Some(dma::Phase::FillWaiting));
        assert_eq!(vdp.memory, memory);
        assert_eq!(vdp.fifo(), &fifo);
        assert_eq!(vdp.command().address, 0);
        counters(&vdp, 0xffff, 0xffff);
    }
}

fn byte_history_guard() {
    let mut fifo = history(0);
    fifo.push(write(1, VRAM_WRITE_CODE, 0)).unwrap();
    assert!(fifo.known_zero_history());
    fifo.take_slot().unwrap();
    assert!(!fifo.known_zero_history());
    let mut writer = StateWriter::new();
    fifo.write_state(&mut writer);
    let mut bytes = writer.into_bytes();
    let phase = bytes.len() - 2;
    bytes[phase] = 0;
    *bytes.last_mut().unwrap() = 1;
    let legacy = VdpWriteFifo::read_state(&mut StateReader::new(&bytes)).unwrap();
    assert!(!legacy.known_zero_history());
}

fn pending_input_guard(region: Region) {
    let mut vdp = fixture(region, 1);
    vdp.state.fifo = history(0);
    vdp.state
        .fifo
        .push(write(0x20, CRAM_WRITE_CODE, 0))
        .unwrap();
    command(&mut vdp, 0, 0x21, region);
    assert!(vdp.state.zero_color_fill_seed());
    let mut pending = vdp.state.clone();
    pending.control.first_word = Some(0x4000);
    assert!(!pending.zero_color_fill_seed());
    let mut pending = vdp.state.clone();
    pending
        .registers
        .advance_to(tick(pending.now().get() + 1))
        .unwrap();
    pending.registers.queue_register(19, 0).unwrap();
    assert!(!pending.zero_color_fill_seed());
    let mut pending = vdp.state.clone();
    pending.read_phase = Some(false);
    assert!(!pending.zero_color_fill_seed());
    let mut pending = vdp.state.clone();
    let mut read = ControlCommand {
        address: 0,
        code: VRAM_READ_CODE,
    };
    pending.cache.complete_word(0, &mut read, 1);
    assert!(!pending.zero_color_fill_seed());
    for phase in [
        None,
        Some(dma::Phase::Fill(0)),
        Some(dma::Phase::Copy(None)),
        Some(dma::Phase::External),
    ] {
        let mut pending = vdp.state.clone();
        pending.dma = phase;
        assert!(!pending.zero_color_fill_seed());
    }
}
