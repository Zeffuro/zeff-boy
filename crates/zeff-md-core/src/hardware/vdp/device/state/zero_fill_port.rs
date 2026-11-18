use super::*;

pub(super) fn receipt_contract() {
    for region in [Region::NtscUs, Region::Pal] {
        for size in [0, H40_CLOCK_MASK] {
            for display in [false, true] {
                for increment in [0, 1, 2] {
                    for address in [0x200, 0x201] {
                        queued_continuation(region, size, display, increment, address);
                    }
                }
                interval_guard(active_fixture(region, size, display, 1), region);
            }
        }
        changed_inputs(region);
        queued_guards(region);
        copy_mode_guard(region);
        bus_routes(region);
    }
}

fn active_fixture(region: Region, size: u8, display: bool, increment: u8) -> Vdp {
    let mut vdp = transfer_fixture(region, size, display);
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
    vdp.state.fifo = history(1);
    command(&mut vdp, 0x200, 0x21, region);
    vdp.state.dma = Some(dma::Phase::Fill(0));
    vdp
}

fn gap(vdp: &mut Vdp, width: u64, region: Region) -> MasterTicks {
    for _ in 0..H32_LINE_TICKS {
        let end = tick(vdp.now().get() + width);
        if vdp.prepare_zero_fill_write(end, region).is_some() {
            return end;
        }
        vdp.advance_to(tick(vdp.now().get() + 1), region).unwrap();
    }
    panic!("no qualified zero-fill port interval");
}

fn queued_continuation(region: Region, size: u8, display: bool, increment: u8, address: u32) {
    let mut vdp = active_fixture(region, size, display, increment);
    let end = gap(&mut vdp, 3, region);
    vdp.state.command.address = address;
    let saved = vdp.command();
    let live = saved.next_address(increment);
    for target in [saved.address, live] {
        for offset in 0..4 {
            vdp.memory
                .write_dma_byte((target as u16 & !1) + offset, 0xa5);
        }
    }
    let registers = VdpDmaRegisters::from_registers(vdp.registers()).unwrap();
    let mut expected = vdp.memory.clone();
    let mut copies = vec![vdp.clone(), restored(&vdp, region)];
    let receipt = vdp.prepare_zero_fill_write(end, region).unwrap();
    let before = vdp.clone();
    assert!(vdp.validate_zero_fill_write(&receipt, region).is_err());
    for word in [0, 0x1234] {
        assert_eq!(
            vdp.write_data(word, region),
            Err(VdpPortError::Dma(VdpDmaError::Busy))
        );
        assert_eq!(vdp, before);
    }
    vdp.advance_to(end, region).unwrap();
    vdp.validate_zero_fill_write(&receipt, region).unwrap();
    vdp.commit_zero_fill_write(0, receipt, region).unwrap();
    for copy in &mut copies {
        let receipt = copy.prepare_zero_fill_write(end, region).unwrap();
        copy.advance_to(end, region).unwrap();
        copy.validate_zero_fill_write(&receipt, region).unwrap();
        copy.commit_zero_fill_write(0, receipt, region).unwrap();
        assert_eq!(copy, &vdp);
    }
    assert_eq!(vdp.memory, expected);
    assert_eq!(
        vdp.command(),
        ControlCommand {
            address: live,
            code: 0x21
        }
    );
    assert_eq!(
        vdp.fifo().front(),
        Some(write(saved.address, VRAM_WRITE_CODE, 0))
    );
    assert_eq!(vdp.fifo().len(), 1);
    counters(&vdp, registers.length(), registers.source());
    copies.extend([vdp.clone(), restored(&vdp, region)]);
    for byte in [saved.address as u16 ^ 1, saved.address as u16] {
        expected.write_dma_byte(byte, 0);
        advance(&mut vdp, &mut copies, region);
        assert_eq!(vdp.memory, expected);
        assert_eq!(vdp.command().address, live);
        assert_eq!(vdp.state.dma, Some(dma::Phase::Fill(0)));
        counters(&vdp, registers.length(), registers.source());
        copies.extend([vdp.clone(), restored(&vdp, region)]);
    }
    assert!(vdp.fifo().is_empty());
    expected.write_dma_byte(live as u16 ^ 1, 0);
    advance(&mut vdp, &mut copies, region);
    assert_eq!(vdp.memory, expected);
    assert_eq!(
        vdp.command().address,
        (live + u32::from(increment)) & COMMAND_ADDRESS_MASK
    );
    counters(
        &vdp,
        registers.length().wrapping_sub(1),
        registers.source().wrapping_add(1),
    );
    restored(&vdp, region);
}

fn interval_guard(mut vdp: Vdp, region: Region) {
    let end = gap(&mut vdp, 3, region);
    let before = vdp.clone();
    assert!(vdp.prepare_zero_fill_write(vdp.now(), region).is_none());
    assert!(
        vdp.prepare_zero_fill_write(tick(vdp.now().get() - 1), region)
            .is_none()
    );
    let first = next_slot(&vdp, region);
    assert!(vdp.prepare_zero_fill_write(first, region).is_none());
    assert!(
        vdp.prepare_zero_fill_write(tick(first.get() + 1), region)
            .is_none()
    );
    assert_eq!(vdp, before);
    let mut at_slot = vdp.clone();
    at_slot.advance_to(first, region).unwrap();
    assert!(
        at_slot
            .prepare_zero_fill_write(tick(first.get() + 1), region)
            .is_none()
    );
    for target in [tick(end.get() - 1), tick(end.get() + 1)] {
        let mut wrong_end = vdp.clone();
        let receipt = wrong_end.prepare_zero_fill_write(end, region).unwrap();
        wrong_end.advance_to(target, region).unwrap();
        let before = wrong_end.clone();
        assert!(
            wrong_end
                .validate_zero_fill_write(&receipt, region)
                .is_err()
        );
        assert!(
            wrong_end
                .commit_zero_fill_write(0, receipt, region)
                .is_err()
        );
        assert_eq!(wrong_end, before);
    }
    for word in [1, 0x1111, 0xabab, 0xffff] {
        let mut rejected = vdp.clone();
        let receipt = rejected.prepare_zero_fill_write(end, region).unwrap();
        rejected.advance_to(end, region).unwrap();
        let before = restored(&rejected, region);
        assert!(
            rejected
                .commit_zero_fill_write(word, receipt, region)
                .is_err()
        );
        assert_eq!(rejected, before);
    }
    let receipt = vdp.prepare_zero_fill_write(end, region).unwrap();
    vdp.advance_to(end, region).unwrap();
    let other = if region == Region::Pal {
        Region::NtscUs
    } else {
        Region::Pal
    };
    let before = vdp.clone();
    assert!(vdp.validate_zero_fill_write(&receipt, other).is_err());
    assert!(vdp.commit_zero_fill_write(0, receipt, other).is_err());
    assert_eq!(vdp, before);
}

fn alter(vdp: &mut Vdp, case: usize) {
    match case {
        0 => vdp.state.command.address ^= 2,
        1 => vdp.state.command.code = 0x31,
        2 => {
            vdp.state.registers.queue_control_register(19, 7).unwrap();
            let at = vdp.state.registers.pending().unwrap().publish_at;
            vdp.state.registers.advance_to(at).unwrap();
        }
        3 => {
            vdp.state.registers.queue_control_register(19, 7).unwrap();
        }
        4 => vdp.state.control.first_word = Some(0x4000),
        5 => vdp.state.read_phase = Some(false),
        6 => {
            let mut command = ControlCommand {
                address: 0,
                code: VRAM_READ_CODE,
            };
            vdp.state.cache.complete_word(0, &mut command, 1);
        }
        7 => vdp
            .state
            .fifo
            .push(write(0x200, VRAM_WRITE_CODE, 0))
            .unwrap(),
        8 => vdp.state.fifo = VdpWriteFifo::default(),
        9 => {
            let mut scratch = VdpMemory::default();
            vdp.state
                .fifo
                .push(write(0x10, CRAM_WRITE_CODE, 0x1111))
                .unwrap();
            vdp.state.fifo.complete_write(&mut scratch).unwrap();
        }
        10 => vdp.state.dma = Some(dma::Phase::Fill(1)),
        11 => vdp.state.dma = Some(dma::Phase::FillWaiting),
        12 => vdp.state.dma = Some(dma::Phase::Copy(None)),
        13 => vdp.state.dma = Some(dma::Phase::External),
        14 => vdp.state.dma = None,
        15 => {
            vdp.state
                .registers
                .queue_control_register(MODE_REGISTER, MODE5_ENABLE | DMA_ENABLE | VRAM_128K_ENABLE)
                .unwrap();
            let at = vdp.state.registers.pending().unwrap().publish_at;
            vdp.state.registers.advance_to(at).unwrap();
        }
        16 => {
            let history = crate::hardware::vdp::timing::blank_mode::BlankHistory::new(vdp.now());
            vdp.state.interrupts.set_blank_history(Some(history));
        }
        17 => vdp.state.fifo = history(2),
        _ => unreachable!(),
    }
}

fn changed_inputs(region: Region) {
    let mut base = active_fixture(region, 0, false, 1);
    let end = gap(&mut base, 3, region);
    for case in 0..18 {
        let mut before_prepare = base.clone();
        alter(&mut before_prepare, case);
        let snapshot = before_prepare.clone();
        if !matches!(case, 0 | 2 | 17) {
            assert!(
                before_prepare
                    .prepare_zero_fill_write(end, region)
                    .is_none()
            );
        }
        assert_eq!(before_prepare, snapshot);
        let mut before_commit = base.clone();
        let receipt = before_commit.prepare_zero_fill_write(end, region).unwrap();
        before_commit.advance_to(end, region).unwrap();
        alter(&mut before_commit, case);
        let snapshot = before_commit.clone();
        assert!(
            before_commit
                .validate_zero_fill_write(&receipt, region)
                .is_err()
        );
        assert!(
            before_commit
                .commit_zero_fill_write(0, receipt, region)
                .is_err()
        );
        assert_eq!(before_commit, snapshot);
    }
}

fn queued_guards(region: Region) {
    let mut queued = active_fixture(region, 0, false, 1);
    let end = gap(&mut queued, 3, region);
    queued.state.command.address = 0x201;
    let receipt = queued.prepare_zero_fill_write(end, region).unwrap();
    queued.advance_to(end, region).unwrap();
    queued.commit_zero_fill_write(0, receipt, region).unwrap();
    for case in 0..8 {
        let mut invalid = queued.clone();
        match case {
            0 => invalid.state.dma = Some(dma::Phase::Fill(1)),
            1 => invalid.state.command.code = 0x31,
            2 => invalid
                .state
                .fifo
                .push(write(0x204, VRAM_WRITE_CODE, 0))
                .unwrap(),
            3 => {
                invalid.state.fifo = VdpWriteFifo::default();
                invalid
                    .state
                    .fifo
                    .push(write(0x201, VRAM_WRITE_CODE, 0))
                    .unwrap();
            }
            4 | 5 => {
                invalid.state.fifo = history(1);
                invalid
                    .state
                    .fifo
                    .push(write(
                        0x201,
                        if case == 4 { VRAM_WRITE_CODE } else { 0x11 },
                        if case == 4 { 0x1111 } else { 0 },
                    ))
                    .unwrap();
            }
            6 => invalid.state.dma = Some(dma::Phase::Copy(None)),
            7 => {
                let mut writer = StateWriter::new();
                invalid.state.fifo.write_state(&mut writer);
                let mut bytes = writer.into_bytes();
                let head = bytes[bytes.len() - 4];
                *bytes.last_mut().unwrap() = 1 << head;
                invalid.state.fifo =
                    VdpWriteFifo::read_state(&mut StateReader::new(&bytes)).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(invalid.validate_state(region).is_err());
    }
}

fn copy_mode_guard(region: Region) {
    let mut vdp = active_fixture(region, 0, true, 1);
    let end = gap(&mut vdp, H32_INTERNAL_CLOCK_TICKS + 1, region);
    let receipt = vdp.prepare_zero_fill_write(end, region).unwrap();
    let mut changed = vdp.clone();
    changed
        .state
        .registers
        .queue_control_register(DMA_SOURCE_REGISTER, DMA_COPY_MODE)
        .unwrap();
    changed.advance_to(end, region).unwrap();
    assert_eq!(changed.registers().now(), end);
    assert!(changed.registers().pending().is_none());
    assert_eq!(
        VdpDmaRegisters::from_registers(changed.registers())
            .unwrap()
            .source_high(),
        DMA_COPY_MODE
    );
    let before = changed.clone();
    assert!(
        changed
            .prepare_zero_fill_write(tick(end.get() + 1), region)
            .is_none()
    );
    assert!(changed.validate_zero_fill_write(&receipt, region).is_err());
    assert!(changed.commit_zero_fill_write(0, receipt, region).is_err());
    assert_eq!(changed, before);
    let receipt = vdp.prepare_zero_fill_write(end, region).unwrap();
    vdp.advance_to(end, region).unwrap();
    vdp.commit_zero_fill_write(0, receipt, region).unwrap();
    vdp.state.registers = *changed.registers();
    assert_eq!(vdp.registers().now(), vdp.now());
    assert!(vdp.validate_state(region).is_err());
}

fn aligned_gap(vdp: &mut Vdp, overlap: bool, region: Region) {
    for _ in 0..H32_LINE_TICKS {
        if vdp.now().get().is_multiple_of(7) {
            let end = tick(vdp.now().get() + 28);
            let clear = vdp.prepare_zero_fill_write(end, region).is_some();
            if clear != overlap && (!overlap || next_slot(vdp, region) <= end) {
                return;
            }
        }
        vdp.advance_to(tick(vdp.now().get() + 1), region).unwrap();
    }
    panic!("no aligned zero-fill bus interval");
}

fn bus_routes(region: Region) {
    use crate::cartridge::Cartridge;
    use crate::hardware::bus::{
        Bus,
        scheduled::{CompactBus, ScheduledBus},
    };
    use crate::hardware::sound::schedule::SoundSchedule;
    use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
    use zeff_m68k::timing::{ClockedBus, CpuClocks};
    let ordinary = BusAccess {
        logical_address: 0xc00000,
        bus_address: 0xc00000,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes: ByteLanes::Both,
    };
    for case in 0..8 {
        let mut vdp = active_fixture(region, 0, true, 1);
        aligned_gap(&mut vdp, case == 7, region);
        let begin = vdp.now();
        assert!(begin.get().is_multiple_of(7));
        let mut expected = vdp.clone();
        expected.advance_to(tick(begin.get() + 28), region).unwrap();
        let mut image = vec![0; 0x200];
        image[..8].copy_from_slice(&[0, 0xff, 0x10, 0, 0, 0, 1, 0]);
        let mut direct = Bus::new(Cartridge::new(image).unwrap(), region);
        direct.advance_machine(begin.get()).unwrap();
        direct.vdp = vdp;
        let start = CpuClocks(begin.get() / 7);
        direct.cpu_clock = start;
        let mut scheduled = direct.clone();
        let mut compact = direct.clone();
        let mut access = ordinary;
        let word = if case == 1 { 0x1111 } else { 0 };
        match case {
            2 => access.lanes = ByteLanes::Upper,
            3 => {
                access.lanes = ByteLanes::Lower;
                access.logical_address |= 1;
            }
            4 => access.logical_address = 0xc00002,
            5 => access.function_code = FunctionCode::SupervisorProgram,
            6 => access.kind = AccessKind::Prefetch,
            _ => {}
        }
        let direct_result = direct.write_cycle(access, word, start);
        let mut cpu = zeff_z80::Cpu::new();
        let mut schedule = SoundSchedule::default();
        let scheduled_result = ScheduledBus::new(&mut scheduled, &mut cpu, &mut schedule)
            .write_cycle(access, word, start);
        let mut cpu = zeff_z80::Cpu::new();
        let mut schedule = SoundSchedule::default();
        let compact_result = CompactBus(ScheduledBus::new(&mut compact, &mut cpu, &mut schedule))
            .write_cycle(access, word, start);
        assert_eq!(direct_result.result.is_ok(), case == 0);
        assert_eq!(scheduled_result.result.is_ok(), case == 0);
        assert_eq!(compact_result.result.is_ok(), case == 0);
        assert_eq!(direct.vdp, scheduled.vdp);
        assert_eq!(direct.vdp, compact.vdp);
        assert_eq!(direct.vdp.now(), expected.now());
        if case == 0 {
            assert_eq!(
                direct.vdp.command().address,
                expected.command().next_address(1)
            );
            assert_eq!(
                direct.vdp.fifo().front(),
                Some(write(expected.command().address, VRAM_WRITE_CODE, 0))
            );
            assert_eq!(direct.vdp.memory, expected.memory);
            assert_eq!(direct.vdp.state.dma, expected.state.dma);
            let registers = VdpDmaRegisters::from_registers(expected.registers()).unwrap();
            counters(&direct.vdp, registers.length(), registers.source());
        } else {
            assert_eq!(direct.vdp, expected);
        }
    }
}
