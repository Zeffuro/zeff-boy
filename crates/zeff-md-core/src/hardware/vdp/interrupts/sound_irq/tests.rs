use super::*;

fn fixture_registers(mode: u8, horizontal: u8) -> VdpRegisterPipeline {
    let mut registers = VdpRegisterPipeline::bootstrap_mode5();
    for (index, value) in [(MODE_REGISTER, mode), (DISPLAY_SIZE_REGISTER, horizontal)] {
        let write = registers.queue_control_register(index, value).unwrap();
        registers.advance_to(write.publish_at).unwrap();
    }
    registers
}

fn pulse_start(region: Region, mode: u8, horizontal: u8) -> u64 {
    let height = if region == Region::Pal && mode & V30_ENABLE != 0 {
        V30_VBLANK_SET_LINE
    } else {
        V28_VBLANK_SET_LINE
    };
    let offset = if horizontal == 0 {
        H32_VINT_COUNTER * H32_DOT_TICKS
    } else {
        u64::from(H40_DOT_TICKS)
    };
    height * H32_LINE_TICKS + offset
}

#[test]
fn sound_pulse_edges_follow_region_height_horizontal_mode_and_counter_epoch() {
    for (region, height) in [
        (Region::NtscUs, 0),
        (Region::Pal, 0),
        (Region::Pal, V30_ENABLE),
    ] {
        for horizontal in [0, H40_CLOCK_MASK] {
            let mode = MODE5_ENABLE | height;
            for epoch in [0, 137] {
                let interrupts =
                    VdpInterrupts::at_counters_zero(MasterTicks::new(epoch), 0, false, false);
                let view = interrupts.sound_interrupt(fixture_registers(mode, horizontal), region);
                let start = epoch + pulse_start(region, mode, horizontal);
                let period = region.frame_lines() * H32_LINE_TICKS;
                for frame in [0, 1, 7] {
                    let start = start + frame * period;
                    for (at, asserted) in [
                        (start - 1, false),
                        (start, true),
                        (start + H32_LINE_TICKS - 1, true),
                        (start + H32_LINE_TICKS, false),
                        (start + period - 1, false),
                    ] {
                        assert_eq!(view.asserted_at(at), Ok(asserted));
                    }
                }
            }
        }
    }
}

#[test]
fn historical_pending_height_publication_selects_its_published_image() {
    for (before, after) in [(0, V30_ENABLE), (V30_ENABLE, 0)] {
        let interrupts = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
        let mut registers = fixture_registers(MODE5_ENABLE | before, 0);
        let old_start = pulse_start(Region::Pal, MODE5_ENABLE, 0);
        registers
            .advance_to(MasterTicks::new(old_start + 1))
            .unwrap();
        let write = registers
            .queue_control_register(MODE_REGISTER, MODE5_ENABLE | after)
            .unwrap();
        let view = interrupts.sound_interrupt(registers, Region::Pal);
        assert_eq!(view.asserted_at(old_start), Ok(before == 0));
        assert_eq!(
            view.asserted_at(write.publish_at.get() - 1),
            Ok(before == 0)
        );
        assert_eq!(view.asserted_at(write.publish_at.get()), Ok(after == 0));
        let tall_start = pulse_start(Region::Pal, MODE5_ENABLE | V30_ENABLE, 0);
        assert_eq!(view.asserted_at(tall_start), Ok(after != 0));
        assert!(registers.pending().is_some());
    }
}

#[test]
fn historical_pending_horizontal_publication_changes_the_vint_offset() {
    let epoch = 12;
    let interrupts = VdpInterrupts::at_counters_zero(MasterTicks::new(epoch), 0, false, false);
    let old_start = epoch + pulse_start(Region::NtscUs, MODE5_ENABLE, 0);
    let h40_end =
        epoch + pulse_start(Region::NtscUs, MODE5_ENABLE, H40_CLOCK_MASK) + H32_LINE_TICKS;
    for (before, after) in [(0, H40_CLOCK_MASK), (H40_CLOCK_MASK, 0)] {
        let mut registers = fixture_registers(MODE5_ENABLE, before);
        registers.advance_to(MasterTicks::new(h40_end - 1)).unwrap();
        let write = registers
            .queue_control_register(DISPLAY_SIZE_REGISTER, after)
            .unwrap();
        let view = interrupts.sound_interrupt(registers, Region::NtscUs);
        assert_eq!(write.publish_at.get(), h40_end);
        let reference_before =
            interrupts.sound_interrupt(fixture_registers(MODE5_ENABLE, before), Region::NtscUs);
        let reference_after =
            interrupts.sound_interrupt(fixture_registers(MODE5_ENABLE, after), Region::NtscUs);
        assert_eq!(
            view.asserted_at(write.publish_at.get() - 1),
            reference_before.asserted_at(write.publish_at.get() - 1)
        );
        assert_eq!(
            view.asserted_at(write.publish_at.get()),
            reference_after.asserted_at(write.publish_at.get())
        );
        let next = Region::NtscUs.frame_lines() * H32_LINE_TICKS;
        for at in [
            old_start - 2,
            old_start - 1,
            old_start,
            old_start + H32_LINE_TICKS - 2,
        ] {
            assert_eq!(
                view.asserted_at(next + at),
                reference_after.asserted_at(next + at)
            );
        }
    }
}

#[test]
fn sound_line_ignores_display_vint_enable_main_pending_status_and_acknowledge() {
    for flags in [
        0,
        DISPLAY_ENABLE,
        VINT_ENABLE_MASK,
        DISPLAY_ENABLE | VINT_ENABLE_MASK,
    ] {
        let registers = fixture_registers(MODE5_ENABLE | flags, H40_CLOCK_MASK);
        let start = pulse_start(Region::NtscUs, MODE5_ENABLE, H40_CLOCK_MASK);
        for pending in [false, true] {
            let mut interrupts =
                VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, pending);
            let before = interrupts.sound_interrupt(registers, Region::NtscUs);
            let _ = interrupts.status_bits();
            interrupts.complete_acknowledge(&registers).unwrap();
            let after = interrupts.sound_interrupt(registers, Region::NtscUs);
            assert_eq!(before, after);
            assert_eq!(before.asserted_at(start), Ok(true));
            assert_eq!(after.asserted_at(start + H32_LINE_TICKS), Ok(false));
        }
    }
}

#[test]
fn unsupported_modes_and_time_before_counter_epoch_retain_timing_errors() {
    let epoch = MasterTicks::new(137);
    let interrupts = VdpInterrupts::at_counters_zero(epoch, 0, false, false);
    let before = interrupts.sound_interrupt(fixture_registers(MODE5_ENABLE, 0), Region::NtscUs);
    assert_eq!(
        before.asserted_at(136),
        Err(VdpTimingError::BeforeEpoch {
            epoch,
            requested: MasterTicks::new(136),
        })
    );
    let tall = interrupts.sound_interrupt(
        fixture_registers(MODE5_ENABLE | V30_ENABLE, 0),
        Region::NtscUs,
    );
    assert_eq!(
        tall.asserted_at(137),
        Err(VdpTimingError::UnsupportedMode {
            index: MODE_REGISTER,
            value: MODE5_ENABLE | V30_ENABLE,
        })
    );
}
