pub mod cpu_cases;
pub mod support;
pub use cpu_cases::{actual_cpu_acknowledge, actual_cpu_faults, actual_cpu_publications};

use support::*;
use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    Region, VdpEvent, VdpInterruptClock, VdpInterruptError, VdpInterrupts, VdpRegisterPipeline,
    VdpScheduleError, VdpTimingError,
};

type Advance = Result<Option<VdpEvent>, VdpScheduleError>;

pub fn ordered_publications_with(
    mut advance: impl FnMut(
        &mut VdpInterrupts,
        MasterTicks,
        Region,
        &mut VdpRegisterPipeline,
    ) -> Advance,
) {
    let (mut state, mut registers) = known(1000, 2, true, false);
    drain(&mut state, &mut registers, 1001);
    let write = registers.queue_register(10, 7).unwrap();
    assert_eq!(write.publish_at, tick(1020));
    assert_eq!(
        advance(&mut state, tick(1019), Region::NtscUs, &mut registers),
        Ok(None)
    );
    assert_eq!(registers.registers()[10], Some(2));
    assert_eq!(registers.pending(), Some(write));
    assert_eq!(
        advance(&mut state, tick(20000), Region::NtscUs, &mut registers),
        Ok(Some(VdpEvent::Register(write)))
    );
    assert_eq!(state.now(), tick(1020));
    assert_eq!(registers.now(), tick(1020));
    assert_eq!(state.line_counter(), 2);
    assert_eq!(registers.registers()[10], Some(7));
    for (at, count) in [(3660, 1), (7080, 0), (10500, 7)] {
        let event = advance(&mut state, tick(20000), Region::NtscUs, &mut registers)
            .unwrap()
            .unwrap();
        assert_eq!(event.at(), tick(at));
        assert!(
            matches!(event, VdpEvent::Interrupt(edge) if edge.clock == VdpInterruptClock::HorizontalCounter)
        );
        assert_eq!(state.line_counter(), count);
    }
    let (mut state, mut registers) = known(1000, 255, true, false);
    publish(&mut registers, 1, 0x44);
    registers.advance_to(tick(1040)).unwrap();
    state
        .advance_to(tick(1040), Region::NtscUs, &registers)
        .unwrap();
    drain(&mut state, &mut registers, 767081);
    let write = registers.queue_register(1, 0x64).unwrap();
    assert_eq!(write.publish_at, tick(767100));
    assert_eq!(
        advance(&mut state, tick(767101), Region::NtscUs, &mut registers)
            .unwrap()
            .unwrap()
            .at(),
        tick(767090)
    );
    assert_eq!(state.status_bits(), 0x80);
    assert_eq!(state.interrupt_level(&registers), Ok(4));
    assert_eq!(registers.pending(), Some(write));
    assert_eq!(
        advance(&mut state, tick(767101), Region::NtscUs, &mut registers),
        Ok(Some(VdpEvent::Register(write)))
    );
    assert_eq!(state.interrupt_level(&registers), Ok(6));
    assert_eq!(
        advance(&mut state, tick(767101), Region::NtscUs, &mut registers),
        Ok(None)
    );
    assert_eq!(registers.pending(), None);
}
pub fn ordered_publications() {
    ordered_publications_with(|state, at, region, registers| {
        state.advance_registers_to(at, region, registers)
    });
}

pub fn phase_and_fault_guards_with(
    mut advance: impl FnMut(
        &mut VdpInterrupts,
        MasterTicks,
        Region,
        &mut VdpRegisterPipeline,
    ) -> Advance,
) {
    let (mut state, mut registers) = known(1000, 3, true, true);
    registers.advance_to(tick(1001)).unwrap();
    let saved = (state, registers);
    assert_eq!(
        advance(&mut state, tick(1100), Region::NtscUs, &mut registers),
        Err(VdpScheduleError::ClockMismatch {
            interrupt: tick(1000),
            registers: tick(1001)
        })
    );
    assert_eq!((state, registers), saved);
    let (_, mut registers) = known(1001, 3, true, true);
    state
        .advance_to(tick(1001), Region::NtscUs, &registers)
        .unwrap();
    let saved = (state, registers);
    assert_eq!(
        advance(&mut state, tick(1100), Region::NtscUs, &mut registers),
        Err(VdpScheduleError::PhaseMismatch)
    );
    assert_eq!((state, registers), saved);
    {
        let (at, region, error) = (
            999,
            Region::NtscUs,
            VdpScheduleError::Interrupt(VdpInterruptError::TimeReversal {
                current: tick(1000),
                requested: tick(999),
            }),
        );
        let (mut state, mut registers) = known(1000, 3, true, true);
        let saved = (state, registers);
        assert_eq!(
            advance(&mut state, tick(at), region, &mut registers),
            Err(error)
        );
        assert_eq!((state, registers), saved);
    }
    let (mut pal, mut pal_registers) = known(1000, 3, true, true);
    let (mut ntsc, mut ntsc_registers) = (pal, pal_registers);
    assert_eq!(
        advance(&mut pal, tick(1100), Region::Pal, &mut pal_registers),
        advance(&mut ntsc, tick(1100), Region::NtscUs, &mut ntsc_registers)
    );
    assert_eq!((pal, pal_registers), (ntsc, ntsc_registers));
    for (index, value) in [(1, 4), (1, 0x4c), (12, 2), (12, 4)] {
        let (mut state, mut registers) = known(1000, 0, false, false);
        drain(&mut state, &mut registers, 3661);
        assert!(state.horizontal_pending());
        registers.queue_register(index, value).unwrap();
        let saved = (state, registers);
        assert_eq!(
            advance(&mut state, tick(4000), Region::NtscUs, &mut registers),
            Err(VdpScheduleError::Interrupt(VdpInterruptError::Timing(
                VdpTimingError::UnsupportedMode { index, value }
            )))
        );
        assert_eq!((state, registers), saved);
    }
    for missing in [1, 12, 10] {
        let mut registers = VdpRegisterPipeline::at_h32_edge(tick(920));
        for (index, value) in [(1, 0x64), (12, 0)] {
            if index == missing {
                break;
            }
            publish(&mut registers, index, value);
        }
        registers.advance_to(tick(1000)).unwrap();
        let mut state = VdpInterrupts::at_counters_zero(tick(1000), 3, true, true);
        let saved = (state, registers);
        assert_eq!(
            advance(&mut state, tick(1100), Region::NtscUs, &mut registers),
            Err(VdpScheduleError::Interrupt(VdpInterruptError::Timing(
                VdpTimingError::UnknownRegister(missing)
            )))
        );
        assert_eq!((state, registers), saved);
    }
    for distance in [0, 1000, 2660] {
        let epoch = u64::MAX - distance;
        let (mut state, mut registers) = known(epoch, 0, false, false);
        if distance == 1000 {
            drain(&mut state, &mut registers, u64::MAX - 1);
            let write = registers.queue_register(0, 0).unwrap();
            assert_eq!(write.publish_at, tick(u64::MAX));
            assert_eq!(
                advance(&mut state, tick(u64::MAX), Region::NtscUs, &mut registers),
                Ok(Some(VdpEvent::Register(write)))
            );
        } else if distance == 2660 {
            assert!(
                advance(&mut state, tick(u64::MAX), Region::NtscUs, &mut registers)
                    .unwrap()
                    .is_some()
            );
            assert!(state.horizontal_pending());
        }
        assert_eq!(
            advance(&mut state, tick(u64::MAX), Region::NtscUs, &mut registers),
            Ok(None)
        );
        assert_eq!(state.now(), tick(u64::MAX));
        assert_eq!(registers.now(), tick(u64::MAX));
    }
}
pub fn phase_and_fault_guards() {
    phase_and_fault_guards_with(|state, at, region, registers| {
        state.advance_registers_to(at, region, registers)
    });
}

pub fn collision_guards_with(
    mut advance: impl FnMut(
        &mut VdpInterrupts,
        MasterTicks,
        Region,
        &mut VdpRegisterPipeline,
    ) -> Advance,
) {
    for index in 0..24 {
        let (mut state, mut registers) = known(1000, 0, false, false);
        drain(&mut state, &mut registers, 3659);
        registers
            .queue_register(index, if index == 1 { 0x64 } else { 0 })
            .unwrap();
        assert_eq!(
            advance(&mut state, tick(3659), Region::NtscUs, &mut registers),
            Ok(None)
        );
        let saved = (state, registers);
        for at in [3660, 3661, 4000] {
            assert_eq!(
                advance(&mut state, tick(at), Region::NtscUs, &mut registers),
                Err(VdpScheduleError::CoincidentPublication(tick(3660)))
            );
            assert_eq!((state, registers), saved);
        }
    }
}
pub fn collision_guards() {
    collision_guards_with(|state, at, region, registers| {
        state.advance_registers_to(at, region, registers)
    });
}

pub fn acknowledge_ordering_with(
    mut acknowledge: impl FnMut(
        &mut VdpInterrupts,
        MasterTicks,
        Region,
        &mut VdpRegisterPipeline,
    ) -> Result<u8, VdpScheduleError>,
) {
    for (at, cleared) in [(767089, 4), (767091, 6), (767108, 6)] {
        let (mut state, mut registers) = known(1000, 255, true, false);
        drain(&mut state, &mut registers, 767080);
        assert_eq!(
            acknowledge(&mut state, tick(at), Region::NtscUs, &mut registers),
            Ok(cleared)
        );
        assert_eq!(state.horizontal_pending(), cleared != 4);
        assert!(!state.vertical_pending());
        assert_eq!(state.now(), tick(at));
        assert_eq!(registers.now(), tick(at));
    }
    for at in [767080, 767079, 767090] {
        let (mut state, mut registers) = known(1000, 255, true, false);
        drain(&mut state, &mut registers, 767080);
        let saved = (state, registers);
        let error = match at {
            767080 => VdpScheduleError::EmptyAcknowledgeInterval,
            767079 => VdpScheduleError::Interrupt(VdpInterruptError::TimeReversal {
                current: tick(767080),
                requested: tick(767079),
            }),
            _ => VdpScheduleError::CoincidentAcknowledge(tick(at)),
        };
        assert_eq!(
            acknowledge(&mut state, tick(at), Region::NtscUs, &mut registers),
            Err(error)
        );
        assert_eq!((state, registers), saved);
    }
    for at in [1020, 3660] {
        let (mut state, mut registers) = known(1000, 0, true, true);
        drain(&mut state, &mut registers, 1001);
        if at == 1020 {
            registers.queue_register(0, 0).unwrap();
        }
        let saved = (state, registers);
        assert_eq!(
            acknowledge(&mut state, tick(at), Region::NtscUs, &mut registers),
            Err(VdpScheduleError::CoincidentAcknowledge(tick(at)))
        );
        assert_eq!((state, registers), saved);
    }
    let (mut state, mut registers) = known(1000, 255, true, false);
    drain(&mut state, &mut registers, 767090);
    let saved = (state, registers);
    assert_eq!(
        acknowledge(&mut state, tick(767090), Region::NtscUs, &mut registers),
        Err(VdpScheduleError::EmptyAcknowledgeInterval)
    );
    assert_eq!((state, registers), saved);

    let mut registers = VdpRegisterPipeline::at_h32_edge(tick(920));
    for (index, value) in [(1, 0x64), (12, 0), (10, 255)] {
        publish(&mut registers, index, value);
    }
    registers.advance_to(tick(1000)).unwrap();
    let mut state = VdpInterrupts::at_counters_zero(tick(1000), 255, true, false);
    drain(&mut state, &mut registers, 767080);
    let saved = (state, registers);
    assert_eq!(
        acknowledge(&mut state, tick(767108), Region::NtscUs, &mut registers),
        Err(VdpScheduleError::Interrupt(VdpInterruptError::Timing(
            VdpTimingError::UnknownRegister(0)
        )))
    );
    assert_eq!((state, registers), saved);

    let (mut state, mut registers) = known(u64::MAX - 1000, 3, true, true);
    drain(&mut state, &mut registers, u64::MAX - 1);
    assert_eq!(
        acknowledge(&mut state, tick(u64::MAX), Region::NtscUs, &mut registers),
        Ok(6)
    );
    assert_eq!(state.now(), tick(u64::MAX));
    assert_eq!(registers.now(), state.now());
    assert!(!state.vertical_pending());
    assert!(state.horizontal_pending());
}
pub fn acknowledge_ordering() {
    acknowledge_ordering_with(|state, at, region, registers| {
        state.complete_scheduled_acknowledge(at, region, registers)
    });
}

pub fn chunk_and_copy() {
    for phase in 1..20 {
        for (index, value) in [(0, 0), (1, 0x44), (10, 7)] {
            let (mut state, mut registers) = known(1000, 17, false, false);
            drain(&mut state, &mut registers, 1000 + phase);
            let write = registers.queue_register(index, value).unwrap();
            let mut copied = (state, registers);
            assert_eq!(drain(&mut state, &mut registers, 901000), 265);
            let mut events = 0;
            for at in [
                1019, 1020, 1021, 3659, 3660, 3661, 767089, 767090, 767091, 901000,
            ] {
                events += drain(&mut copied.0, &mut copied.1, at);
            }
            assert_eq!(events, 265);
            assert_eq!((state, registers), copied);
            assert_eq!(registers.registers()[usize::from(index)], Some(value));
            assert_eq!(write.publish_at, tick(1020));
            assert_eq!(registers.pending(), None);
        }
    }
    let (mut state, mut registers) = known(1007, 0, true, true);
    assert!(
        state
            .advance_registers_to(tick(3667), Region::NtscJapan, &mut registers)
            .unwrap()
            .is_some()
    );
}
