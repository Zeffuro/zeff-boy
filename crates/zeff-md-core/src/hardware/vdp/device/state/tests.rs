use super::*;
use crate::VdpDmaError;
use crate::hardware::vdp::{VdpDmaSource, VdpDmaWrite, VdpWrite};

const REGION: Region = Region::NtscUs;

#[path = "cram_provenance.rs"]
mod cram_provenance;
#[path = "display_transition_tests.rs"]
mod display_transition_tests;
#[path = "normal_interlace.rs"]
mod normal_interlace;
#[path = "pending_fill.rs"]
mod pending_fill;
#[path = "register_interruption.rs"]
mod register_interruption;

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

fn encoded(vdp: &Vdp, region: Region) -> Vec<u8> {
    let mut writer = StateWriter::new();
    vdp.write_state_in_region(&mut writer, region).unwrap();
    writer.into_bytes()
}

fn restored(vdp: &Vdp, region: Region) -> Vdp {
    let bytes = encoded(vdp, region);
    let mut reader = StateReader::new(&bytes);
    let mut target = Vdp::bootstrap_mode5();
    target.read_state(&mut reader, region).unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(&target, vdp);
    target
}

fn register(vdp: &mut Vdp, index: u8, value: u8, region: Region) {
    vdp.advance_to(tick(vdp.now().get() + 1), region).unwrap();
    let write = vdp.queue_register(index, value, region).unwrap();
    vdp.advance_to(write.publish_at, region).unwrap();
}

fn command(vdp: &mut Vdp, address: u32, code: u8, region: Region) {
    vdp.write_control(
        (address as u16 & FIRST_ADDRESS_MASK) | (u16::from(code & 3) << FIRST_CODE_SHIFT),
        region,
    )
    .unwrap();
    vdp.write_control(
        ((address >> SECOND_ADDRESS_SHIFT) as u16 & SECOND_ADDRESS_MASK)
            | (u16::from(code & !3) << SECOND_CODE_SHIFT),
        region,
    )
    .unwrap();
}

fn next_slot(vdp: &Vdp, region: Region) -> MasterTicks {
    vdp.state
        .interrupts
        .next_write_slot(vdp.now(), region, &vdp.state.registers)
        .unwrap()
}

fn transfer_fixture(region: Region, size: u8, display: bool) -> Vdp {
    let mut vdp = Vdp::bootstrap_mode5();
    register(&mut vdp, AUTO_INCREMENT_REGISTER, 2, region);
    register(&mut vdp, DISPLAY_SIZE_REGISTER, size, region);
    let mode = MODE5_ENABLE
        | DMA_ENABLE
        | VINT_ENABLE_MASK
        | if display { DISPLAY_ENABLE } else { 0 }
        | if region == Region::Pal { V30_ENABLE } else { 0 };
    register(&mut vdp, MODE_REGISTER, mode, region);
    let mut vdp =
        Vdp::from_stable_display_state(*vdp.interrupts(), *vdp.registers(), region).unwrap();
    vdp.advance_to(tick(20 * H32_LINE_TICKS + 101), region)
        .unwrap();
    vdp
}

fn queue_mode(vdp: &mut Vdp, value: u8, region: Region, control_port: bool) -> VdpRegisterWrite {
    let at = vdp.now().get() + 1;
    let at = at + u64::from(at.is_multiple_of(H32_INTERNAL_CLOCK_TICKS));
    vdp.advance_to(tick(at), region).unwrap();
    if control_port {
        let word =
            REGISTER_TAG | (u16::from(MODE_REGISTER) << REGISTER_INDEX_SHIFT) | u16::from(value);
        vdp.write_control(word, region).unwrap();
        vdp.registers().pending().unwrap()
    } else {
        vdp.queue_register(MODE_REGISTER, value, region).unwrap()
    }
}

#[test]
fn intensity_toggles_preserve_pending_transfers_and_restore() {
    for region in [Region::NtscUs, Region::Pal] {
        for size in [0, H40_CLOCK_MASK] {
            for code in [VRAM_WRITE_CODE, VRAM_READ_CODE] {
                for enabled in [true, false] {
                    let initial = size | if enabled { 0 } else { SHADOW_HIGHLIGHT_ENABLE };
                    let mut fixture = transfer_fixture(region, initial, true);
                    command(&mut fixture, 0, code, region);
                    if code == VRAM_WRITE_CODE {
                        fixture.write_data(0xcafe, region).unwrap();
                    }
                    fixture
                        .advance_to(tick(fixture.now().get() + 1), region)
                        .unwrap();
                    let before = fixture.clone();
                    let mut vdp = fixture.clone();
                    let value = initial ^ SHADOW_HIGHLIGHT_ENABLE;
                    let write = vdp
                        .queue_register(DISPLAY_SIZE_REGISTER, value, region)
                        .unwrap();
                    let mut copy = restored(&vdp, region);
                    vdp.advance_to(tick(write.publish_at.get() - 1), region)
                        .unwrap();
                    assert_eq!(vdp.memory, before.memory);
                    assert_eq!(
                        vdp.registers().registers()[usize::from(DISPLAY_SIZE_REGISTER)],
                        Some(initial)
                    );
                    vdp.advance_to(write.publish_at, region).unwrap();
                    copy.advance_to(write.publish_at, region).unwrap();
                    assert_eq!(vdp, copy);
                    assert_eq!(
                        vdp.registers().registers()[usize::from(DISPLAY_SIZE_REGISTER)],
                        Some(value)
                    );
                    let end = tick(vdp.now().get() + H32_LINE_TICKS);
                    vdp.advance_to(end, region).unwrap();
                    copy.advance_to(end, region).unwrap();
                    assert_eq!(vdp, copy);
                    if code == VRAM_WRITE_CODE {
                        assert!(vdp.fifo().is_empty());
                        assert_eq!(&vdp.memory.vram()[..2], &[0xca, 0xfe]);
                    } else {
                        assert!(vdp.read_cache().is_ready());
                    }
                    restored(&vdp, region);
                }
            }
        }
    }
}

#[test]
fn non_memory_mode_writes_preserve_pending_fifo_and_restore() {
    for region in [Region::NtscUs, Region::Pal] {
        for size in [0, H40_CLOCK_MASK] {
            for display in [false, true] {
                for half in [false, true] {
                    let mut fixture = transfer_fixture(region, size, display);
                    command(&mut fixture, 0, VRAM_WRITE_CODE, region);
                    fixture.write_data(0xcafe, region).unwrap();
                    if half {
                        fixture
                            .advance_to(next_slot(&fixture, region), region)
                            .unwrap();
                        assert_eq!(&fixture.memory.vram()[..2], &[0, 0xfe]);
                    }
                    let mode = fixture.registers().registers()[usize::from(MODE_REGISTER)].unwrap();
                    for toggle in [
                        0,
                        DMA_ENABLE,
                        VINT_ENABLE_MASK,
                        DMA_ENABLE | VINT_ENABLE_MASK,
                    ] {
                        let mut vdp = fixture.clone();
                        let mut control = fixture.clone();
                        let write =
                            queue_mode(&mut vdp, mode ^ toggle, region, toggle == DMA_ENABLE);
                        assert_eq!(
                            vdp.registers().registers()[usize::from(MODE_REGISTER)],
                            Some(mode)
                        );
                        let mut copy = restored(&vdp, region);
                        let end = tick(write.publish_at.get() + H32_LINE_TICKS);
                        vdp.advance_to(end, region).unwrap();
                        copy.advance_to(end, region).unwrap();
                        control.advance_to(end, region).unwrap();
                        assert_eq!(copy, vdp);
                        assert_eq!(vdp.memory, control.memory);
                        assert_eq!(vdp.fifo(), control.fifo());
                        assert!(vdp.fifo().is_empty());
                        assert_eq!(
                            vdp.registers().registers()[usize::from(MODE_REGISTER)],
                            Some(mode ^ toggle)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn non_memory_mode_writes_preserve_both_read_phases() {
    for region in [Region::NtscUs, Region::Pal] {
        for size in [0, H40_CLOCK_MASK] {
            for half in [false, true] {
                let mut vdp = transfer_fixture(region, size, true);
                vdp.memory
                    .write_word(
                        ControlCommand {
                            address: 0,
                            code: VRAM_WRITE_CODE,
                        },
                        0xcafe,
                    )
                    .unwrap();
                command(&mut vdp, 0, VRAM_READ_CODE, region);
                if half {
                    vdp.advance_to(next_slot(&vdp, region), region).unwrap();
                }
                assert_eq!(vdp.state.read_phase, Some(half));
                let mode = vdp.registers().registers()[usize::from(MODE_REGISTER)].unwrap();
                let write = queue_mode(&mut vdp, mode & !DMA_ENABLE, region, false);
                let mut copy = restored(&vdp, region);
                let end = tick(write.publish_at.get() + H32_LINE_TICKS);
                vdp.advance_to(end, region).unwrap();
                copy.advance_to(end, region).unwrap();
                assert_eq!(copy, vdp);
                assert_eq!(vdp.read_data(region).unwrap(), 0xcafe);
            }
        }
    }
}

#[test]
fn unqualified_transfer_changes_and_active_dma_reject_atomically() {
    let mut vdp = transfer_fixture(REGION, 0, true);
    command(&mut vdp, 0, VRAM_WRITE_CODE, REGION);
    vdp.write_data(0xcafe, REGION).unwrap();
    let mode = vdp.registers().registers()[usize::from(MODE_REGISTER)].unwrap();
    for (index, value) in [
        (MODE_REGISTER, mode ^ MODE5_ENABLE),
        (MODE_REGISTER, mode ^ V30_ENABLE),
        (MODE_REGISTER, mode ^ VRAM_128K_ENABLE),
        (DISPLAY_SIZE_REGISTER, H40_CLOCK_MASK | INTERLACE_MODE_MASK),
    ] {
        let before = vdp.clone();
        assert!(vdp.queue_register(index, value, REGION).is_err());
        assert_eq!(vdp, before);
    }
    vdp.state
        .registers
        .queue_control_register(MODE_REGISTER, mode ^ V30_ENABLE)
        .unwrap();
    assert!(vdp.validate_state(REGION).is_err());
    let mut vdp = dma_fixture(DMA_FILL_MODE);
    vdp.write_data(0xcafe, REGION).unwrap();
    let before = vdp.clone();
    assert_eq!(
        vdp.queue_register(MODE_REGISTER, MODE5_ENABLE, REGION),
        Err(VdpScheduleError::Dma(VdpDmaError::Busy))
    );
    assert_eq!(vdp, before);
}

#[test]
fn fifo_half_byte_stale_slots_and_sprite_cache_continue() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut vdp = Vdp::bootstrap_mode5();
        register(&mut vdp, AUTO_INCREMENT_REGISTER, 2, region);
        for (address, data) in [(0, 0x0123), (2, 0x0321), (4, 0xaabb), (6, 0xccdd)] {
            vdp.state
                .fifo
                .push(VdpWrite {
                    command: ControlCommand {
                        address,
                        code: VRAM_WRITE_CODE,
                    },
                    data,
                })
                .unwrap();
        }
        for _ in 0..6 {
            vdp.advance_to(next_slot(&vdp, region), region).unwrap();
        }
        assert_eq!(vdp.fifo().len(), 1);
        vdp.advance_to(next_slot(&vdp, region), region).unwrap();
        assert_eq!(vdp.memory.vram()[7], 0xdd);
        assert_eq!(vdp.memory.vram()[6], 0);
        vdp.memory
            .write_word(
                ControlCommand {
                    address: 0,
                    code: VRAM_WRITE_CODE,
                },
                0xeeee,
            )
            .unwrap();
        let mut copy = restored(&vdp, region);
        assert_eq!(
            copy.state.sprites.entry_words(0).unwrap(),
            [Some(0x0123), Some(0x0321)]
        );
        for _ in 0..4 {
            let at = next_slot(&vdp, region);
            vdp.advance_to(at, region).unwrap();
            copy.advance_to(at, region).unwrap();
            assert_eq!(copy, vdp);
        }
        assert_eq!(copy.fifo().next_slot_data(), Some(0x0123));
        command(&mut vdp, 0, CRAM_READ_CODE, region);
        command(&mut copy, 0, CRAM_READ_CODE, region);
        let at = next_slot(&vdp, region);
        vdp.advance_to(at, region).unwrap();
        copy.advance_to(at, region).unwrap();
        assert_eq!(vdp.read_data(region).unwrap(), 0x0123 & !CRAM_DATA_MASK);
        assert_eq!(copy.read_data(region).unwrap(), 0x0123 & !CRAM_DATA_MASK);
        assert_eq!(copy, vdp);
    }
}

#[test]
fn pending_sprite_table_and_fifo_are_admitted() {
    for code in [CRAM_WRITE_CODE, VRAM_WRITE_CODE, 7, 0x17] {
        let mut vdp = Vdp::bootstrap_mode5();
        register(&mut vdp, AUTO_INCREMENT_REGISTER, 2, REGION);
        vdp.advance_to(tick(vdp.now().get() + 1), REGION).unwrap();
        command(&mut vdp, 0x4000, code, REGION);
        vdp.write_data(0x0eee, REGION).unwrap();
        let write = vdp
            .queue_register(SPRITE_TABLE_REGISTER, 0x20, REGION)
            .unwrap();
        let mut copy = restored(&vdp, REGION);
        let at = tick(write.publish_at.get() + 200);
        vdp.advance_to(at, REGION).unwrap();
        copy.advance_to(at, REGION).unwrap();
        assert_eq!(copy, vdp);
        if code == CRAM_WRITE_CODE {
            assert_eq!(copy.memory.cram()[0], 0x0eee);
        } else if code == VRAM_WRITE_CODE {
            assert_eq!(&copy.memory.vram()[0x4000..0x4002], &[0x0e, 0xee]);
            assert_eq!(
                copy.sprite_cache().unwrap().entry_words(0).unwrap(),
                [Some(0x2ee), None]
            );
        } else {
            assert_eq!(copy.memory, VdpMemory::default());
            assert_eq!(
                copy.sprite_cache().unwrap().entry_words(0).unwrap(),
                [None, None]
            );
        }
    }
}

#[test]
fn pending_register_control_hv_and_interrupt_latches_continue() {
    let mut vdp = Vdp::bootstrap_mode5();
    register(
        &mut vdp,
        HINT_ENABLE_REGISTER,
        HINT_ENABLE_MASK | HV_COUNTER_LATCH_MASK,
        REGION,
    );
    register(
        &mut vdp,
        MODE_REGISTER,
        MODE5_ENABLE | VINT_ENABLE_MASK,
        REGION,
    );
    vdp.advance_to(tick(224 * H32_LINE_TICKS + 40), REGION)
        .unwrap();
    assert!(vdp.interrupts().vertical_pending());
    vdp.advance_to(tick(vdp.now().get() + 1), REGION).unwrap();
    let write = vdp
        .queue_register(HINT_RELOAD_REGISTER, 137, REGION)
        .unwrap();
    vdp.write_control(0x4005, REGION).unwrap();
    let mut copy = restored(&vdp, REGION);
    vdp.advance_to(write.publish_at, REGION).unwrap();
    copy.advance_to(write.publish_at, REGION).unwrap();
    assert_eq!(copy.read_hv(REGION), vdp.read_hv(REGION));
    vdp.write_control(0, REGION).unwrap();
    copy.write_control(0, REGION).unwrap();
    vdp.write_data(0xabcd, REGION).unwrap();
    copy.write_data(0xabcd, REGION).unwrap();
    let at = tick(vdp.now().get() + 200);
    vdp.advance_to(at, REGION).unwrap();
    copy.advance_to(at, REGION).unwrap();
    vdp.acknowledge_cpu();
    copy.acknowledge_cpu();
    assert_eq!(copy, vdp);
}

#[test]
fn read_prefetch_both_byte_phases_and_ready_cache_continue() {
    let mut vdp = Vdp::bootstrap_mode5();
    register(&mut vdp, AUTO_INCREMENT_REGISTER, 2, REGION);
    vdp.invalidate_interval();
    vdp.memory
        .write_word(
            ControlCommand {
                address: 0,
                code: VRAM_WRITE_CODE,
            },
            0xcafe,
        )
        .unwrap();
    command(&mut vdp, 0, VRAM_READ_CODE, REGION);
    for phase in [Some(false), Some(true), None] {
        assert_eq!(vdp.state.read_phase, phase);
        let mut copy = restored(&vdp, REGION);
        let at = next_slot(&vdp, REGION);
        vdp.advance_to(at, REGION).unwrap();
        copy.advance_to(at, REGION).unwrap();
        assert_eq!(copy, vdp);
    }
    assert_eq!(vdp.read_data(REGION).unwrap(), 0xcafe);
    restored(&vdp, REGION);
}

fn dma_fixture(mode: u8) -> Vdp {
    dma_alias_fixture(mode, 0x30)
}

fn dma_alias_fixture(mode: u8, copy_code: u8) -> Vdp {
    let mut vdp = Vdp::bootstrap_mode5();
    for (index, value) in [
        (MODE_REGISTER, MODE5_ENABLE | DMA_ENABLE),
        (AUTO_INCREMENT_REGISTER, 2),
        (DMA_LENGTH_LOW_REGISTER, 2),
        (DMA_SOURCE_REGISTER, mode),
    ] {
        register(&mut vdp, index, value, REGION);
    }
    vdp.invalidate_interval();
    vdp.memory
        .write_word(
            ControlCommand {
                address: 0,
                code: VRAM_WRITE_CODE,
            },
            0x1234,
        )
        .unwrap();
    command(
        &mut vdp,
        0x2000,
        if mode == DMA_COPY_MODE {
            copy_code
        } else {
            0x21
        },
        REGION,
    );
    vdp
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn fill_waiting_trigger_and_active_fill_continue() {
    pending_fill::waiting_control_contract();
    cram_provenance::cold_fill_retirement_and_reads();
    let mut seed = Vdp::bootstrap_mode5();
    register(&mut seed, AUTO_INCREMENT_REGISTER, 2, REGION);
    seed.memory
        .write_word(
            ControlCommand {
                address: 0,
                code: VRAM_WRITE_CODE,
            },
            0xcafe,
        )
        .unwrap();
    command(&mut seed, 0, VRAM_READ_CODE, REGION);
    for slots in 0..=2 {
        let mut source = seed.clone();
        for _ in 0..slots {
            let at = next_slot(&source, REGION);
            source.advance_to(at, REGION).unwrap();
        }
        for index in 24..=31 {
            let word = 0x8045 | (index << 8);
            let mut vdp = source.clone();
            let image = *vdp.registers().registers();
            let buffered = vdp.read_cache().buffered_word();
            let high = vdp.command().address & !0x3fff;
            vdp.write_control(word, REGION).unwrap();
            assert_eq!(vdp.registers().registers(), &image);
            assert_eq!(vdp.registers().pending(), None);
            assert_eq!(vdp.state.read_phase, None);
            assert!(!vdp.read_cache().is_ready());
            assert_eq!(vdp.read_cache().buffered_word(), buffered);
            assert_eq!(vdp.command().address, high | u32::from(word & 0x3fff));
            assert_eq!(vdp.command().code, 0x12);
            assert_eq!(vdp.memory, source.memory);
            let mut copy = restored(&vdp, REGION);
            let at = tick(vdp.now().get() + 40);
            vdp.advance_to(at, REGION).unwrap();
            copy.advance_to(at, REGION).unwrap();
            assert_eq!(copy, vdp);
            for target in [&mut vdp, &mut copy] {
                command(target, 0, VRAM_READ_CODE, REGION);
                for _ in 0..2 {
                    let at = next_slot(target, REGION);
                    target.advance_to(at, REGION).unwrap();
                }
                assert_eq!(target.read_data(REGION), Ok(0xcafe));
            }
            assert_eq!(copy, vdp);
        }
    }
    for index in 24..=31 {
        let word = 0x8045 | (index << 8);
        let mut pending = Vdp::bootstrap_mode5();
        pending.advance_to(tick(1), REGION).unwrap();
        pending
            .queue_register(HINT_RELOAD_REGISTER, 1, REGION)
            .unwrap();
        let before = pending.clone();
        assert!(pending.write_control(word, REGION).is_err());
        assert_eq!(pending, before);
        let mut half = Vdp::bootstrap_mode5();
        half.write_control(0x4000, REGION).unwrap();
        half.write_control(word, REGION).unwrap();
        assert_eq!(half.control().pending_word(), None);
        assert_eq!(half.registers().pending(), None);
        assert_eq!(
            half.command(),
            ControlCommand {
                address: 0x14000,
                code: 0x11
            }
        );
        for phase in [dma::Phase::FillWaiting, dma::Phase::Fill(0)] {
            let mut busy = dma_fixture(DMA_FILL_MODE);
            busy.state.dma = Some(phase);
            let before = restored(&busy, REGION);
            assert!(busy.write_control(word, REGION).is_err());
            assert_eq!(busy, before);
        }
    }
    for (length, source, address, increment) in [
        (0u16, 0xffffu16, 0x1ffffu32, 1u8),
        (1, 0xffff, 0xffff, 1),
        (2, 0x123, 0x1ff00, 255),
        (2, 0, 0, 0),
    ] {
        for (code, write_enabled) in [(0x21, true), (0x31, false), (0x32, false)] {
            let mut image = VdpRegisterPipeline::at_h32_edge(tick(0));
            let [lo, hi] = length.to_le_bytes();
            let [slo, shi] = source.to_le_bytes();
            for (offset, value) in [lo, hi, slo, shi, 0xbf].into_iter().enumerate() {
                image.advance_to(tick(image.now().get() + 1)).unwrap();
                let write = image.queue_register(19 + offset as u8, value).unwrap();
                image.advance_to(write.publish_at).unwrap();
            }
            let mut dma = VdpDmaRegisters::from_registers(&image).unwrap();
            let mut target = ControlCommand { address, code };
            if code == 0x32 {
                let before = (dma, target);
                assert!(
                    dma.fill_byte(&mut target, increment, 0xabcd, &mut VdpMemory::default())
                        .is_err()
                );
                assert_eq!((dma, target), before);
            }
            let next = (address + u32::from(increment)) & COMMAND_ADDRESS_MASK;
            let write = dma.complete_fill_service(&mut target, next, 0xabcd, write_enabled);
            assert_eq!(
                write,
                write_enabled.then_some(VdpDmaWrite {
                    address: address as u16 ^ 1,
                    data: 0xab
                })
            );
            assert_eq!(target.address, next);
            assert_eq!(dma.source(), source.wrapping_add(1));
            assert_eq!(dma.length(), length.wrapping_sub(1));
            assert_eq!(dma.source_high(), 0xbf);
            assert_eq!(
                target.code,
                if length == 1 {
                    code & !COMMAND_CD5
                } else {
                    code
                }
            );
        }
    }
    let mut invalid = dma_fixture(DMA_FILL_MODE);
    invalid.state.command.code = 0x32;
    invalid.state.dma = Some(dma::Phase::Fill(0xabcd));
    assert!(
        invalid
            .write_state_in_region(&mut StateWriter::new(), REGION)
            .is_err()
    );
    for queued_target in [CRAM_WRITE_CODE, VSRAM_WRITE_CODE] {
        for fill_code in [0x21, 0x31] {
            let mut vdp = Vdp::bootstrap_mode5();
            for (index, value) in [
                (MODE_REGISTER, MODE5_ENABLE | DMA_ENABLE),
                (AUTO_INCREMENT_REGISTER, 2),
                (DMA_LENGTH_LOW_REGISTER, 2),
                (DMA_SOURCE_REGISTER, DMA_FILL_MODE),
            ] {
                register(&mut vdp, index, value, REGION);
            }
            command(&mut vdp, 0x20, queued_target, REGION);
            vdp.write_data(0x1357, REGION).unwrap();
            vdp.write_data(0x2468, REGION).unwrap();
            command(&mut vdp, 0x2000, fill_code, REGION);
            let mut copy = restored(&vdp, REGION);
            let mut untriggered = restored(&vdp, REGION);
            let mut whole_call = untriggered.clone();
            let whole_before = encoded(&whole_call, REGION);
            let first = next_slot(&untriggered, REGION);
            untriggered.advance_to(first, REGION).unwrap();
            assert_eq!(untriggered.fifo().len(), 1);
            assert_eq!(untriggered.state.dma, Some(dma::Phase::FillWaiting));
            let mut late_restore = restored(&untriggered, REGION);
            let before = encoded(&untriggered, REGION);
            let next = next_slot(&untriggered, REGION);
            assert_eq!(
                whole_call.advance_to(next, REGION),
                Err(VdpScheduleError::Dma(VdpDmaError::PendingInputs))
            );
            assert_eq!(encoded(&whole_call, REGION), whole_before);
            for candidate in [&mut untriggered, &mut late_restore] {
                assert_eq!(
                    candidate.advance_to(next, REGION),
                    Err(VdpScheduleError::Dma(VdpDmaError::PendingInputs))
                );
                assert_eq!(encoded(candidate, REGION), before);
            }
            for phase in [dma::Phase::Fill(0xabcd), dma::Phase::Copy(None)] {
                let mut invalid = vdp.clone();
                invalid.state.dma = Some(phase);
                assert!(
                    invalid
                        .write_state_in_region(&mut StateWriter::new(), REGION)
                        .is_err()
                );
            }
            let mut invalid = vdp.clone();
            invalid.state.fifo = Default::default();
            invalid
                .state
                .fifo
                .push(VdpWrite {
                    command: ControlCommand {
                        address: 0,
                        code: 9,
                    },
                    data: 0,
                })
                .unwrap();
            assert!(
                invalid
                    .write_state_in_region(&mut StateWriter::new(), REGION)
                    .is_err()
            );
            vdp.write_data(0xabcd, REGION).unwrap();
            copy.write_data(0xabcd, REGION).unwrap();
            copy = restored(&copy, REGION);
            for _ in 0..6 {
                let at = next_slot(&vdp, REGION);
                vdp.advance_to(at, REGION).unwrap();
                copy.advance_to(at, REGION).unwrap();
                assert_eq!(copy, vdp);
                copy = restored(&copy, REGION);
            }
            assert_eq!(vdp.state.dma, None);
            assert!(vdp.fifo().is_empty());
            assert_eq!(
                &vdp.memory.vram()[0x2000..0x2006],
                &[0xab, 0xcd, 0, 0xab, 0, 0xab]
            );
            let (target, other, mask): (&[u16], &[u16], u16) = if queued_target == CRAM_WRITE_CODE {
                (vdp.memory.cram(), vdp.memory.vsram(), CRAM_DATA_MASK)
            } else {
                (vdp.memory.vsram(), vdp.memory.cram(), VSRAM_DATA_MASK)
            };
            assert_eq!(&target[16..18], &[0x1357 & mask, 0x2468 & mask]);
            assert!(
                target[..16]
                    .iter()
                    .chain(target[18..].iter())
                    .all(|word| *word == 0)
            );
            assert!(other.iter().all(|word| *word == 0));
        }
    }
    for code in [0x21, 0x31, 0x23, 0x33, 0x25, 0x35] {
        let target = code & !(COMMAND_CD4 | COMMAND_CD5);
        let word_fill = matches!(target, CRAM_WRITE_CODE | VSRAM_WRITE_CODE);
        let mut vdp = dma_fixture(DMA_FILL_MODE);
        vdp.state.dma = None;
        if word_fill {
            command(&mut vdp, 0x4000, VRAM_WRITE_CODE, REGION);
            for word in [0x1379, 0x2f4d, 0x59a3, 0xc6e1] {
                vdp.write_data(word, REGION).unwrap();
                for _ in 0..2 {
                    let at = next_slot(&vdp, REGION);
                    vdp.advance_to(at, REGION).unwrap();
                }
            }
        }
        command(
            &mut vdp,
            if word_fill { 0x7f } else { 0x2000 },
            code,
            REGION,
        );
        assert_eq!(vdp.state.dma, Some(dma::Phase::FillWaiting));
        let memory = vdp.memory.clone();
        let mut copy = restored(&vdp, REGION);
        vdp.write_data(0xabcd, REGION).unwrap();
        copy.write_data(0xabcd, REGION).unwrap();
        copy = restored(&copy, REGION);
        for _ in 0..5 {
            let at = next_slot(&vdp, REGION);
            vdp.advance_to(at, REGION).unwrap();
            copy.advance_to(at, REGION).unwrap();
            assert_eq!(copy, vdp);
            copy = restored(&vdp, REGION);
        }
        assert!(vdp.state.dma.is_none());
        if target == CRAM_WRITE_CODE {
            assert_eq!(vdp.memory.cram()[63], 0xabcd & CRAM_DATA_MASK);
            assert_eq!(&vdp.memory.cram()[..2], &[0x2f4d & CRAM_DATA_MASK; 2]);
            assert_eq!(vdp.memory.vram(), memory.vram());
        } else if target == VSRAM_WRITE_CODE {
            assert_eq!(&vdp.memory.vsram()[..2], &[0x2f4d & VSRAM_DATA_MASK; 2]);
            assert!(vdp.memory.vsram()[2..].iter().all(|word| *word == 0));
            assert_eq!(vdp.memory.cram(), memory.cram());
            assert_eq!(vdp.memory.vram(), memory.vram());
        } else {
            assert_eq!(vdp.memory.vram()[0x2003], 0xab);
        }
    }
}

#[test]
fn copy_prefetched_snapshot_and_external_dma_continue() {
    for code in [0x30, 0x31, 0x33, 0x35, 0x3f] {
        let mut vdp = dma_alias_fixture(DMA_COPY_MODE, code);
        let mut copy = restored(&vdp, REGION);
        let at = next_slot(&vdp, REGION);
        vdp.advance_to(at, REGION).unwrap();
        copy.advance_to(at, REGION).unwrap();
        assert_eq!(copy, vdp);
        assert!(matches!(vdp.state.dma, Some(dma::Phase::Copy(Some(_)))));
        vdp.invalidate_interval();
        vdp.memory
            .write_word(
                ControlCommand {
                    address: 0,
                    code: VRAM_WRITE_CODE,
                },
                0xffff,
            )
            .unwrap();
        copy = restored(&vdp, REGION);
        for _ in 0..4 {
            let at = next_slot(&vdp, REGION);
            vdp.advance_to(at, REGION).unwrap();
            copy.advance_to(at, REGION).unwrap();
            assert_eq!(copy, vdp);
        }
        assert_eq!(vdp.memory.vram()[0x2001], 0x34);
    }

    let mut vdp = dma_fixture(0);
    let mut copy = restored(&vdp, REGION);
    let source = VdpDmaSource::new(&[0xbe, 0xef, 0x12, 0x34], &[]);
    for _ in 0..20 {
        let at = tick(vdp.now().get() + 20);
        vdp.advance_with_source(at, REGION, source).unwrap();
        copy.advance_with_source(at, REGION, source).unwrap();
        assert_eq!(copy, vdp);
        copy = restored(&vdp, REGION);
    }
    assert!(vdp.state.dma.is_none());
    assert!(vdp.fifo().is_empty());
    assert_eq!(
        &vdp.memory.vram()[0x2000..0x2004],
        &[0xbe, 0xef, 0x12, 0x34]
    );
}

#[test]
fn malformed_and_truncated_payloads_leave_target_unchanged() {
    let original = Vdp::bootstrap_mode5();
    let bytes = encoded(&original, REGION);
    let mut target = dma_fixture(DMA_FILL_MODE);
    let before = target.clone();
    for end in (0..200).chain([bytes.len() / 2, bytes.len() - 209, bytes.len() - 1]) {
        assert!(
            target
                .read_state(&mut StateReader::new(&bytes[..end]), REGION)
                .is_err()
        );
        assert_eq!(target, before);
    }
    let mut invalid = bytes.clone();
    invalid[16] = 2;
    assert!(
        target
            .read_state(&mut StateReader::new(&invalid), REGION)
            .is_err()
    );
    assert_eq!(target, before);
    invalid = bytes.clone();
    invalid[0..8].copy_from_slice(&1u64.to_le_bytes());
    assert!(
        target
            .read_state(&mut StateReader::new(&invalid), REGION)
            .is_err()
    );
    assert_eq!(target, before);
    invalid = bytes.clone();
    invalid[bytes.len() - 2] = 0xff;
    assert!(
        target
            .read_state(&mut StateReader::new(&invalid), REGION)
            .is_err()
    );
    assert_eq!(target, before);

    let mut cursor = StateReader::new(&bytes);
    VdpRegisterPipeline::read_state(&mut cursor).unwrap();
    let interrupt = cursor.position();
    VdpInterrupts::read_core_state(&mut cursor, REGION).unwrap();
    let _ = read_optional(&mut cursor, StateReader::read_u16).unwrap();
    let command = cursor.position();
    read_command(&mut cursor).unwrap();
    VdpWriteFifo::read_state(&mut cursor).unwrap();
    let after_fifo = cursor.position();
    VdpReadCache::read_state(&mut cursor).unwrap();
    let _ = read_optional(&mut cursor, StateReader::read_bool).unwrap();
    let dma = cursor.position();
    for (offset, value) in [
        (interrupt + 17, 2),
        (command + 4, 0xff),
        (after_fifo - 4, 4),
        (after_fifo - 3, 1),
        (after_fifo - 2, 1),
        (after_fifo - 1, 16),
        (after_fifo + 1, 1),
        (dma, 0xff),
    ] {
        let mut invalid = bytes.clone();
        invalid[offset] = value;
        assert!(
            target
                .read_state(&mut StateReader::new(&invalid), REGION)
                .is_err(),
            "accepted malformed byte at {offset}"
        );
        assert_eq!(target, before);
    }

    let mut pending = Vdp::bootstrap_mode5();
    pending.advance_to(tick(1), REGION).unwrap();
    pending
        .queue_register(AUTO_INCREMENT_REGISTER, 2, REGION)
        .unwrap();
    let mut invalid = encoded(&pending, REGION);
    invalid[17] = REGISTER_COUNT;
    assert!(
        target
            .read_state(&mut StateReader::new(&invalid), REGION)
            .is_err()
    );
    assert_eq!(target, before);
}
