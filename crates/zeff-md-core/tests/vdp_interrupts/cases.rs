use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    Region, VdpInterruptClock, VdpInterruptEdge, VdpInterruptError, VdpInterrupts,
    VdpRegisterPipeline, VdpTimingError, VdpVerticalTiming,
};

type Advance = Result<Option<VdpInterruptEdge>, VdpInterruptError>;

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}
fn edge(at: u64, clock: VdpInterruptClock) -> VdpInterruptEdge {
    VdpInterruptEdge {
        at: tick(at),
        clock,
    }
}
fn publish(registers: &mut VdpRegisterPipeline, index: u8, value: u8) {
    registers
        .advance_to(tick(registers.now().get() + 1))
        .unwrap();
    let write = registers.queue_register(index, value).unwrap();
    assert_eq!(registers.advance_to(write.publish_at), Ok(Some(write)));
}
fn known(r0: u8, r1: u8, reload: u8) -> VdpRegisterPipeline {
    let mut registers = VdpRegisterPipeline::at_h32_edge(tick(0));
    for (index, value) in [(0, r0), (1, r1), (12, 0), (10, reload)] {
        publish(&mut registers, index, value);
    }
    registers
}
fn events(epoch: u64, cycles: u64) -> Vec<VdpInterruptEdge> {
    let mut result = Vec::new();
    for line in 0..262 * cycles {
        if line % 262 == 224 {
            result.push(edge(epoch + line * 3420 + 10, VdpInterruptClock::Vertical));
        }
        result.push(edge(
            epoch + line * 3420 + 2660,
            VdpInterruptClock::HorizontalCounter,
        ));
    }
    result
}
fn reach(state: &mut VdpInterrupts, at: u64, registers: &VdpRegisterPipeline) -> usize {
    let mut count = 0;
    while state
        .advance_to(tick(at), Region::NtscUs, registers)
        .unwrap()
        .is_some()
    {
        count += 1;
    }
    assert_eq!(state.now(), tick(at));
    count
}

pub fn event_calendar_with(
    mut query: impl FnMut(
        &VdpVerticalTiming,
        MasterTicks,
        Region,
        &VdpRegisterPipeline,
    ) -> Result<VdpInterruptEdge, VdpTimingError>,
) {
    let registers = known(0, 0x44, 0);
    for epoch in [1000, 1007] {
        let timing = VdpVerticalTiming::at_counters_zero(tick(epoch));
        let saved = timing;
        let expected = events(epoch, 3);
        for region in [Region::NtscUs, Region::NtscJapan] {
            for current in expected.iter().take(526) {
                for at in [current.at.get() - 1, current.at.get(), current.at.get() + 1] {
                    let next = expected.iter().find(|event| event.at.get() > at).unwrap();
                    assert_eq!(query(&timing, tick(at), region, &registers), Ok(*next));
                }
            }
            for line in [223, 224, 261] {
                for phase in 0..3420 {
                    let at = epoch + line * 3420 + phase;
                    let next = expected.iter().find(|event| event.at.get() > at).unwrap();
                    assert_eq!(query(&timing, tick(at), region, &registers), Ok(*next));
                }
            }
        }
        assert_eq!(timing, saved);
    }
    for (distance, clock) in [
        (2660, VdpInterruptClock::HorizontalCounter),
        (766090, VdpInterruptClock::Vertical),
    ] {
        let timing = VdpVerticalTiming::at_counters_zero(tick(u64::MAX - distance));
        assert_eq!(
            query(&timing, tick(u64::MAX - 1), Region::NtscUs, &registers),
            Ok(edge(u64::MAX, clock))
        );
        assert_eq!(
            query(&timing, tick(u64::MAX), Region::NtscUs, &registers),
            Err(VdpTimingError::ClockOverflow)
        );
    }
    let timing = VdpVerticalTiming::at_counters_zero(tick(1000));
    assert_eq!(
        query(&timing, tick(999), Region::NtscUs, &registers),
        Err(VdpTimingError::BeforeEpoch {
            epoch: tick(1000),
            requested: tick(999)
        })
    );
}
pub fn event_calendar() {
    event_calendar_with(|timing, at, region, registers| {
        timing.next_interrupt_edge(at, region, registers)
    });
}

pub fn line_counter_partition_with(
    mut advance: impl FnMut(&mut VdpInterrupts, MasterTicks, Region, &VdpRegisterPipeline) -> Advance,
) {
    for reload in 0..=255 {
        let registers = known(0, 0x44, reload);
        for seed in 0..=255 {
            let mut state = VdpInterrupts::at_counters_zero(tick(1000), seed, false, false);
            assert_eq!(state.line_counter(), seed);
            assert_eq!(
                advance(&mut state, tick(3659), Region::NtscUs, &registers),
                Ok(None)
            );
            assert_eq!(state.line_counter(), seed);
            assert_eq!(
                advance(&mut state, tick(3660), Region::NtscUs, &registers),
                Ok(Some(edge(3660, VdpInterruptClock::HorizontalCounter)))
            );
            assert_eq!(
                state.line_counter(),
                if seed == 0 { reload } else { seed - 1 }
            );
            assert_eq!(state.horizontal_pending(), seed == 0);
            assert!(!state.vertical_pending());
            assert_eq!(state.interrupt_level(&registers), Ok(0));
        }
    }
}
pub fn line_counter_partition() {
    line_counter_partition_with(|state, at, region, registers| {
        state.advance_to(at, region, registers)
    });
}

pub fn line_counter_frames() {
    let expected = events(1000, 2);
    for reload in [0, 1, 2, 7, 223, 224, 255] {
        let registers = known(0, 0x44, reload);
        let mut state = VdpInterrupts::at_counters_zero(tick(1000), reload, false, false);
        let mut counter = u16::from(reload);
        let mut h_pending = false;
        let mut v_pending = false;
        let mut horizontal_ordinal = 0;
        for event in &expected {
            assert_eq!(
                state.advance_to(event.at, Region::NtscUs, &registers),
                Ok(Some(*event))
            );
            if event.clock == VdpInterruptClock::Vertical {
                v_pending = true;
            } else {
                horizontal_ordinal += 1;
                let incoming = horizontal_ordinal % 262;
                if incoming <= 224 {
                    if counter == 0 {
                        h_pending = true;
                        counter = u16::from(reload);
                    } else {
                        counter -= 1;
                    }
                } else {
                    counter = u16::from(reload);
                }
            }
            assert_eq!(state.line_counter(), counter as u8);
            assert_eq!(state.horizontal_pending(), h_pending);
            assert_eq!(state.vertical_pending(), v_pending);
            assert_eq!(state.status_bits(), if v_pending { 0x80 } else { 0 });
            let saved = state;
            for _ in 0..2 {
                assert_eq!(state.status_bits(), saved.status_bits());
            }
            assert_eq!(state, saved);
            assert_eq!(
                state.advance_to(event.at, Region::NtscUs, &registers),
                Ok(None)
            );
        }
        assert_eq!(state.now(), tick(1792320));
        assert_eq!(
            state.line_counter(),
            if reload == 0 { 0 } else { reload - 1 }
        );
        assert_eq!(state.vertical_pending(), v_pending);
    }
}

pub fn pending_enable_acknowledge_with(
    mut acknowledge: impl FnMut(
        &mut VdpInterrupts,
        &VdpRegisterPipeline,
    ) -> Result<u8, VdpInterruptError>,
) {
    for h_pending in [false, true] {
        for v_pending in [false, true] {
            for h_enabled in [false, true] {
                for v_enabled in [false, true] {
                    let registers = known(
                        if h_enabled { 0x10 } else { 0 },
                        if v_enabled { 0x64 } else { 0x44 },
                        3,
                    );
                    let mut state =
                        VdpInterrupts::at_counters_zero(tick(1000), 17, h_pending, v_pending);
                    let level = if v_pending && v_enabled {
                        6
                    } else if h_pending && h_enabled {
                        4
                    } else {
                        0
                    };
                    assert_eq!(state.interrupt_level(&registers), Ok(level));
                    assert_eq!(acknowledge(&mut state, &registers), Ok(level));
                    assert_eq!(state.horizontal_pending(), h_pending && level != 4);
                    assert_eq!(state.vertical_pending(), v_pending && level != 6);
                    assert_eq!(state.line_counter(), 17);
                    assert_eq!(state.now(), tick(1000));
                    let next = if level == 6 && h_enabled && h_pending {
                        4
                    } else {
                        0
                    };
                    assert_eq!(acknowledge(&mut state, &registers), Ok(next));
                    assert_eq!(acknowledge(&mut state, &registers), Ok(0));
                }
            }
        }
    }
    for (index, payloads) in [(0, 256), (1, 256)] {
        for value in 0..payloads {
            let value = value as u8;
            if index == 1 && value & 4 == 0 {
                continue;
            }
            let registers = if index == 0 {
                known(value, 0x44, 0)
            } else {
                known(0x10, value, 0)
            };
            let mut state = VdpInterrupts::at_counters_zero(tick(1000), 9, true, true);
            let level = if index == 1 && value & 0x20 != 0 {
                6
            } else if index == 1 || value & 0x10 != 0 {
                4
            } else {
                0
            };
            assert_eq!(acknowledge(&mut state, &registers), Ok(level));
            assert_eq!(state.status_bits(), if level == 6 { 0 } else { 0x80 });
        }
    }
}
pub fn pending_enable_acknowledge() {
    pending_enable_acknowledge_with(|state, registers| state.complete_acknowledge(registers));
}

pub fn reload_publication() {
    let mut registers = known(0, 0x44, 2);
    let mut state = VdpInterrupts::at_counters_zero(tick(1000), 1, true, true);
    assert_eq!(state.interrupt_level(&registers), Ok(0));
    publish(&mut registers, 10, 7);
    assert_eq!(state.line_counter(), 1);
    assert_eq!(
        state
            .advance_to(tick(3660), Region::NtscUs, &registers)
            .unwrap()
            .unwrap()
            .clock,
        VdpInterruptClock::HorizontalCounter
    );
    assert_eq!(state.line_counter(), 0);
    assert_eq!(
        state
            .advance_to(tick(7080), Region::NtscUs, &registers)
            .unwrap()
            .unwrap()
            .clock,
        VdpInterruptClock::HorizontalCounter
    );
    assert_eq!(state.line_counter(), 7);
    registers
        .advance_to(tick(registers.now().get() + 1))
        .unwrap();
    let write = registers.queue_register(0, 0x10).unwrap();
    assert_eq!(state.interrupt_level(&registers), Ok(0));
    registers.advance_to(write.publish_at).unwrap();
    assert_eq!(state.interrupt_level(&registers), Ok(4));
    publish(&mut registers, 1, 0x64);
    assert_eq!(state.interrupt_level(&registers), Ok(6));
    publish(&mut registers, 1, 0x44);
    assert_eq!(state.interrupt_level(&registers), Ok(4));
    assert_eq!(state.status_bits(), 0x80);
    publish(&mut registers, 0, 0);
    assert_eq!(state.interrupt_level(&registers), Ok(0));
    assert!(state.horizontal_pending());
    assert!(state.vertical_pending());
}

pub fn fault_atomicity_with(
    mut advance: impl FnMut(&mut VdpInterrupts, MasterTicks, Region, &VdpRegisterPipeline) -> Advance,
    mut acknowledge: impl FnMut(
        &mut VdpInterrupts,
        &VdpRegisterPipeline,
    ) -> Result<u8, VdpInterruptError>,
) {
    let mut state = VdpInterrupts::at_counters_zero(tick(1000), 19, true, true);
    let initial = state;
    let good = known(0x10, 0x64, 3);
    let mut missing = VdpRegisterPipeline::at_h32_edge(tick(0));
    for index in [1, 12, 10] {
        assert_eq!(
            advance(&mut state, tick(4000), Region::NtscUs, &missing),
            Err(VdpInterruptError::Timing(VdpTimingError::UnknownRegister(
                index
            )))
        );
        assert_eq!(state, initial);
        publish(&mut missing, index, if index == 1 { 0x44 } else { 0 });
    }
    let mut pal = state;
    assert_eq!(
        advance(&mut pal, tick(4000), Region::Pal, &good),
        Ok(Some(edge(3660, VdpInterruptClock::HorizontalCounter)))
    );
    assert_eq!(pal.line_counter(), 18);
    assert_eq!(state, initial);
    for (index, value) in [(1, 4), (1, 0x4c), (12, 2), (12, 4)] {
        let mut invalid = good;
        publish(&mut invalid, index, value);
        assert_eq!(
            advance(&mut state, tick(4000), Region::NtscUs, &invalid),
            Err(VdpInterruptError::Timing(VdpTimingError::UnsupportedMode {
                index,
                value
            }))
        );
        assert_eq!(state, initial);
    }
    for (index, value) in [(1, 0x24), (12, 0x10), (10, 7)] {
        let mut pending = good;
        pending.advance_to(tick(pending.now().get() + 1)).unwrap();
        pending.queue_register(index, value).unwrap();
        let error = if index == 10 {
            VdpInterruptError::PendingReload
        } else {
            VdpInterruptError::Timing(VdpTimingError::PendingModeChange)
        };
        assert_eq!(
            advance(&mut state, tick(4000), Region::NtscUs, &pending),
            Err(error)
        );
        assert_eq!(state, initial);
    }
    assert_eq!(
        advance(&mut state, tick(999), Region::NtscUs, &good),
        Err(VdpInterruptError::TimeReversal {
            current: tick(1000),
            requested: tick(999)
        })
    );
    assert_eq!(state, initial);
    for distance in [0, 2660, 766090] {
        let mut last = VdpInterrupts::at_counters_zero(tick(u64::MAX - distance), 0, false, false);
        let mut count = 0;
        while advance(&mut last, tick(u64::MAX), Region::NtscUs, &good)
            .unwrap()
            .is_some()
        {
            count += 1;
        }
        assert_eq!(last.now(), tick(u64::MAX));
        assert_eq!(
            count,
            match distance {
                0 => 0,
                2660 => 1,
                _ => 225,
            }
        );
        assert_eq!(last.vertical_pending(), distance == 766090);
    }
    let mut missing = VdpRegisterPipeline::at_h32_edge(tick(0));
    for index in [0, 1] {
        let error = Err(VdpInterruptError::Timing(VdpTimingError::UnknownRegister(
            index,
        )));
        assert_eq!(state.interrupt_level(&missing), error);
        assert_eq!(acknowledge(&mut state, &missing), error);
        assert_eq!(state, initial);
        publish(&mut missing, index, if index == 1 { 0x44 } else { 0 });
    }
}
pub fn fault_atomicity() {
    fault_atomicity_with(
        |state, at, region, registers| state.advance_to(at, region, registers),
        |state, registers| state.complete_acknowledge(registers),
    );
}

pub fn event_order_and_copy() {
    let registers = known(0x10, 0x64, 255);
    let timing = VdpVerticalTiming::at_counters_zero(tick(1000));
    let mut state = VdpInterrupts::at_counters_zero(tick(1000), 255, true, false);
    reach(&mut state, 766319, &registers);
    assert_eq!(
        timing.vblank_flag_at(tick(766319), Region::NtscUs, &registers),
        Ok(false)
    );
    assert_eq!(state.status_bits(), 0);
    assert_eq!(
        state.advance_to(tick(766320), Region::NtscUs, &registers),
        Ok(Some(edge(766320, VdpInterruptClock::HorizontalCounter)))
    );
    assert_eq!(
        timing.vblank_flag_at(tick(766320), Region::NtscUs, &registers),
        Ok(true)
    );
    assert_eq!(state.status_bits(), 0);
    reach(&mut state, 767089, &registers);
    let sampled = state.interrupt_level(&registers).unwrap();
    assert_eq!(sampled, 4);
    let mut before = state;
    assert_eq!(before.complete_acknowledge(&registers), Ok(4));
    assert_eq!(
        state.advance_to(tick(767090), Region::NtscUs, &registers),
        Ok(Some(edge(767090, VdpInterruptClock::Vertical)))
    );
    assert_eq!(state.interrupt_level(&registers), Ok(6));
    assert_eq!(state.complete_acknowledge(&registers), Ok(6));
    assert!(state.horizontal_pending());
    assert_eq!(state.interrupt_level(&registers), Ok(sampled));
    assert_eq!(
        before
            .advance_to(tick(767090), Region::NtscUs, &registers)
            .unwrap()
            .unwrap()
            .clock,
        VdpInterruptClock::Vertical
    );
    assert!(!before.horizontal_pending());
    assert!(before.vertical_pending());
    let mut coarse = VdpInterrupts::at_counters_zero(tick(1000), 7, false, false);
    let mut fine = coarse;
    assert_eq!(reach(&mut coarse, 1793080, &registers), 526);
    for event in events(1000, 2) {
        assert_eq!(
            fine.advance_to(tick(event.at.get() - 1), Region::NtscUs, &registers),
            Ok(None)
        );
        assert_eq!(
            fine.advance_to(event.at, Region::NtscUs, &registers),
            Ok(Some(event))
        );
        assert_eq!(
            fine.advance_to(event.at, Region::NtscUs, &registers),
            Ok(None)
        );
    }
    assert_eq!(
        fine.advance_to(tick(1793080), Region::NtscUs, &registers),
        Ok(None)
    );
    assert_eq!(fine, coarse);
}
