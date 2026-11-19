use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use crate::hardware::vdp::{VdpDmaError, VdpEvent, VdpInterruptClock};

const LINE: u64 = 20 * H32_LINE_TICKS;

#[path = "clock_transition_tests/blank_mode.rs"]
mod blank_mode;
#[path = "clock_transition_tests/m5_vertical.rs"]
mod m5_vertical;
#[path = "clock_transition_tests/normal.rs"]
mod normal;
const REGION: Region = Region::NtscUs;
const CONFIGURATIONS: [(Region, bool); 4] = [
    (Region::NtscUs, false),
    (Region::NtscJapan, false),
    (Region::Pal, false),
    (Region::Pal, true),
];

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

fn registers(size: u8, display: bool, v30: bool, epoch: u64) -> VdpRegisterPipeline {
    let mut registers = VdpRegisterPipeline::bootstrap_mode5();
    let mode = MODE5_ENABLE
        | DMA_ENABLE
        | VINT_ENABLE_MASK
        | if display { DISPLAY_ENABLE } else { 0 }
        | if v30 { V30_ENABLE } else { 0 };
    for (index, value) in [
        (AUTO_INCREMENT_REGISTER, 2),
        (DISPLAY_SIZE_REGISTER, size),
        (HINT_RELOAD_REGISTER, 255),
        (HINT_ENABLE_REGISTER, HINT_ENABLE_MASK),
        (MODE_REGISTER, mode),
    ] {
        registers
            .advance_to(tick(registers.now().get() + 1))
            .unwrap();
        let write = registers.queue_control_register(index, value).unwrap();
        registers.advance_to(write.publish_at).unwrap();
    }
    let mut writer = StateWriter::new();
    registers.write_state(&mut writer);
    let mut bytes = writer.into_bytes();
    // Register and raster origins may differ by a whole internal clock.
    bytes[..8].copy_from_slice(&epoch.saturating_sub(20).to_le_bytes());
    bytes[8..16].copy_from_slice(&epoch.to_le_bytes());
    VdpRegisterPipeline::read_state(&mut StateReader::new(&bytes)).unwrap()
}

fn fixture(region: Region, size: u8, display: bool, v30: bool, epoch: u64, at: u64) -> Vdp {
    let interrupts = VdpInterrupts::at_counters_zero(tick(epoch), 255, false, false);
    let mut vdp =
        Vdp::from_stable_display_state(interrupts, registers(size, display, v30, epoch), region)
            .unwrap();
    vdp.advance_to(tick(epoch + at), region).unwrap();
    vdp
}

fn encoded(vdp: &Vdp, region: Region) -> Vec<u8> {
    let mut writer = StateWriter::new();
    vdp.write_state_in_region(&mut writer, region).unwrap();
    writer.into_bytes()
}

fn restored(vdp: &Vdp, region: Region) -> Vdp {
    let bytes = encoded(vdp, region);
    let mut reader = StateReader::new(&bytes);
    let mut copy = Vdp::bootstrap_mode5();
    copy.read_state(&mut reader, region).unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(&copy, vdp);
    copy
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

fn write(vdp: &mut Vdp, address: u32, code: u8, data: u16, region: Region) {
    command(vdp, address, code, region);
    vdp.write_data(data, region).unwrap();
}

fn clock_word(value: u8) -> u16 {
    REGISTER_TAG | (u16::from(DISPLAY_SIZE_REGISTER) << REGISTER_INDEX_SHIFT) | u16::from(value)
}

fn toggle(vdp: &mut Vdp, region: Region) -> VdpRegisterWrite {
    let size = vdp.registers().registers()[usize::from(DISPLAY_SIZE_REGISTER)].unwrap();
    vdp.write_control(clock_word(size ^ H40_CLOCK_MASK), region)
        .unwrap();
    vdp.registers().pending().unwrap()
}

fn advance(vdp: &mut Vdp, copies: &mut [Vdp], at: u64, region: Region) {
    vdp.advance_to(tick(at), region).unwrap();
    for copy in copies {
        copy.advance_to(tick(at), region).unwrap();
        assert_eq!(copy, vdp);
        assert_eq!(encoded(copy, region), encoded(vdp, region));
    }
}

fn reject_pending_restore(vdp: &Vdp, region: Region) {
    let mut published = vdp.clone();
    published.state.registers = published.state.registers.published_state().1;
    let mut writer = StateWriter::new();
    published.registers().write_state(&mut writer);
    let prefix = writer.into_bytes().len();
    let mut writer = StateWriter::new();
    vdp.registers().write_state(&mut writer);
    let mut bytes = encoded(&published, region);
    bytes.splice(..prefix, writer.into_bytes());
    let mut target = Vdp::bootstrap_mode5();
    let before = target.clone();
    assert!(
        target
            .read_state(&mut StateReader::new(&bytes), region)
            .is_err()
    );
    assert_eq!(target, before);
}

fn reject_clock(vdp: &mut Vdp, value: u8) {
    let before = vdp.clone();
    assert!(
        vdp.queue_register(DISPLAY_SIZE_REGISTER, value, REGION)
            .is_err()
    );
    assert_eq!(*vdp, before);
}

fn reject_injected_clock(vdp: &mut Vdp, value: u8) {
    vdp.state
        .registers
        .queue_control_register(DISPLAY_SIZE_REGISTER, value)
        .unwrap();
    assert!(vdp.validate_state(REGION).is_err());
    assert!(State::from_known_state(*vdp.interrupts(), *vdp.registers(), REGION).is_err());
    reject_pending_restore(vdp, REGION);
}

#[test]
fn bare_pair_exhibits_the_excluded_counter_edge_ownership() {
    for (old, edges, counter) in [(H40_CLOCK_MASK, 2, 5), (0, 0, 7)] {
        let mut registers = registers(old, false, false, 0);
        let mut interrupts = VdpInterrupts::at_counters_zero(tick(LINE), 7, false, false);
        registers.advance_to(tick(LINE)).unwrap();
        let event = interrupts
            .advance_pair(tick(LINE + 2639), REGION, &mut registers, false)
            .unwrap();
        assert!(event.is_none());
        let write = registers
            .queue_control_register(DISPLAY_SIZE_REGISTER, old ^ H40_CLOCK_MASK)
            .unwrap();
        assert_eq!(write.publish_at, tick(LINE + 2640));
        let mut horizontal = 0;
        while let Some(event) = interrupts
            .advance_pair(tick(LINE + 2660), REGION, &mut registers, false)
            .unwrap()
        {
            horizontal += usize::from(matches!(event,
                VdpEvent::Interrupt(edge) if edge.clock == VdpInterruptClock::HorizontalCounter
            ));
        }
        assert_eq!(horizontal, edges);
        assert_eq!(interrupts.line_counter(), counter);
    }
}

#[test]
fn every_publication_phase_preserves_one_line_clock_in_each_progressive_configuration() {
    let mut covered = 0;
    let mut rejected = 0;
    for epoch in [0, 137] {
        for (region, v30) in CONFIGURATIONS {
            for old in [0, H40_CLOCK_MASK] {
                for display in [false, true] {
                    for phase in (0..H32_LINE_TICKS).step_by(20) {
                        let publication = epoch + LINE + phase;
                        let mut vdp = fixture(region, old, display, v30, epoch, LINE + phase - 1);
                        covered += 1;
                        if phase == 2640 {
                            let before = vdp.clone();
                            assert!(
                                vdp.write_control(clock_word(old ^ H40_CLOCK_MASK), region)
                                    .is_err()
                            );
                            assert_eq!(vdp, before);
                            rejected += 1;
                            continue;
                        }
                        assert_eq!(toggle(&mut vdp, region).publish_at, tick(publication));
                        let mut whole = vdp.clone();
                        let mut copies = [restored(&vdp, region)];
                        advance(&mut vdp, &mut copies, publication, region);
                        assert_eq!(vdp.registers().registers()[12], Some(old ^ H40_CLOCK_MASK));
                        assert_eq!(
                            vdp.registers().horizontal_mode().unwrap().width(),
                            if old == 0 { 320 } else { 256 }
                        );
                        advance(&mut vdp, &mut copies, epoch + LINE + 3400, region);
                        whole.advance_to(tick(epoch + LINE + 3400), region).unwrap();
                        assert_eq!(vdp, whole);
                        assert_eq!(vdp.interrupts().line_counter(), 234);
                        assert!(!vdp.interrupts().horizontal_pending());
                        assert_eq!(vdp.read_hv(region).unwrap() >> 8, 21);
                        let position = vdp.field_position(region).unwrap();
                        assert_eq!(
                            (position.id, position.start, position.line),
                            (0, tick(epoch), 20)
                        );
                        assert!(!position.mode.interlaced() && !position.active_mode.interlaced());
                        assert!(vdp.state.memory_mode_known);
                    }
                }
            }
        }
    }
    assert_eq!((covered, rejected), (5472, 32));
}

#[test]
fn fifo_uses_literal_old_before_publication_and_new_at_publication_slots() {
    for (region, v30) in CONFIGURATIONS {
        for old in [0, H40_CLOCK_MASK] {
            for display in [false, true] {
                let slots = match (old != 0, display) {
                    (false, false) => [320, 336],
                    (false, true) => [528, 656],
                    (true, false) => [330, 350],
                    (true, true) => [370, 690],
                };
                let mut vdp = fixture(region, old, display, v30, 0, LINE + 319);
                write(&mut vdp, 0, VRAM_WRITE_CODE, 0xcafe, region);
                assert_eq!(toggle(&mut vdp, region).publish_at, tick(LINE + 320));
                let captured = vdp.command();
                let mut whole = vdp.clone();
                let mut copies = vec![vdp.clone(), restored(&vdp, region)];
                let mut cuts = vec![
                    319,
                    320,
                    321,
                    slots[0] - 1,
                    slots[0],
                    slots[1] - 1,
                    slots[1],
                ];
                cuts.sort_unstable();
                cuts.dedup();
                for cut in cuts {
                    advance(&mut vdp, &mut copies, LINE + cut, region);
                    assert_eq!(
                        &vdp.memory.vram()[..2],
                        if cut < slots[0] {
                            &[0, 0]
                        } else if cut < slots[1] {
                            &[0, 0xfe]
                        } else {
                            &[0xca, 0xfe]
                        }
                    );
                    assert_eq!(vdp.fifo().len(), usize::from(cut < slots[1]));
                    assert_eq!(vdp.command(), captured);
                    assert_eq!(vdp.registers().pending().is_some(), cut < 320);
                    copies.push(restored(&vdp, region));
                }
                whole.advance_to(tick(LINE + slots[1]), region).unwrap();
                assert_eq!(whole, vdp);
                assert_eq!(vdp.state.sprites.entry_words(0).unwrap()[0], Some(0x2fe));
            }
        }
    }
    let mut vdp = fixture(REGION, 0, false, false, 0, LINE + 201);
    write(&mut vdp, 0, VSRAM_WRITE_CODE, 0x1234, REGION);
    assert_eq!(toggle(&mut vdp, REGION).publish_at, tick(LINE + 220));
    vdp.advance_to(tick(LINE + 210), REGION).unwrap();
    assert!(vdp.fifo().is_empty());
    let mut copy = restored(&vdp, REGION);
    advance(
        &mut vdp,
        std::slice::from_mut(&mut copy),
        LINE + 240,
        REGION,
    );
    assert_eq!(vdp.registers().registers()[12], Some(H40_CLOCK_MASK));
    assert_eq!(vdp.memory.vsram()[0], 0x234);
}

#[test]
fn full_and_wrapped_fifo_preserve_captured_addresses_and_byte_order() {
    for old in [0, H40_CLOCK_MASK] {
        for display in [false, true] {
            let slots = match (old != 0, display) {
                (false, false) => [320, 336, 352, 368, 384, 416, 432, 448],
                (false, true) => [528, 656, 784, 1040, 1168, 1296, 1552, 1680],
                (true, false) => [330, 350, 370, 390, 410, 430, 450, 470],
                (true, true) => [370, 690, 850, 1010, 1330, 1490, 1650, 1970],
            };
            for wrapped in [false, true] {
                let mut vdp = fixture(REGION, old, display, false, 0, LINE + 319);
                if wrapped {
                    for _ in 0..3 {
                        write(&mut vdp, 0x800, VRAM_WRITE_CODE, 0, REGION);
                        vdp.state.fifo.complete_write(&mut vdp.memory).unwrap();
                    }
                }
                command(&mut vdp, 0x100, VRAM_WRITE_CODE, REGION);
                for word in [0x1234, 0x5678, 0x9abc, 0xdef0] {
                    vdp.write_data(word, REGION).unwrap();
                }
                assert_eq!(vdp.fifo().status_bits(), FIFO_FULL_STATUS);
                toggle(&mut vdp, REGION);
                let command = vdp.command();
                assert_eq!(command.address, 0x0c00 | u32::from(old ^ H40_CLOCK_MASK));
                let mut whole = vdp.clone();
                let mut copies = vec![restored(&vdp, REGION)];
                let expected = [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0];
                for (index, slot) in slots.into_iter().enumerate() {
                    advance(&mut vdp, &mut copies, LINE + slot - 1, REGION);
                    assert_eq!(vdp.fifo().len(), 4 - index / 2);
                    advance(&mut vdp, &mut copies, LINE + slot, REGION);
                    let mut bytes = [0; 8];
                    for served in 0..=index {
                        let byte = (served / 2) * 2 + usize::from(served % 2 == 0);
                        bytes[byte] = expected[byte];
                    }
                    assert_eq!(&vdp.memory.vram()[0x100..0x108], &bytes);
                    assert_eq!(vdp.fifo().len(), 4 - index.div_ceil(2));
                    assert_eq!(vdp.command(), command);
                    copies.push(restored(&vdp, REGION));
                }
                whole.advance_to(tick(LINE + slots[7]), REGION).unwrap();
                assert_eq!(whole, vdp);
                assert_eq!(vdp.fifo().status_bits(), FIFO_EMPTY_STATUS);
            }
        }
    }
}

#[test]
fn partial_vram_and_completion_time_sprite_aperture_survive_width_changes() {
    for (old, display, accept, first, publication, second) in [
        (0, false, 201, 210, 220, 224),
        (0, true, 201, 210, 220, 272),
        (H40_CLOCK_MASK, false, 261, 272, 280, 290),
        (H40_CLOCK_MASK, true, 261, 272, 280, 370),
    ] {
        for odd in [false, true] {
            let mut vdp = fixture(REGION, old, display, false, 0, LINE + accept);
            let address = 0x100 + u32::from(odd);
            write(&mut vdp, address, VRAM_WRITE_CODE, 0xcafe, REGION);
            vdp.advance_to(tick(LINE + first), REGION).unwrap();
            assert_eq!(
                &vdp.memory.vram()[0x100..0x102],
                if odd { &[0xfe, 0] } else { &[0, 0xfe] }
            );
            let overwrite = ControlCommand {
                address: 0x100,
                code: VRAM_WRITE_CODE,
            };
            vdp.memory.write_word(overwrite, 0x125a).unwrap();
            vdp.advance_to(tick(LINE + first + 1), REGION).unwrap();
            assert_eq!(
                toggle(&mut vdp, REGION).publish_at,
                tick(LINE + publication)
            );
            let captured = vdp.command();
            assert_eq!(captured.address, 0x0c00 | u32::from(old ^ H40_CLOCK_MASK));
            let mut whole = vdp.clone();
            let mut copies = vec![restored(&vdp, REGION)];
            for cut in [publication - 1, publication, second - 1, second] {
                advance(&mut vdp, &mut copies, LINE + cut, REGION);
                copies.push(restored(&vdp, REGION));
            }
            whole.advance_to(tick(LINE + second), REGION).unwrap();
            assert_eq!(whole, vdp);
            assert_eq!(
                &vdp.memory.vram()[0x100..0x102],
                if odd { &[0x12, 0xca] } else { &[0xca, 0x5a] }
            );
            assert_eq!(vdp.command(), captured);
            assert!(vdp.fifo().is_empty());
        }
    }
    let h40 = registers(H40_CLOCK_MASK, false, false, 0);
    for old in [0, H40_CLOCK_MASK] {
        for index in [0u8, 64, 79] {
            let mut vdp = fixture(REGION, old, false, false, 0, LINE + 201);
            let change = vdp
                .queue_register(SPRITE_TABLE_REGISTER, 1, REGION)
                .unwrap();
            vdp.advance_to(change.publish_at, REGION).unwrap();
            write(&mut vdp, 0x200, VRAM_WRITE_CODE, 0x123, REGION);
            vdp.advance_to(tick(LINE + 339), REGION).unwrap();
            let address = 0x200 + u32::from(index) * 8;
            write(&mut vdp, address, VRAM_WRITE_CODE, 0xcafe, REGION);
            let change = toggle(&mut vdp, REGION);
            assert_eq!(change.publish_at, tick(LINE + 340));
            let mut copy = restored(&vdp, REGION);
            let end = if old == 0 { 368 } else { 370 };
            advance(
                &mut vdp,
                std::slice::from_mut(&mut copy),
                LINE + end,
                REGION,
            );
            let masked_base = if old == 0 { 0 } else { 0x200 };
            assert_eq!(vdp.registers().sprite_table_base(), Some(masked_base));
            assert_eq!(
                vdp.state
                    .sprites
                    .entry_words_with_registers(&h40, index)
                    .unwrap()[0],
                if index < 64 || old == 0 {
                    Some(0x2fe)
                } else {
                    None
                }
            );
            assert_eq!(
                vdp.state.sprites.entry_words(0).unwrap()[0],
                Some(if index == 0 { 0x2fe } else { 0x123 })
            );
            assert_eq!(
                &vdp.memory.vram()[address as usize..address as usize + 2],
                &[0xca, 0xfe]
            );
        }
    }
}

#[test]
fn color_and_no_effect_targets_retire_once_on_the_new_calendar() {
    for (region, v30) in CONFIGURATIONS {
        for old in [0, H40_CLOCK_MASK] {
            for display in [false, true] {
                let slot = match (old != 0, display) {
                    (false, false) => 320,
                    (false, true) => 528,
                    (true, false) => 330,
                    (true, true) => 370,
                };
                for code in [CRAM_WRITE_CODE, VSRAM_WRITE_CODE, 7, 0x17] {
                    let mut vdp = fixture(region, old, display, v30, 0, LINE + 319);
                    write(&mut vdp, 1, code, 0xdef7, region);
                    toggle(&mut vdp, region);
                    let captured = vdp.command();
                    assert_eq!(captured.address, 0x0c00 | u32::from(old ^ H40_CLOCK_MASK));
                    let mut copies = vec![restored(&vdp, region)];
                    advance(&mut vdp, &mut copies, LINE + slot - 1, region);
                    assert_eq!(vdp.fifo().len(), 1);
                    assert_eq!(vdp.memory, VdpMemory::default());
                    advance(&mut vdp, &mut copies, LINE + slot, region);
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
                    assert_eq!(vdp.command(), captured);
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn blanking_and_field_boundaries_preserve_split_and_restored_futures() {
    blank_mode::assert_staged_blank_history();
    for epoch in [0, 137] {
        for (region, v30) in CONFIGURATIONS {
            let height = if v30 { 240 } else { 224 };
            let lines = region.frame_lines();
            for old in [0, H40_CLOCK_MASK] {
                for display in [false, true] {
                    for publication in [
                        H32_LINE_TICKS,
                        (height - 1) * H32_LINE_TICKS + 2620,
                        (height - 1) * H32_LINE_TICKS + 2660,
                        height * H32_LINE_TICKS,
                        height * H32_LINE_TICKS + 20,
                        (height + 1) * H32_LINE_TICKS,
                        (height + 1) * H32_LINE_TICKS + 20,
                        (lines - 1) * H32_LINE_TICKS + 2620,
                        lines * H32_LINE_TICKS,
                    ] {
                        let mut vdp = fixture(region, old, display, v30, epoch, publication - 1);
                        assert_eq!(
                            toggle(&mut vdp, region).publish_at,
                            tick(epoch + publication)
                        );
                        let sound = vdp.interrupts().sound_interrupt(vdp.registers(), region);
                        for line in [height, height + 1] {
                            for phase in [7, 8, 9, 10, 19, 20] {
                                let at = line * H32_LINE_TICKS + phase;
                                let size = if at < publication {
                                    old
                                } else {
                                    old ^ H40_CLOCK_MASK
                                };
                                let start =
                                    height * H32_LINE_TICKS + if size == 0 { 10 } else { 8 };
                                assert_eq!(
                                    sound.asserted_at(epoch + at),
                                    Ok((start..start + H32_LINE_TICKS).contains(&at))
                                );
                            }
                        }
                        let mut whole = vdp.clone();
                        let mut copies = vec![restored(&vdp, region)];
                        for cut in [
                            publication,
                            publication + 1,
                            publication + 20,
                            publication + H32_LINE_TICKS,
                        ] {
                            advance(&mut vdp, &mut copies, epoch + cut, region);
                            copies.push(restored(&vdp, region));
                        }
                        whole
                            .advance_to(tick(epoch + publication + H32_LINE_TICKS), region)
                            .unwrap();
                        assert_eq!(vdp, whole);
                        assert_eq!(
                            vdp.field_count(),
                            (publication + H32_LINE_TICKS) / (lines * H32_LINE_TICKS)
                        );
                        let position = vdp.field_position(region).unwrap();
                        assert!(!position.mode.interlaced() && !position.active_mode.interlaced());
                        let phase = (publication + H32_LINE_TICKS) % (lines * H32_LINE_TICKS);
                        let clock = if old == 0 { 2640 } else { 2660 };
                        let blank = (height - 1) * H32_LINE_TICKS + clock
                            ..(lines - 2) * H32_LINE_TICKS + clock;
                        assert_eq!(
                            vdp.state
                                .interrupts
                                .vblank_flag(region, &vdp.state.registers),
                            Ok(blank.contains(&phase))
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn topology_read_dma_strict_and_acknowledge_guards_remain_atomic() {
    m5_vertical::checks();
    for old in [0, H40_CLOCK_MASK] {
        for queued in [false, true] {
            let mut vdp = fixture(REGION, old, false, false, 137, LINE + 2639);
            if queued {
                write(&mut vdp, 0, VSRAM_WRITE_CODE, 0, REGION);
            }
            reject_clock(&mut vdp, old ^ H40_CLOCK_MASK);
            reject_injected_clock(&mut vdp, old ^ H40_CLOCK_MASK);
        }
        for (interlaced, active) in [(true, false), (false, true), (true, true)] {
            for at in [LINE + 319, REGION.frame_lines() * H32_LINE_TICKS - 1] {
                let mut vdp = fixture(REGION, old, false, false, 0, at);
                let mut writer = StateWriter::new();
                vdp.interrupts().write_state(&mut writer);
                let mut bytes = writer.into_bytes();
                bytes[32] = u8::from(interlaced);
                bytes[34] = u8::from(active);
                vdp.state.interrupts =
                    VdpInterrupts::read_state(&mut StateReader::new(&bytes), REGION).unwrap();
                assert!(vdp.validate_state(REGION).is_ok());
                reject_clock(&mut vdp, old ^ H40_CLOCK_MASK);
                reject_injected_clock(&mut vdp, old ^ H40_CLOCK_MASK);
            }
        }
        for value in [
            1,
            0x80,
            (old ^ H40_CLOCK_MASK) | SHADOW_HIGHLIGHT_ENABLE,
            (old ^ H40_CLOCK_MASK) | 0x10,
            (old ^ H40_CLOCK_MASK) | INTERLACE_MODE_MASK,
        ] {
            let mut vdp = fixture(REGION, old, true, false, 0, LINE + 319);
            write(&mut vdp, 0, VRAM_WRITE_CODE, 0xcafe, REGION);
            reject_clock(&mut vdp, value);
        }
        for half in [false, true] {
            let mut vdp = fixture(REGION, old, true, false, 0, LINE + 201);
            command(&mut vdp, 0, VRAM_READ_CODE, REGION);
            if half {
                let slot = if old == 0 { 210 } else { 272 };
                vdp.advance_to(tick(LINE + slot + 1), REGION).unwrap();
            }
            reject_clock(&mut vdp, old ^ H40_CLOCK_MASK);
            vdp.state
                .registers
                .queue_control_register(DISPLAY_SIZE_REGISTER, old ^ H40_CLOCK_MASK)
                .unwrap();
            assert!(vdp.validate_state(REGION).is_err());
            reject_pending_restore(&vdp, REGION);
        }
        for phase in [
            dma::Phase::External,
            dma::Phase::FillWaiting,
            dma::Phase::Fill(0),
            dma::Phase::Copy(None),
        ] {
            let mut vdp = fixture(REGION, old, false, false, 0, LINE + 319);
            vdp.state.dma = Some(phase);
            let before = vdp.clone();
            assert_eq!(
                vdp.queue_register(DISPLAY_SIZE_REGISTER, old ^ H40_CLOCK_MASK, REGION),
                Err(VdpScheduleError::Dma(VdpDmaError::Busy))
            );
            assert_eq!(vdp, before);
        }
        let mut vdp = fixture(REGION, old, false, false, 0, LINE + 319);
        write(&mut vdp, 0, VRAM_WRITE_CODE, 0xcafe, REGION);
        let change = toggle(&mut vdp, REGION);
        let before = vdp.clone();
        assert_eq!(
            vdp.queue_register(HINT_RELOAD_REGISTER, 1, REGION),
            Err(VdpScheduleError::Register(VdpRegisterError::Busy {
                publish_at: change.publish_at
            }))
        );
        assert_eq!(vdp, before);
        assert!(vdp.write_data(0x1234, REGION).is_err());
        assert_eq!(vdp, before);
        assert!(vdp.write_control(0x8f02, REGION).is_err());
        assert_eq!(vdp, before);
        let mut read = vdp.clone();
        read.write_control(0, REGION).unwrap();
        let partial = read.clone();
        assert!(read.write_control(0, REGION).is_err());
        assert_eq!(read, partial);
        assert!(read.read_data(REGION).is_err());
        assert_eq!(read, partial);
        assert_eq!(
            vdp.acknowledge_to(change.publish_at, REGION),
            Err(VdpScheduleError::CoincidentAcknowledge(change.publish_at))
        );
        assert_eq!(vdp, before);
    }
    let mut strict = fixture(REGION, 0, false, false, 0, LINE + 2659);
    let before = strict.clone();
    assert_eq!(
        strict.queue_register(DISPLAY_SIZE_REGISTER, H40_CLOCK_MASK, REGION),
        Err(VdpScheduleError::CoincidentPublication(tick(LINE + 2660)))
    );
    assert_eq!(strict, before);
    assert_eq!(toggle(&mut strict, REGION).publish_at, tick(LINE + 2660));
}

#[test]
fn repeated_changes_keep_each_lines_counter_edge_owned_once() {
    for old in [0, H40_CLOCK_MASK] {
        for requests in [[2599, 2619, 2659], [2659, 2679, 2699]] {
            let mut vdp = fixture(REGION, old, true, false, 137, LINE + requests[0]);
            for request in requests {
                vdp.advance_to(tick(137 + LINE + request), REGION).unwrap();
                let write = toggle(&mut vdp, REGION);
                vdp.advance_to(write.publish_at, REGION).unwrap();
                assert!(vdp.validate_state(REGION).is_ok());
            }
            let mut copy = restored(&vdp, REGION);
            advance(
                &mut vdp,
                std::slice::from_mut(&mut copy),
                137 + LINE + 3400,
                REGION,
            );
            assert_eq!(vdp.interrupts().line_counter(), 234);
            assert_eq!(vdp.read_hv(REGION).unwrap() >> 8, 21);
        }
    }
}
