use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    Region, Vdp, VdpInterruptClock, VdpInterruptEdge, VdpInterrupts, VdpRegisterPipeline,
    VdpScheduleError, VdpTimingError, VdpVBlankEdge, VdpVerticalTiming,
};

const LINE: u64 = 3420;
const PAL_LINES: u64 = 313;
const PAL_FIELD: u64 = 1_070_460;

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

fn known(r1: u8, r12: u8, reload: u8) -> VdpRegisterPipeline {
    let mut registers = VdpRegisterPipeline::at_h32_edge(tick(0));
    for (index, value) in [(0, 0x10), (1, r1), (12, r12), (10, reload)] {
        registers
            .advance_to(tick(registers.now().get() + 1))
            .unwrap();
        let write = registers.queue_register(index, value).unwrap();
        registers.advance_to(write.publish_at).unwrap();
    }
    registers
}

fn device(region: Region, r1: u8, r12: u8, reload: u8) -> Vdp {
    let mut device = Vdp::bootstrap_mode5();
    for (index, value) in [(0, 0x10), (1, r1), (12, r12), (10, reload)] {
        device
            .advance_to(tick(device.now().get() + 1), region)
            .unwrap();
        let write = device.queue_register(index, value, region).unwrap();
        device.advance_to(write.publish_at, region).unwrap();
    }
    device
}

fn raw_counter(ordinal: u64, height: u64) -> u16 {
    let (low_end, high_start) = if height == 240 {
        (0x10a, 0x1d2)
    } else {
        (0x102, 0x1ca)
    };
    (0..=low_end)
        .chain(high_start..=0x1ff)
        .nth((ordinal % PAL_LINES) as usize)
        .unwrap()
}

pub fn vertical_counter_and_blank_edges() {
    vertical_edges(224);
}

fn vertical_edges(height: u64) {
    for (width, clock) in [(0, 2660), (0x81, 2640)] {
        let registers = known(if height == 240 { 0x4c } else { 0x44 }, width, 0);
        for epoch in [137, 144] {
            let timing = VdpVerticalTiming::at_counters_zero(tick(epoch));
            let saved = timing;
            for ordinal in 0..PAL_LINES * 2 {
                let edge = ordinal * LINE + clock;
                for (at, incoming) in [
                    (edge - 1, ordinal),
                    (edge, ordinal + 1),
                    (edge + 1, ordinal + 1),
                ] {
                    let raw = raw_counter(incoming, height);
                    assert_eq!(
                        timing.v_counter_at(tick(epoch + at), Region::Pal, &registers),
                        Ok(raw)
                    );
                    assert_eq!(
                        timing.vblank_flag_at(tick(epoch + at), Region::Pal, &registers),
                        Ok((height as u16..=0x1fe).contains(&raw))
                    );
                }
            }
            for field in 0..2 {
                let base = epoch + field * PAL_FIELD;
                for (offset, active) in [
                    ((height - 1) * LINE + clock, true),
                    (311 * LINE + clock, false),
                ] {
                    let at = base + offset;
                    let edge = VdpVBlankEdge {
                        at: tick(at),
                        active,
                    };
                    assert_eq!(
                        timing.next_vblank_edge(tick(at - 1), Region::Pal, &registers),
                        Ok(edge)
                    );
                    for (delta, expected) in [(-1, !active), (0, active), (1, active)] {
                        assert_eq!(
                            timing.vblank_flag_at(
                                tick((at as i64 + delta) as u64),
                                Region::Pal,
                                &registers
                            ),
                            Ok(expected)
                        );
                    }
                    let next = if active {
                        VdpVBlankEdge {
                            at: tick(base + 311 * LINE + clock),
                            active: false,
                        }
                    } else {
                        VdpVBlankEdge {
                            at: tick(base + PAL_FIELD + (height - 1) * LINE + clock),
                            active: true,
                        }
                    };
                    assert_eq!(
                        timing.next_vblank_edge(tick(at), Region::Pal, &registers),
                        Ok(next)
                    );
                }
            }
            assert_eq!(timing, saved);
        }
    }
}

pub fn interrupt_calendar_and_hint_counter() {
    interrupt_calendar(224);
}

fn interrupt_calendar(height: u64) {
    let epoch = 1000;
    for (width, clock, vint) in [(0, 2660, 10), (0x81, 2640, 8)] {
        for reload in [0, 1, 223, 224, 239, 240, 255] {
            let registers = known(if height == 240 { 0x6c } else { 0x64 }, width, reload);
            let timing = VdpVerticalTiming::at_counters_zero(tick(epoch));
            let mut irq = VdpInterrupts::at_counters_zero(tick(epoch), reload, false, false);
            let mut counter = reload;
            for ordinal in 0..PAL_LINES * 2 {
                let base = epoch + ordinal * LINE;
                if ordinal % PAL_LINES == height {
                    let at = base + vint;
                    let expected = VdpInterruptEdge {
                        at: tick(at),
                        clock: VdpInterruptClock::Vertical,
                    };
                    assert_eq!(
                        timing.next_interrupt_edge(tick(at - 1), Region::Pal, &registers),
                        Ok(expected)
                    );
                    assert_eq!(
                        irq.advance_to(tick(at - 1), Region::Pal, &registers),
                        Ok(None)
                    );
                    assert!(!irq.vertical_pending());
                    assert_eq!(
                        irq.advance_to(tick(at), Region::Pal, &registers),
                        Ok(Some(expected))
                    );
                    assert_eq!(irq.status_bits(), 0x80);
                    assert_eq!(irq.interrupt_level(&registers), Ok(6));
                    assert_eq!(irq.complete_acknowledge(&registers), Ok(6));
                    assert!(!irq.vertical_pending());
                }
                let at = base + clock;
                let expected = VdpInterruptEdge {
                    at: tick(at),
                    clock: VdpInterruptClock::HorizontalCounter,
                };
                assert_eq!(
                    timing.next_interrupt_edge(tick(at - 1), Region::Pal, &registers),
                    Ok(expected)
                );
                assert_eq!(
                    irq.advance_to(tick(at - 1), Region::Pal, &registers),
                    Ok(None)
                );
                assert_eq!(irq.line_counter(), counter);
                assert_eq!(
                    irq.advance_to(tick(at), Region::Pal, &registers),
                    Ok(Some(expected))
                );
                let decrements = u64::from(raw_counter(ordinal + 1, height)) <= height;
                let pending = decrements && counter == 0;
                counter = if !decrements || counter == 0 {
                    reload
                } else {
                    counter - 1
                };
                assert_eq!(irq.line_counter(), counter);
                assert_eq!(irq.horizontal_pending(), pending);
                assert_eq!(
                    irq.complete_acknowledge(&registers),
                    Ok(if pending { 4 } else { 0 })
                );
                assert_eq!(
                    irq.advance_to(tick(at + 1), Region::Pal, &registers),
                    Ok(None)
                );
            }
        }
    }
}

pub fn acknowledge_edges_and_status() {
    acknowledge_status(224);
}

fn acknowledge_status(height: u64) {
    for (width, clock, vint) in [(0, 2660, 10), (0x81, 2640, 8)] {
        let mut hint = device(Region::Pal, 4, width, 0);
        assert_eq!(hint.acknowledge_to(tick(clock - 1), Region::Pal), Ok(0));
        let saved = hint.clone();
        assert_eq!(
            hint.acknowledge_to(tick(clock), Region::Pal),
            Err(VdpScheduleError::CoincidentAcknowledge(tick(clock)))
        );
        assert_eq!(hint, saved);
        assert_eq!(hint.acknowledge_to(tick(clock + 1), Region::Pal), Ok(4));
        assert!(!hint.interrupts().horizontal_pending());
        let mut pal = device(
            Region::Pal,
            if height == 240 { 0x2c } else { 0x24 },
            width,
            0,
        );
        let mut ntsc = device(Region::NtscUs, 0x24, width, 0);
        assert_eq!(pal.read_status(Region::Pal).unwrap() & 0x89, 0x09);
        assert_eq!(ntsc.read_status(Region::NtscUs).unwrap() & 0x89, 0x08);
        for field in 0..2 {
            let edge = field * PAL_FIELD + height * LINE + vint;
            assert_eq!(pal.acknowledge_to(tick(edge - 1), Region::Pal), Ok(4));
            assert_eq!(pal.read_status(Region::Pal).unwrap() & 0x89, 0x09);
            let saved = pal.clone();
            assert_eq!(
                pal.acknowledge_to(tick(edge), Region::Pal),
                Err(VdpScheduleError::CoincidentAcknowledge(tick(edge)))
            );
            assert_eq!(pal, saved);
            pal.advance_to(tick(edge), Region::Pal).unwrap();
            assert_eq!(pal.read_status(Region::Pal).unwrap() & 0x89, 0x89);
            assert_eq!(pal.read_status(Region::Pal).unwrap() & 0x89, 0x89);
            assert_eq!(pal.acknowledge_to(tick(edge + 1), Region::Pal), Ok(6));
            assert_eq!(pal.read_status(Region::Pal).unwrap() & 0x89, 0x09);
        }
        let (jump_line, low_end, high_start) = if height == 240 {
            (266, 0x10a, 0x1d2)
        } else {
            (258, 0x102, 0x1ca)
        };
        for (at, vertical) in [
            (jump_line * LINE + clock - 1, low_end),
            (jump_line * LINE + clock, high_start),
            (312 * LINE + clock, 0),
        ] {
            let mut candidate = device(Region::Pal, if height == 240 { 0x0c } else { 4 }, width, 0);
            candidate.advance_to(tick(at), Region::Pal).unwrap();
            assert_eq!(
                candidate.read_hv(Region::Pal).unwrap() >> 8,
                vertical & 0xff
            );
            assert_eq!(candidate.read_status(Region::Pal).unwrap() & 1, 1);
        }
    }
}

pub fn memory_admission_and_horizontal_slots() {
    memory_calendar(224);
}

fn memory_calendar(height: u64) {
    let timing = VdpVerticalTiming::at_counters_zero(tick(0));
    for width in [0, 0x81] {
        let active = known(if height == 240 { 0x4c } else { 0x44 }, width, 0);
        let blank = known(if height == 240 { 0x0c } else { 4 }, width, 0);
        for field in 0..2 {
            for line in [height + 1, 258, 259, 261, 262, 309] {
                for phase in 0..LINE {
                    let at = tick(field * PAL_FIELD + line * LINE + phase);
                    assert_eq!(
                        timing.is_memory_slot(at, Region::Pal, &active),
                        timing.is_memory_slot(at, Region::Pal, &blank)
                    );
                    let expected = timing.next_memory_slot(at, Region::Pal, &blank).unwrap();
                    if expected.get() / LINE % PAL_LINES < 310 {
                        assert_eq!(
                            timing.next_memory_slot(at, Region::Pal, &active),
                            Ok(expected)
                        );
                    } else {
                        assert_eq!(
                            timing.next_memory_slot(at, Region::Pal, &active),
                            Err(VdpTimingError::UnqualifiedMemoryInterval(expected))
                        );
                    }
                }
            }
            for line in [height - 2, height - 1, height, 310, 311, 312] {
                let at = tick(field * PAL_FIELD + line * LINE);
                assert_eq!(
                    timing.is_memory_slot(at, Region::Pal, &active),
                    Err(VdpTimingError::UnqualifiedMemoryInterval(at))
                );
                assert_eq!(
                    timing.next_memory_slot(at, Region::Pal, &active),
                    Err(VdpTimingError::UnqualifiedMemoryInterval(at))
                );
                assert!(timing.next_memory_slot(at, Region::Pal, &blank).is_ok());
            }
        }
        let ntsc_active = known(0x44, width, 0);
        for line in [0, 1, height - 3] {
            for phase in 0..LINE {
                let at = tick(line * LINE + phase);
                assert_eq!(
                    timing.is_memory_slot(at, Region::Pal, &active),
                    timing.is_memory_slot(tick(phase), Region::NtscUs, &ntsc_active)
                );
                let expected = timing
                    .next_memory_slot(tick(phase), Region::NtscUs, &ntsc_active)
                    .unwrap()
                    .get()
                    + line * LINE;
                assert_eq!(
                    timing.next_memory_slot(at, Region::Pal, &active),
                    if expected / LINE < height - 2 {
                        Ok(tick(expected))
                    } else {
                        Err(VdpTimingError::UnqualifiedMemoryInterval(tick(expected)))
                    }
                );
            }
        }
    }
}

pub fn long_skip_and_ntsc_regression() {
    long_skip(224);
}

fn long_skip(height: u64) {
    for width in [0, 0x81] {
        let mut pal = device(
            Region::Pal,
            if height == 240 { 0x2c } else { 0x24 },
            width,
            7,
        );
        let mut split = pal.clone();
        let horizon = 1_000_003 * PAL_FIELD + 2661;
        pal.advance_to(tick(horizon), Region::Pal).unwrap();
        for at in [896040, PAL_FIELD - 1, PAL_FIELD, 2 * PAL_FIELD, horizon] {
            split.advance_to(tick(at), Region::Pal).unwrap();
        }
        assert_eq!(pal, split);
        assert!(pal.interrupts().vertical_pending());
        assert_eq!(pal.interrupts().line_counter(), 5);
        assert_eq!(pal.read_hv(Region::Pal).unwrap() >> 8, 1);
        let mut ntsc = device(Region::NtscUs, 4, width, 7);
        let mut japan = device(Region::NtscJapan, 4, width, 7);
        let mut pal = device(Region::Pal, if height == 240 { 0x0c } else { 4 }, width, 7);
        for at in [765320, 802940, 891860, 895280, 896040, 1_792_080] {
            ntsc.advance_to(tick(at), Region::NtscUs).unwrap();
            japan.advance_to(tick(at), Region::NtscJapan).unwrap();
            pal.advance_to(tick(at), Region::Pal).unwrap();
            assert_eq!(ntsc, japan);
            assert_eq!(ntsc.read_status(Region::NtscUs).unwrap() & 1, 0);
            assert_eq!(pal.read_status(Region::Pal).unwrap() & 1, 1);
            if at == 896040 {
                assert_eq!(ntsc.read_hv(Region::NtscUs).unwrap() >> 8, 0);
                assert_eq!(
                    pal.read_hv(Region::Pal).unwrap() >> 8,
                    if height == 240 { 0x06 } else { 0xcd }
                );
            }
        }
        for reload in [239, 240, 241] {
            let mut irq = device(
                Region::Pal,
                if height == 240 { 0x0c } else { 4 },
                width,
                reload,
            );
            irq.advance_to(tick(PAL_FIELD), Region::Pal).unwrap();
            assert_eq!(irq.interrupts().line_counter(), reload - 1);
            assert_eq!(irq.acknowledge_to(tick(PAL_FIELD + 1), Region::Pal), Ok(4));
            assert!(!irq.interrupts().horizontal_pending());
            irq.advance_to(tick(horizon), Region::Pal).unwrap();
            assert_eq!(
                irq.interrupts().horizontal_pending(),
                u64::from(reload) <= height
            );
            assert_eq!(irq.interrupts().line_counter(), reload - 2);
        }
    }
}

pub fn v30_counter_interrupt_and_memory_edges() {
    vertical_edges(240);
    interrupt_calendar(240);
    acknowledge_status(240);
    memory_calendar(240);
    long_skip(240);
}

pub fn unsupported_modes_and_overflow() {
    for width in [0, 0x81] {
        let registers = known(0x4c, width, 0);
        let timing = VdpVerticalTiming::at_counters_zero(tick(137));
        let error = VdpTimingError::UnsupportedMode {
            index: 1,
            value: 0x4c,
        };
        for region in [Region::NtscUs, Region::NtscJapan] {
            assert_eq!(
                timing.v_counter_at(tick(137), region, &registers),
                Err(error)
            );
        }
        assert_eq!(
            timing.v_counter_at(tick(137), Region::Pal, &registers),
            Ok(0)
        );
        let registers = known(0x44, width, 0);
        for (distance, active) in [
            (223 * LINE + if width == 0 { 2660 } else { 2640 }, true),
            (311 * LINE + if width == 0 { 2660 } else { 2640 }, false),
        ] {
            let epoch = u64::MAX - distance;
            let timing = VdpVerticalTiming::at_counters_zero(tick(epoch));
            assert_eq!(
                timing.next_vblank_edge(tick(u64::MAX - 1), Region::Pal, &registers),
                Ok(VdpVBlankEdge {
                    at: tick(u64::MAX),
                    active
                })
            );
            assert_eq!(
                timing.next_vblank_edge(tick(u64::MAX), Region::Pal, &registers),
                Err(VdpTimingError::ClockOverflow)
            );
        }
        assert_eq!(
            timing.v_counter_at(tick(136), Region::Pal, &registers),
            Err(VdpTimingError::BeforeEpoch {
                epoch: tick(137),
                requested: tick(136)
            })
        );
    }
}
