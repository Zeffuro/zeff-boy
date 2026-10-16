use super::*;

const LINE: u64 = 20 * H32_LINE_TICKS;

fn fixture(region: Region, size: u8, display: bool, v30: bool, phase: u64) -> Vdp {
    let mut vdp = transfer_fixture(region, size, display);
    if region == Region::Pal && !v30 {
        let mode = vdp.registers().registers()[usize::from(MODE_REGISTER)].unwrap();
        register(&mut vdp, MODE_REGISTER, mode & !V30_ENABLE, region);
        vdp = Vdp::from_stable_display_state(*vdp.interrupts(), *vdp.registers(), region).unwrap();
    }
    vdp.advance_to(tick(LINE + phase), region).unwrap();
    vdp
}

fn toggle(vdp: &mut Vdp, region: Region) -> VdpRegisterWrite {
    let mode = vdp.registers().registers()[usize::from(MODE_REGISTER)].unwrap();
    vdp.queue_register(MODE_REGISTER, mode ^ DISPLAY_ENABLE, region)
        .unwrap()
}

fn write(vdp: &mut Vdp, address: u32, code: u8, data: u16, region: Region) {
    command(vdp, address, code, region);
    vdp.write_data(data, region).unwrap();
}

fn advance_all(vdp: &mut Vdp, copies: &mut [Vdp], at: u64, region: Region) {
    vdp.advance_to(tick(LINE + at), region).unwrap();
    for copy in copies {
        copy.advance_to(tick(LINE + at), region).unwrap();
        assert_eq!(copy, vdp);
        assert_eq!(encoded(copy, region), encoded(vdp, region));
    }
}

#[test]
fn publication_splits_literal_old_and_new_slot_timelines() {
    // These offsets specify the existing logical calendar, not physical DE latency.
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for size in [0, H40_CLOCK_MASK] {
            for v30 in [false, true] {
                if v30 && region != Region::Pal {
                    continue;
                }
                for display in [false, true] {
                    let (accept, publication, first, second) = match (size != 0, display) {
                        (false, true) => (201, 220, 210, 230),
                        (false, false) => (201, 220, 210, 370),
                        (true, true) => (301, 320, 320, 336),
                        (true, false) => (301, 320, 304, 528),
                    };
                    let mut vdp = fixture(region, size, display, v30, accept);
                    write(&mut vdp, 0, VRAM_WRITE_CODE, 0xcafe, region);
                    let command = vdp.command();
                    assert_eq!(command.address, 2);
                    let old_mode = vdp.registers().registers()[usize::from(MODE_REGISTER)];
                    let change = toggle(&mut vdp, region);
                    assert_eq!(change.publish_at, tick(LINE + publication));
                    let mut copies = vec![vdp.clone(), restored(&vdp, region)];
                    let mut cuts = vec![
                        publication - 1,
                        publication,
                        publication + 1,
                        publication + 16,
                        first - 1,
                        first,
                        second - 1,
                        second,
                    ];
                    cuts.retain(|&at| at <= second);
                    cuts.sort_unstable();
                    cuts.dedup();
                    for at in cuts {
                        advance_all(&mut vdp, &mut copies, at, region);
                        let expected = if at < first {
                            [0, 0]
                        } else if at < second {
                            [0, 0xfe]
                        } else {
                            [0xca, 0xfe]
                        };
                        assert_eq!(&vdp.memory.vram()[..2], &expected);
                        assert_eq!(vdp.fifo().len(), usize::from(at < second));
                        assert_eq!(vdp.command(), command);
                        assert_eq!(vdp.registers().pending().is_some(), at < publication);
                        assert_eq!(
                            vdp.registers().registers()[usize::from(MODE_REGISTER)],
                            old_mode.map(
                                |mode| mode ^ if at < publication { 0 } else { DISPLAY_ENABLE }
                            )
                        );
                        assert_eq!(vdp.state.memory_mode_known, at < publication);
                        copies.push(restored(&vdp, region));
                    }
                    let mut uninterrupted = fixture(region, size, display, v30, accept);
                    write(&mut uninterrupted, 0, VRAM_WRITE_CODE, 0xcafe, region);
                    toggle(&mut uninterrupted, region);
                    uninterrupted
                        .advance_to(tick(LINE + second), region)
                        .unwrap();
                    assert_eq!(uninterrupted, vdp);
                    assert_eq!(vdp.state.sprites.entry_words(0).unwrap()[0], Some(0x2fe));
                    assert_eq!(vdp.interrupt_level(), 0);
                    vdp.advance_to(tick(LINE + 2 * H32_LINE_TICKS), region)
                        .unwrap();
                    assert!(!vdp.state.memory_mode_known);
                    assert!(vdp.state.validate_memory_configuration(region).is_err());
                    assert!(!restored(&vdp, region).state.memory_mode_known);
                }
            }
        }
    }
}

#[test]
fn half_served_odd_word_preserves_overwritten_first_byte_and_captured_address() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for size in [0, H40_CLOCK_MASK] {
            for display in [false, true] {
                let (accept, first, publication, second) = match (size != 0, display) {
                    (false, true) => (201, 210, 220, 230),
                    (false, false) => (201, 210, 220, 370),
                    (true, true) => (261, 272, 280, 288),
                    (true, false) => (301, 304, 320, 528),
                };
                let mut vdp = fixture(region, size, display, false, accept);
                write(&mut vdp, 0x101, VRAM_WRITE_CODE, 0xcafe, region);
                vdp.advance_to(tick(LINE + first), region).unwrap();
                assert_eq!(&vdp.memory.vram()[0x100..0x102], &[0, 0xca]);
                vdp.memory
                    .write_word(
                        ControlCommand {
                            address: 0x100,
                            code: VRAM_WRITE_CODE,
                        },
                        0x125a,
                    )
                    .unwrap();
                vdp.advance_to(tick(LINE + first + 1), region).unwrap();
                let change = toggle(&mut vdp, region);
                assert_eq!(change.publish_at, tick(LINE + publication));
                let captured = vdp.fifo().front().unwrap();
                assert_eq!(captured.command.address, 0x101);
                let mut copies = vec![restored(&vdp, region), vdp.clone()];
                advance_all(&mut vdp, &mut copies, publication - 1, region);
                assert_eq!(&vdp.memory.vram()[0x100..0x102], &[0x12, 0x5a]);
                advance_all(&mut vdp, &mut copies, second, region);
                assert_eq!(&vdp.memory.vram()[0x100..0x102], &[0xfe, 0x5a]);
                assert!(vdp.fifo().is_empty());
                assert_eq!(vdp.command().address, 0x103);
                assert_eq!(vdp.registers().auto_increment(), Some(2));
            }
        }
    }
}

#[test]
fn full_and_wrapped_fifo_follow_literal_new_calendar_without_replay() {
    for size in [0, H40_CLOCK_MASK] {
        for display in [false, true] {
            let slots: [u64; 8] = match (size != 0, display) {
                (false, true) => [330, 350, 370, 390, 410, 430, 450, 470],
                (false, false) => [370, 690, 850, 1010, 1330, 1490, 1650, 1970],
                (true, true) => [320, 336, 352, 368, 384, 416, 432, 448],
                (true, false) => [528, 656, 784, 1040, 1168, 1296, 1552, 1680],
            };
            for wrapped in [false, true] {
                let mut vdp = fixture(REGION, size, display, false, 319);
                if wrapped {
                    for _ in 0..3 {
                        vdp.state
                            .fifo
                            .push(VdpWrite {
                                command: ControlCommand {
                                    address: 0x200,
                                    code: VRAM_WRITE_CODE,
                                },
                                data: 0,
                            })
                            .unwrap();
                        vdp.state.fifo.complete_write(&mut vdp.memory).unwrap();
                    }
                }
                command(&mut vdp, 0x100, VRAM_WRITE_CODE, REGION);
                for data in [0x1234, 0x5678, 0x9abc, 0xdef0] {
                    vdp.write_data(data, REGION).unwrap();
                }
                assert!(vdp.fifo().is_full());
                assert_eq!(vdp.fifo().status_bits(), FIFO_FULL_STATUS);
                let captured = vdp.command();
                assert_eq!(captured.address, 0x108);
                assert_eq!(toggle(&mut vdp, REGION).publish_at, tick(LINE + 320));
                let mut uninterrupted = vdp.clone();
                let mut copies = vec![restored(&vdp, REGION)];
                let expected = [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0];
                for (phase, at) in slots.into_iter().enumerate() {
                    advance_all(&mut vdp, &mut copies, at - 1, REGION);
                    assert_eq!(vdp.fifo().len(), 4 - phase / 2);
                    advance_all(&mut vdp, &mut copies, at, REGION);
                    let mut bytes = [0; 8];
                    for served in 0..=phase {
                        let byte = (served / 2) * 2 + usize::from(served % 2 == 0);
                        bytes[byte] = expected[byte];
                    }
                    assert_eq!(&vdp.memory.vram()[0x100..0x108], &bytes);
                    assert_eq!(vdp.fifo().len(), 4 - phase.div_ceil(2));
                    assert_eq!(vdp.command(), captured);
                    copies.push(restored(&vdp, REGION));
                }
                uninterrupted
                    .advance_to(tick(LINE + slots[7]), REGION)
                    .unwrap();
                assert_eq!(uninterrupted, vdp);
                assert_eq!(vdp.fifo().status_bits(), FIFO_EMPTY_STATUS);
            }
        }
    }
}

#[test]
fn color_and_unused_targets_retire_once_at_the_new_opportunity() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for size in [0, H40_CLOCK_MASK] {
            for display in [false, true] {
                for code in [CRAM_WRITE_CODE, VSRAM_WRITE_CODE, 7, 0x17] {
                    let mut vdp = fixture(region, size, display, region == Region::Pal, 319);
                    write(&mut vdp, 1, code, 0xdef7, region);
                    toggle(&mut vdp, region);
                    let at = match (size != 0, display) {
                        (false, true) => 330,
                        (false, false) => 370,
                        (true, true) => 320,
                        (true, false) => 528,
                    };
                    let mut copies = vec![restored(&vdp, region)];
                    advance_all(&mut vdp, &mut copies, at - 1, region);
                    assert_eq!(vdp.fifo().len(), 1);
                    assert_eq!(vdp.memory, VdpMemory::default());
                    advance_all(&mut vdp, &mut copies, at, region);
                    assert!(vdp.fifo().is_empty());
                    assert_eq!(
                        vdp.memory.cram()[0],
                        if code == CRAM_WRITE_CODE { 0x0ee6 } else { 0 }
                    );
                    assert_eq!(
                        vdp.memory.vsram()[0],
                        if code == VSRAM_WRITE_CODE { 0x06f7 } else { 0 }
                    );
                    assert!(vdp.memory.vram().iter().all(|&byte| byte == 0));
                    assert_eq!(vdp.state.sprites.entry_words(0).unwrap(), [None, None]);
                    assert_eq!(vdp.command().address, 3);
                }
            }
        }
    }
}

#[test]
fn blank_interval_uses_nonactive_slots_for_both_display_directions() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for size in [0, H40_CLOCK_MASK] {
            for display in [false, true] {
                let base = 250 * H32_LINE_TICKS;
                let mut vdp = fixture(region, size, display, region == Region::Pal, 319);
                vdp.advance_to(tick(base + 319), region).unwrap();
                write(&mut vdp, 0x100, VRAM_WRITE_CODE, 0xcafe, region);
                assert_eq!(toggle(&mut vdp, region).publish_at, tick(base + 320));
                let mut copy = restored(&vdp, region);
                let slots = if size == 0 { [330, 350] } else { [320, 336] };
                for (phase, offset) in slots.into_iter().enumerate() {
                    vdp.advance_to(tick(base + offset), region).unwrap();
                    copy.advance_to(tick(base + offset), region).unwrap();
                    assert_eq!(copy, vdp);
                    assert_eq!(
                        &vdp.memory.vram()[0x100..0x102],
                        if phase == 0 {
                            &[0, 0xfe]
                        } else {
                            &[0xca, 0xfe]
                        }
                    );
                    assert_eq!(vdp.fifo().len(), usize::from(phase == 0));
                    assert!(!vdp.state.memory_mode_known);
                }
            }
        }
    }
}

#[test]
fn publication_acknowledge_collision_rolls_back_all_fifo_progress() {
    for size in [0, H40_CLOCK_MASK] {
        for display in [false, true] {
            let mut vdp = fixture(REGION, size, display, false, 301);
            write(&mut vdp, 0, VRAM_WRITE_CODE, 0xcafe, REGION);
            let change = toggle(&mut vdp, REGION);
            let before = vdp.clone();
            assert_eq!(
                vdp.acknowledge_to(change.publish_at, REGION),
                Err(VdpScheduleError::CoincidentAcknowledge(change.publish_at))
            );
            assert_eq!(vdp, before);
            assert_eq!(encoded(&vdp, REGION), encoded(&before, REGION));
            vdp.advance_to(change.publish_at, REGION).unwrap();
            assert!(!vdp.state.memory_mode_known);
        }
    }
}

#[test]
fn display_exception_preserves_read_dma_unknown_and_structural_guards() {
    let unknown = VdpRegisterPipeline::at_h32_edge(MasterTicks::ZERO);
    assert!(!unknown.changes_display_enable(MODE_REGISTER, MODE5_ENABLE | DISPLAY_ENABLE));
    for half in [false, true] {
        let mut vdp = fixture(REGION, 0, true, false, 201);
        command(&mut vdp, 0, VRAM_READ_CODE, REGION);
        if half {
            vdp.advance_to(tick(LINE + 210), REGION).unwrap();
            vdp.advance_to(tick(LINE + 211), REGION).unwrap();
        }
        let before = vdp.clone();
        let mode = vdp.registers().registers()[usize::from(MODE_REGISTER)].unwrap();
        assert!(
            vdp.queue_register(MODE_REGISTER, mode ^ DISPLAY_ENABLE, REGION)
                .is_err()
        );
        assert_eq!(vdp, before);
        vdp.state
            .registers
            .queue_control_register(MODE_REGISTER, mode ^ DISPLAY_ENABLE)
            .unwrap();
        assert!(vdp.validate_state(REGION).is_err());
    }
    for mode in [DMA_FILL_MODE, DMA_COPY_MODE, 0] {
        let mut vdp = dma_fixture(mode);
        let before = vdp.clone();
        assert_eq!(
            vdp.queue_register(MODE_REGISTER, MODE5_ENABLE | DISPLAY_ENABLE, REGION),
            Err(VdpScheduleError::Dma(VdpDmaError::Busy))
        );
        assert_eq!(vdp, before);
    }
    let mut fixture = fixture(REGION, 0, true, false, 319);
    write(&mut fixture, 0, VRAM_WRITE_CODE, 0xcafe, REGION);
    let mode = fixture.registers().registers()[usize::from(MODE_REGISTER)].unwrap();
    for (index, value) in [
        (MODE_REGISTER, mode ^ (DISPLAY_ENABLE | MODE5_ENABLE)),
        (MODE_REGISTER, mode ^ (DISPLAY_ENABLE | V30_ENABLE)),
        (MODE_REGISTER, mode ^ (DISPLAY_ENABLE | VRAM_128K_ENABLE)),
        (DISPLAY_SIZE_REGISTER, H40_CLOCK_MASK),
        (DISPLAY_SIZE_REGISTER, 1),
    ] {
        let mut vdp = fixture.clone();
        assert!(vdp.queue_register(index, value, REGION).is_err());
        assert_eq!(vdp, fixture);
        match vdp.state.registers.queue_control_register(index, value) {
            Ok(_) => assert!(vdp.validate_state(REGION).is_err()),
            Err(VdpRegisterError::UnsupportedModeChange { .. }) => assert_eq!(vdp, fixture),
            Err(error) => panic!("unexpected register rejection: {error:?}"),
        }
    }
    toggle(&mut fixture, REGION);
    let before = fixture.clone();
    assert!(fixture.write_data(0x1234, REGION).is_err());
    assert_eq!(fixture, before);
}
