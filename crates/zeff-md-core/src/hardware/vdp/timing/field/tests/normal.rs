use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};

fn normal(region: Region, h40: bool, tall: bool) -> VdpRegisterPipeline {
    let mut registers = registers(region, h40, tall, false);
    let write = registers
        .queue_control_register(12, if h40 { 0x8b } else { 0x0a })
        .unwrap();
    registers.advance_to(write.publish_at).unwrap();
    registers
}

#[test]
fn normal_interlace_literal_ninth_bit_jump_wrap_and_vc_edges() {
    for (region, tall, first_lines, cases) in [
        (
            Region::NtscUs,
            false,
            263,
            [(234, 0x0ea, 0xea), (235, 0x1e4, 0xe5), (236, 0x1e5, 0xe5)],
        ),
        (
            Region::NtscJapan,
            false,
            263,
            [(234, 0x0ea, 0xea), (235, 0x1e4, 0xe5), (236, 0x1e5, 0xe5)],
        ),
        (
            Region::Pal,
            false,
            313,
            [(255, 0x0ff, 0xfe), (256, 0x100, 1), (258, 0x1c9, 0xc9)],
        ),
        (
            Region::Pal,
            true,
            313,
            [(255, 0x0ff, 0xfe), (256, 0x100, 1), (266, 0x1d1, 0xd1)],
        ),
    ] {
        for h40 in [false, true] {
            let registers = normal(region, h40, tall);
            let timing = VdpVerticalTiming::at_counters_zero(MasterTicks::ZERO);
            let vc = if h40 { 2640 } else { 2660 };
            for (line, internal, external) in cases {
                for offset in [0, 1, H32_LINE_TICKS - vc - 1] {
                    let at = MasterTicks::new((line - 1) * H32_LINE_TICKS + vc + offset);
                    assert_eq!(timing.v_counter_at(at, region, &registers), Ok(internal));
                    assert_eq!(
                        timing.hv_counter_at(at, region, &registers).unwrap() >> 8,
                        external
                    );
                }
            }
            let (line, internal, external) = if region == Region::Pal {
                (256, 0x0ff, 0xfe)
            } else {
                (235, 0x0ea, 0xea)
            };
            let before = MasterTicks::new((line - 1) * H32_LINE_TICKS + vc - 1);
            assert_eq!(
                timing.v_counter_at(before, region, &registers),
                Ok(internal)
            );
            assert_eq!(
                timing.hv_counter_at(before, region, &registers).unwrap() >> 8,
                external
            );
            let first_wrap = (first_lines - 1) * H32_LINE_TICKS + vc;
            for (tick, internal, external) in [(first_wrap - 1, 0x1ff, 0xff), (first_wrap, 0, 0)] {
                let at = MasterTicks::new(tick);
                assert_eq!(timing.v_counter_at(at, region, &registers), Ok(internal));
                assert_eq!(
                    timing.hv_counter_at(at, region, &registers).unwrap() >> 8,
                    external
                );
            }
            let first_end = first_lines * H32_LINE_TICKS;
            let even_jump = if region == Region::Pal {
                if tall { 266 } else { 258 }
            } else {
                235
            };
            let even_internal = if region == Region::Pal {
                if tall { 0x1d2 } else { 0x1ca }
            } else {
                0x1e5
            };
            let even_external = if region == Region::Pal {
                if tall { 0xd3 } else { 0xcb }
            } else {
                0xe5
            };
            let at = MasterTicks::new(first_end + (even_jump - 1) * H32_LINE_TICKS + vc);
            assert_eq!(
                timing.v_counter_at(at, region, &registers),
                Ok(even_internal)
            );
            assert_eq!(
                timing.hv_counter_at(at, region, &registers).unwrap() >> 8,
                even_external
            );
            let position = timing.field_position_at(at, region, &registers).unwrap();
            assert_eq!(position.mode, InterlaceMode::Normal);
            assert_eq!(position.active_mode, InterlaceMode::Normal);
            assert_eq!(position.lines, first_lines - 1);
            assert!(!position.odd && position.active_odd);
        }
    }
}

#[test]
fn normal_interlace_cross_mode_publication_ties_keep_current_and_active_history() {
    for region in [Region::NtscUs, Region::Pal] {
        for h40 in [false, true] {
            for initial in [0, 2, 6] {
                for requested in [0, 2, 6] {
                    if initial == requested {
                        continue;
                    }
                    for offset in [-20i64, 0, 20] {
                        let mut registers = registers(region, h40, false, false);
                        let size = if h40 { 0x81 } else { 0 };
                        let write = registers
                            .queue_control_register(12, size | initial)
                            .unwrap();
                        registers.advance_to(write.publish_at).unwrap();
                        let first_lines = if initial == 0 {
                            region.frame_lines()
                        } else if region == Region::Pal {
                            313
                        } else {
                            263
                        };
                        let start = first_lines * H32_LINE_TICKS;
                        let latch = start + 223 * H32_LINE_TICKS + if h40 { 2640 } else { 2660 };
                        let calendar = FieldCalendar::new(MasterTicks::ZERO)
                            .project_field(MasterTicks::new(start + 100), region, &registers)
                            .unwrap();
                        let publication = (latch as i64 + offset) as u64;
                        registers
                            .advance_to(MasterTicks::new(publication - 1))
                            .unwrap();
                        let write = registers
                            .queue_control_register(12, size | requested)
                            .unwrap();
                        assert_eq!(write.publish_at.get(), publication);
                        let projected = calendar
                            .project_field(MasterTicks::new(latch), region, &registers)
                            .unwrap();
                        assert_eq!(
                            projected.mode,
                            InterlaceMode::from_register(if offset < 0 {
                                requested
                            } else {
                                initial
                            })
                            .unwrap()
                        );
                        assert_eq!(
                            projected.active_mode,
                            InterlaceMode::from_register(initial).unwrap()
                        );
                        let timing = VdpVerticalTiming {
                            blank: None,
                            history: None,
                            horizontal: VdpH32Timing::at_counter_zero(MasterTicks::ZERO),
                            field: projected,
                        };
                        let mut writer = StateWriter::new();
                        timing.write_state(&mut writer);
                        let bytes = writer.into_bytes();
                        assert_eq!(
                            VdpVerticalTiming::read_state(&mut StateReader::new(&bytes)).unwrap(),
                            timing
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn normal_interlace_mode_switch_preserves_odd_and_outgoing_geometry() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut registers = normal(region, true, false);
        let mut interrupts = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
        while interrupts
            .advance_in_mode(registers.now(), region, &registers, false)
            .unwrap()
            .is_some()
        {}
        let end = if region == Region::Pal { 313 } else { 263 } * H32_LINE_TICKS;
        advance(&mut interrupts, end + 100, region, &mut registers);
        let write = registers.queue_control_register(12, 0x87).unwrap();
        advance(
            &mut interrupts,
            write.publish_at.get(),
            region,
            &mut registers,
        );
        let latch = end + 223 * H32_LINE_TICKS + 2640;
        advance(&mut interrupts, latch - 1, region, &mut registers);
        assert_eq!(
            interrupts.field_position(region, &registers).unwrap().mode,
            InterlaceMode::Normal
        );
        advance(&mut interrupts, latch, region, &mut registers);
        let field = interrupts.field_position(region, &registers).unwrap();
        assert_eq!(field.mode, InterlaceMode::Double);
        assert_eq!(field.active_mode, InterlaceMode::Normal);
        assert!(field.odd && field.active_odd);
        assert_eq!(
            interrupts.hv_counter(region, &registers).unwrap() >> 8,
            0xc1
        );
        advance(
            &mut interrupts,
            end + 224 * H32_LINE_TICKS + 8,
            region,
            &mut registers,
        );
        assert!(!interrupts.field_position(region, &registers).unwrap().odd);
    }
}

#[test]
fn normal_interlace_state_tags_roundtrip_and_invalid_tags_reject() {
    for mode in [
        InterlaceMode::Progressive,
        InterlaceMode::Double,
        InterlaceMode::Normal,
    ] {
        let mut writer = StateWriter::new();
        mode.write_state(&mut writer);
        let bytes = writer.into_bytes();
        assert_eq!(
            InterlaceMode::read_state(&mut StateReader::new(&bytes)).unwrap(),
            mode
        );
    }
    for tag in 3..=255 {
        assert!(InterlaceMode::read_state(&mut StateReader::new(&[tag])).is_err());
    }
}
