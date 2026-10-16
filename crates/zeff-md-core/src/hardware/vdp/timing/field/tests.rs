use super::*;
use crate::hardware::vdp::VdpInterrupts;

mod boundaries;

fn registers(region: Region, h40: bool, tall: bool, interlaced: bool) -> VdpRegisterPipeline {
    let mut registers = VdpRegisterPipeline::bootstrap_mode5();
    for (index, value) in [
        (
            MODE_REGISTER,
            MODE5_ENABLE | DISPLAY_ENABLE | if tall { V30_ENABLE } else { 0 },
        ),
        (
            DISPLAY_SIZE_REGISTER,
            (if h40 { H40_CLOCK_MASK } else { 0 })
                | if interlaced { INTERLACE_MODE_MASK } else { 0 },
        ),
    ] {
        let write = registers.queue_control_register(index, value).unwrap();
        registers.advance_to(write.publish_at).unwrap();
    }
    assert!(region == Region::Pal || !tall);
    registers
}

fn advance(
    interrupts: &mut VdpInterrupts,
    at: u64,
    region: Region,
    registers: &mut VdpRegisterPipeline,
) {
    while interrupts
        .advance_pair(MasterTicks::new(at), region, registers, false)
        .unwrap()
        .is_some()
    {}
}

#[test]
fn latch_toggle_wrap_and_outgoing_rows_follow_one_calendar() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for h40 in [false, true] {
            for tall in [false, true]
                .into_iter()
                .filter(|&tall| !tall || region == Region::Pal)
            {
                let mut registers = registers(region, h40, tall, true);
                let mut interrupts =
                    VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, true);
                while interrupts
                    .advance_in_mode(registers.now(), region, &registers, false)
                    .unwrap()
                    .is_some()
                {}
                let height = registers.display_height(region) as u64;
                let mode = VdpVerticalTiming::horizontal_mode(&registers).unwrap();
                let latch = VdpVerticalTiming::blank_offset(height, mode.vertical_clock());
                advance(&mut interrupts, latch - 1, region, &mut registers);
                assert!(
                    !interrupts
                        .field_position(region, &registers)
                        .unwrap()
                        .interlaced
                );
                advance(&mut interrupts, latch, region, &mut registers);
                let position = interrupts.field_position(region, &registers).unwrap();
                assert!(position.interlaced);
                assert!(!position.odd && !position.active_interlaced);
                advance(
                    &mut interrupts,
                    height * H32_LINE_TICKS,
                    region,
                    &mut registers,
                );
                let (row, outgoing) = interrupts
                    .completed_row(interrupts.now(), region, &registers)
                    .unwrap()
                    .unwrap();
                assert_eq!(u64::from(row), height - 1);
                assert!(!outgoing.active_interlaced);
                let toggle = height * H32_LINE_TICKS + mode.vertical_interrupt();
                advance(&mut interrupts, toggle, region, &mut registers);
                assert_eq!(interrupts.status_bits() & 0x10, 0x10);
                let base = if region == Region::Pal { 312 } else { 262 };
                let first_end = (base + 1) * H32_LINE_TICKS;
                assert_eq!(
                    interrupts.next_field_end(region, &registers).unwrap().get(),
                    first_end
                );
                advance(&mut interrupts, first_end, region, &mut registers);
                let position = interrupts.field_position(region, &registers).unwrap();
                assert_eq!((position.id, position.line), (1, 0));
                assert!(position.active_interlaced && position.active_odd);
                advance(&mut interrupts, first_end + toggle, region, &mut registers);
                assert_eq!(interrupts.status_bits() & 0x10, 0);
                assert_eq!(
                    interrupts.next_field_end(region, &registers).unwrap().get(),
                    first_end + base * H32_LINE_TICKS
                );
            }
        }
    }
}

#[test]
fn interlace_internal_v_and_hv_keep_counter_bit_separate_from_parity() {
    for (region, tall, low_count, high_start) in [
        (Region::NtscUs, false, 235, 0x1e4),
        (Region::Pal, false, 258, 0x1c9),
        (Region::Pal, true, 266, 0x1d1),
    ] {
        for h40 in [false, true] {
            let registers = registers(region, h40, tall, true);
            let timing = VdpVerticalTiming::at_counters_zero(MasterTicks::ZERO);
            for (line, internal) in [
                (low_count - 1, low_count - 1),
                (low_count, high_start),
                (low_count + 1, high_start + 1),
            ] {
                let at = MasterTicks::new(line * H32_LINE_TICKS);
                assert_eq!(
                    timing.v_counter_at(at, region, &registers).unwrap(),
                    internal as u16
                );
                assert_eq!(
                    (timing.hv_counter_at(at, region, &registers).unwrap() >> 8) as u8,
                    (internal as u8).rotate_left(1)
                );
            }
        }
    }
}

#[test]
fn disabling_interlace_keeps_shifted_origin_and_outgoing_geometry() {
    let region = Region::NtscUs;
    let mut registers = registers(region, false, false, true);
    let calendar = FieldCalendar::new(MasterTicks::ZERO);
    let first_end = 263 * H32_LINE_TICKS;
    let (calendar, _) = calendar
        .project(MasterTicks::new(first_end + 1), region, &registers)
        .unwrap();
    registers
        .advance_to(MasterTicks::new(first_end + 1))
        .unwrap();
    let write = registers
        .queue_control_register(DISPLAY_SIZE_REGISTER, 0)
        .unwrap();
    let (calendar, registers) = calendar
        .project(
            MasterTicks::new(first_end + 224 * H32_LINE_TICKS),
            region,
            &registers,
        )
        .unwrap();
    assert!(!calendar.interlaced && !calendar.odd);
    assert!(calendar.active_interlaced && calendar.active_odd);
    assert!(write.publish_at.get() < calendar.updated_at.get());
    let end = first_end + 262 * H32_LINE_TICKS;
    let (calendar, _) = calendar
        .project(
            MasterTicks::new(end + 5 * 262 * H32_LINE_TICKS + 1),
            region,
            &registers,
        )
        .unwrap();
    assert_eq!(calendar.start.get(), end + 5 * 262 * H32_LINE_TICKS);
    assert_eq!(calendar.id, 7);
    assert!(!calendar.active_interlaced);
}

#[test]
fn publication_before_at_and_after_blank_uses_the_old_image_on_a_tie() {
    for h40 in [false, true] {
        for offset in [-20_i64, 0, 20] {
            let region = Region::NtscUs;
            let mut registers = registers(region, h40, false, false);
            let mode = VdpVerticalTiming::horizontal_mode(&registers).unwrap();
            let latch = VdpVerticalTiming::blank_offset(224, mode.vertical_clock());
            let publication = (latch as i64 + offset) as u64;
            registers
                .advance_to(MasterTicks::new(publication - 1))
                .unwrap();
            let write = registers
                .queue_control_register(
                    DISPLAY_SIZE_REGISTER,
                    (if h40 { H40_CLOCK_MASK } else { 0 }) | INTERLACE_MODE_MASK,
                )
                .unwrap();
            assert_eq!(write.publish_at.get(), publication);
            let calendar = FieldCalendar::new(MasterTicks::ZERO);
            let (at_latch, _) = calendar
                .project(MasterTicks::new(latch), region, &registers)
                .unwrap();
            assert_eq!(at_latch.interlaced, offset < 0);
            let second_latch = 262 * H32_LINE_TICKS + latch;
            let (later, _) = calendar
                .project(MasterTicks::new(second_latch), region, &registers)
                .unwrap();
            assert!(later.interlaced);
        }
    }
}

#[test]
fn sound_snapshot_projects_pending_mode_through_the_same_field_edges() {
    let region = Region::Pal;
    let mut registers = registers(region, true, false, false);
    registers.advance_to(MasterTicks::new(10_001)).unwrap();
    registers
        .queue_control_register(DISPLAY_SIZE_REGISTER, H40_CLOCK_MASK | INTERLACE_MODE_MASK)
        .unwrap();
    let interrupts = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
    let snapshot = interrupts.sound_interrupt(registers, region);
    let timing = VdpVerticalTiming::at_counters_zero(MasterTicks::ZERO);
    let pulse = 224 * H32_LINE_TICKS + u64::from(H40_DOT_TICKS);
    for origin in [0, 313 * H32_LINE_TICKS, (313 + 312) * H32_LINE_TICKS] {
        for (offset, asserted) in [
            (pulse - 1, false),
            (pulse, true),
            (pulse + H32_LINE_TICKS - 1, true),
            (pulse + H32_LINE_TICKS, false),
        ] {
            let at = origin + offset;
            assert_eq!(snapshot.asserted_at(at), Ok(asserted));
            let position = timing
                .field_position_at(MasterTicks::new(at), region, &registers)
                .unwrap();
            assert_eq!(position.start.get(), origin);
        }
    }
}

#[test]
fn every_legacy_state_version_derives_progressive_origin_without_changing_ticks() {
    use crate::hardware::vdp::state::{StateReader, StateWriter};
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for version in 1..14 {
            let epoch = 137;
            let now = epoch + region.frame_lines() * H32_LINE_TICKS * 9 + 123;
            let mut writer = StateWriter::new();
            writer.write_u64(epoch);
            writer.write_u64(now);
            writer.write_u8(17);
            writer.write_bool(true);
            writer.write_bool(false);
            let bytes = writer.into_bytes();
            let mut reader = StateReader::new(&bytes);
            let interrupts = VdpInterrupts::read_state(&mut reader, version, region).unwrap();
            assert!(reader.is_exhausted());
            assert_eq!(interrupts.now().get(), now);
            let mut registers = registers(region, false, false, false);
            registers.advance_to(MasterTicks::new(now)).unwrap();
            let position = interrupts.field_position(region, &registers).unwrap();
            assert_eq!(
                (position.id, position.line, position.start.get()),
                (9, 0, now - 123)
            );
            assert!(!position.interlaced && !position.active_interlaced && !position.odd);
        }
    }
}

#[test]
fn interlace_state_round_trip_retains_latched_and_active_field_history() {
    use crate::hardware::vdp::state::{StateReader, StateWriter};
    use crate::save_state::STATE_VERSION_WITH_INTERLACE;
    for region in [Region::NtscUs, Region::Pal] {
        let mut registers = registers(region, true, false, true);
        let mut interrupts = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
        while interrupts
            .advance_in_mode(registers.now(), region, &registers, false)
            .unwrap()
            .is_some()
        {}
        let first_end = if region == Region::Pal { 313 } else { 263 } * H32_LINE_TICKS;
        for at in [
            223 * H32_LINE_TICKS + H40_VCOUNTER_TICKS,
            224 * H32_LINE_TICKS + u64::from(H40_DOT_TICKS),
            first_end + 123 * H32_LINE_TICKS + 17,
        ] {
            advance(&mut interrupts, at, region, &mut registers);
            let mut writer = StateWriter::new();
            interrupts.write_state(&mut writer);
            let bytes = writer.into_bytes();
            let mut reader = StateReader::new(&bytes);
            let restored =
                VdpInterrupts::read_state(&mut reader, STATE_VERSION_WITH_INTERLACE, region)
                    .unwrap();
            assert_eq!(restored, interrupts);
            restored.validate_state(&registers, region).unwrap();
            assert!(reader.is_exhausted());
            let mut malformed = bytes;
            malformed[8..16].copy_from_slice(&(at + H32_LINE_TICKS).to_le_bytes());
            let invalid = VdpInterrupts::read_state(
                &mut StateReader::new(&malformed),
                STATE_VERSION_WITH_INTERLACE,
                region,
            )
            .unwrap();
            assert!(invalid.validate_state(&registers, region).is_err());
        }
    }
}
