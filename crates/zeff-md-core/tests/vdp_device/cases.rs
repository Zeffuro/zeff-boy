use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    Region, Vdp, VdpInterruptError, VdpInterrupts, VdpRegisterError, VdpRegisterPipeline,
    VdpScheduleError, VdpTimingError,
};

const INTERNAL_TICKS: u64 = 20;
const LINE_TICKS: u64 = 3420;
const FRAME_TICKS: u64 = 262 * LINE_TICKS;
const HORIZONTAL_EDGE: u64 = 2660;
const VERTICAL_EDGE: u64 = 224 * LINE_TICKS + 10;
const MODE_REGISTER: u8 = 1;
const SIZE_REGISTER: u8 = 12;
const RELOAD_REGISTER: u8 = 10;
const MODE5: u8 = 4;
const DISPLAY: u8 = 0x40;
const VINT_ENABLE: u8 = 0x20;
const HINT_ENABLE: u8 = 0x10;
const EPOCH: u64 = 4 * INTERNAL_TICKS;

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}
fn known(epoch: u64, reload: u8, seed: u8) -> (VdpInterrupts, VdpRegisterPipeline) {
    let mut registers = VdpRegisterPipeline::at_h32_edge(tick(epoch - EPOCH));
    for (index, value) in [
        (0, HINT_ENABLE),
        (MODE_REGISTER, MODE5 | DISPLAY | VINT_ENABLE),
        (SIZE_REGISTER, 0),
        (RELOAD_REGISTER, reload),
    ] {
        registers
            .advance_to(tick(registers.now().get() + 1))
            .unwrap();
        let write = registers.queue_register(index, value).unwrap();
        assert_eq!(registers.advance_to(write.publish_at), Ok(Some(write)));
    }
    (
        VdpInterrupts::at_counters_zero(tick(epoch), seed, false, false),
        registers,
    )
}
fn drain(state: &mut VdpInterrupts, registers: &mut VdpRegisterPipeline, at: u64) {
    while state
        .advance_registers_to(tick(at), Region::NtscUs, registers)
        .unwrap()
        .is_some()
    {}
    assert_eq!(state.now(), registers.now());
    assert_eq!(state.now(), tick(at));
}
fn compare(device: &Vdp, state: &VdpInterrupts, registers: &VdpRegisterPipeline) {
    assert_eq!(device.interrupts(), state);
    assert_eq!(device.registers(), registers);
    assert_eq!(device.now(), registers.now());
    assert_eq!(
        device.interrupt_level(),
        state.interrupt_level(registers).unwrap()
    );
}

pub fn bootstrap_and_display_off() {
    let mut device = Vdp::bootstrap_mode5();
    assert_eq!(device, Vdp::bootstrap_mode5());
    assert_eq!(device.now(), MasterTicks::ZERO);
    assert_eq!(device.interrupts().line_counter(), 0);
    assert!(!device.interrupts().horizontal_pending());
    assert!(!device.interrupts().vertical_pending());
    assert_eq!(device.interrupt_level(), 0);
    assert_eq!(device.registers().pending(), None);
    for (index, value) in device.registers().registers().iter().enumerate() {
        assert_eq!(
            *value,
            Some(if index == usize::from(MODE_REGISTER) {
                MODE5
            } else {
                0
            })
        );
    }
    let mut strict = *device.interrupts();
    let mut registers = *device.registers();
    assert_eq!(
        strict.advance_registers_to(tick(HORIZONTAL_EDGE), Region::NtscUs, &mut registers),
        Err(VdpScheduleError::Interrupt(VdpInterruptError::Timing(
            VdpTimingError::UnsupportedMode {
                index: MODE_REGISTER,
                value: MODE5
            }
        )))
    );
    device
        .advance_to(tick(HORIZONTAL_EDGE - 1), Region::NtscUs)
        .unwrap();
    assert!(!device.interrupts().horizontal_pending());
    device
        .advance_to(tick(HORIZONTAL_EDGE), Region::NtscUs)
        .unwrap();
    assert!(device.interrupts().horizontal_pending());
    device
        .advance_to(tick(VERTICAL_EDGE), Region::NtscUs)
        .unwrap();
    assert!(device.interrupts().vertical_pending());
    assert_eq!(device.interrupt_level(), 0);
    for (index, value, level) in [(0, HINT_ENABLE, 4), (MODE_REGISTER, MODE5 | VINT_ENABLE, 6)] {
        device
            .advance_to(tick(device.now().get() + 1), Region::NtscUs)
            .unwrap();
        let write = device.queue_register(index, value, Region::NtscUs).unwrap();
        assert_eq!(device.interrupt_level(), if index == 0 { 0 } else { 4 });
        device.advance_to(write.publish_at, Region::NtscUs).unwrap();
        assert_eq!(device.interrupt_level(), level);
    }
    assert_eq!(
        device.registers().registers()[usize::from(MODE_REGISTER)],
        Some(MODE5 | VINT_ENABLE)
    );
    assert_eq!(
        device.acknowledge_to(tick(device.now().get() + 1), Region::NtscUs),
        Ok(6)
    );
    assert_eq!(
        device.acknowledge_to(tick(device.now().get() + 1), Region::NtscUs),
        Ok(4)
    );
    assert_eq!(device.interrupt_level(), 0);
}

pub fn arbitrary_counter_and_settled_cycles() {
    for reload in 0..=u8::MAX {
        for seed in [0, reload, reload.wrapping_add(73), u8::MAX] {
            let (mut state, mut registers) = known(EPOCH, reload, seed);
            let mut device = Vdp::from_known_state(state, registers, Region::NtscUs).unwrap();
            for offset in [
                HORIZONTAL_EDGE - 1,
                HORIZONTAL_EDGE,
                FRAME_TICKS,
                7 * FRAME_TICKS + VERTICAL_EDGE,
            ] {
                device
                    .advance_to(tick(EPOCH + offset), Region::NtscUs)
                    .unwrap();
                drain(&mut state, &mut registers, EPOCH + offset);
                compare(&device, &state, &registers);
            }
        }
        let (mut state, mut registers) = known(EPOCH, reload, reload.saturating_sub(1));
        let mut device = Vdp::from_known_state(state, registers, Region::NtscJapan).unwrap();
        device
            .advance_to(tick(EPOCH + 9 * FRAME_TICKS), Region::NtscJapan)
            .unwrap();
        drain(&mut state, &mut registers, EPOCH + 9 * FRAME_TICKS);
        compare(&device, &state, &registers);
        assert_eq!(state.line_counter(), reload.saturating_sub(1));
        assert_eq!(state.horizontal_pending(), reload <= 224);
        assert!(state.vertical_pending());
    }
    for reload in [0, 1, 224, 225, u8::MAX] {
        for seed in 0..=u8::MAX {
            let (mut state, mut registers) = known(EPOCH, reload, seed);
            let mut device = Vdp::from_known_state(state, registers, Region::NtscUs).unwrap();
            device
                .advance_to(tick(EPOCH + 3 * FRAME_TICKS), Region::NtscUs)
                .unwrap();
            drain(&mut state, &mut registers, EPOCH + 3 * FRAME_TICKS);
            compare(&device, &state, &registers);
        }
    }
}

pub fn pending_reload_and_chunking() {
    for reload in 0..=u8::MAX {
        let (mut state, mut registers) = known(EPOCH, reload, reload.saturating_sub(1));
        let mut device = Vdp::from_known_state(state, registers, Region::NtscUs).unwrap();
        let begin = EPOCH + FRAME_TICKS + 1;
        device.advance_to(tick(begin), Region::NtscUs).unwrap();
        drain(&mut state, &mut registers, begin);
        let value = reload.wrapping_add(97);
        let write = device
            .queue_register(RELOAD_REGISTER, value, Region::NtscUs)
            .unwrap();
        assert_eq!(registers.queue_register(RELOAD_REGISTER, value), Ok(write));
        let pending = device.clone();
        assert_eq!(
            Vdp::from_known_state(state, registers, Region::NtscUs),
            Ok(pending.clone())
        );
        for at in [
            write.publish_at.get() - 1,
            write.publish_at.get(),
            EPOCH + 8 * FRAME_TICKS + HORIZONTAL_EDGE,
        ] {
            device.advance_to(tick(at), Region::NtscUs).unwrap();
            drain(&mut state, &mut registers, at);
            compare(&device, &state, &registers);
        }
        let mut coarse = pending;
        coarse.advance_to(device.now(), Region::NtscUs).unwrap();
        assert_eq!(coarse, device);
        assert_eq!(
            device.registers().registers()[usize::from(RELOAD_REGISTER)],
            Some(value)
        );
        assert_eq!(device.registers().pending(), None);
    }
}

pub fn mode_and_clock_atomicity() {
    let mut device = Vdp::bootstrap_mode5();
    assert_eq!(
        device.queue_register(0, HINT_ENABLE, Region::NtscUs),
        Err(VdpRegisterError::CoincidentClockEdge.into())
    );
    device.advance_to(tick(1), Region::NtscUs).unwrap();
    let initial = device.clone();
    for (index, mask, expected) in [
        (MODE_REGISTER, MODE5 | 8, MODE5),
        (SIZE_REGISTER, 0x81 | 6, 0),
    ] {
        for value in 0..=u8::MAX {
            let mut candidate = initial.clone();
            let result = candidate.queue_register(index, value, Region::NtscUs);
            if value & mask == expected
                || (index == SIZE_REGISTER && matches!(value & mask, 6 | 0x81 | 0x87))
            {
                let write = result.unwrap();
                assert_eq!(
                    candidate.registers().registers(),
                    initial.registers().registers()
                );
                candidate
                    .advance_to(write.publish_at, Region::NtscUs)
                    .unwrap();
                assert_eq!(
                    candidate.registers().registers()[usize::from(index)],
                    Some(value)
                );
            } else {
                assert_eq!(
                    result,
                    Err(VdpRegisterError::UnsupportedModeChange { index, value }.into())
                );
                assert_eq!(candidate, initial);
            }
        }
    }
    assert_eq!(
        device.queue_register(24, 0, Region::NtscUs),
        Err(VdpRegisterError::UnsupportedRegister(24).into())
    );
    assert_eq!(device, initial);
    let mut pal = device.clone();
    let mut ntsc = device.clone();
    assert_eq!(
        pal.queue_register(0, HINT_ENABLE, Region::Pal),
        ntsc.queue_register(0, HINT_ENABLE, Region::NtscUs)
    );
    assert!(pal.registers().pending().is_some());
    assert_eq!(pal.advance_to(tick(100), Region::Pal), Ok(()));
    assert_eq!(ntsc.advance_to(tick(100), Region::NtscUs), Ok(()));
    assert_eq!(pal.acknowledge_to(tick(101), Region::Pal), Ok(0));
    assert_eq!(ntsc.acknowledge_to(tick(101), Region::NtscUs), Ok(0));
    assert_eq!(pal, ntsc);
    assert_eq!(device, initial);
    let write = device
        .queue_register(RELOAD_REGISTER, 7, Region::NtscUs)
        .unwrap();
    let pending = device.clone();
    assert_eq!(
        device.queue_register(0, HINT_ENABLE, Region::NtscUs),
        Err(VdpRegisterError::Busy {
            publish_at: write.publish_at
        }
        .into())
    );
    assert_eq!(device, pending);
    assert_eq!(
        device.advance_to(tick(0), Region::NtscUs),
        Err(VdpInterruptError::TimeReversal {
            current: tick(1),
            requested: tick(0)
        }
        .into())
    );
    assert_eq!(device, pending);
    device
        .advance_to(tick(HORIZONTAL_EDGE - 1), Region::NtscUs)
        .unwrap();
    let before_edge = device.clone();
    assert_eq!(
        device.queue_register(0, HINT_ENABLE, Region::NtscUs),
        Err(VdpScheduleError::CoincidentPublication(tick(
            HORIZONTAL_EDGE
        )))
    );
    assert_eq!(device, before_edge);
    let (state, mut registers) = known(EPOCH, 0, 0);
    registers.advance_to(tick(EPOCH + 1)).unwrap();
    assert_eq!(
        Vdp::from_known_state(state, registers, Region::NtscUs),
        Err(VdpScheduleError::ClockMismatch {
            interrupt: tick(EPOCH),
            registers: tick(EPOCH + 1)
        })
    );
    let mismatched = VdpInterrupts::at_counters_zero(tick(EPOCH + 1), 0, false, false);
    assert_eq!(
        Vdp::from_known_state(mismatched, registers, Region::NtscUs),
        Err(VdpScheduleError::PhaseMismatch)
    );
    assert_eq!(
        Vdp::from_known_state(
            VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false),
            VdpRegisterPipeline::at_h32_edge(MasterTicks::ZERO),
            Region::NtscUs,
        ),
        Err(VdpScheduleError::Interrupt(VdpInterruptError::Timing(
            VdpTimingError::UnknownRegister(MODE_REGISTER)
        )))
    );
    for (index, value) in [(MODE_REGISTER, MODE5 | DISPLAY | 8), (SIZE_REGISTER, 2)] {
        let (mut state, mut registers) = known(EPOCH, 0, 0);
        drain(&mut state, &mut registers, EPOCH + 1);
        registers.queue_register(index, value).unwrap();
        assert_eq!(
            Vdp::from_known_state(state, registers, Region::NtscUs),
            Err(VdpRegisterError::UnsupportedModeChange { index, value }.into())
        );
    }
    let (mut state, mut registers) = known(EPOCH, 0, 0);
    drain(&mut state, &mut registers, EPOCH + HORIZONTAL_EDGE - 1);
    let write = registers.queue_register(RELOAD_REGISTER, 7).unwrap();
    assert_eq!(
        Vdp::from_known_state(state, registers, Region::NtscUs),
        Err(VdpScheduleError::CoincidentPublication(write.publish_at))
    );
}

pub fn acknowledge_endpoints() {
    let (state, registers) = known(EPOCH, 0, 0);
    let initial = Vdp::from_known_state(state, registers, Region::NtscUs).unwrap();
    for endpoint in [
        EPOCH + HORIZONTAL_EDGE,
        EPOCH + VERTICAL_EDGE,
        EPOCH + 11 * FRAME_TICKS + VERTICAL_EDGE,
    ] {
        let mut device = initial.clone();
        assert_eq!(
            device.acknowledge_to(tick(endpoint), Region::NtscUs),
            Err(VdpScheduleError::CoincidentAcknowledge(tick(endpoint)))
        );
        assert_eq!(device, initial);
        let mut state = state;
        let mut registers = registers;
        let expected = state
            .complete_scheduled_acknowledge(tick(endpoint + 1), Region::NtscUs, &mut registers)
            .unwrap();
        assert_eq!(
            device.acknowledge_to(tick(endpoint + 1), Region::NtscUs),
            Ok(expected)
        );
        compare(&device, &state, &registers);
    }
    let mut device = initial.clone();
    assert_eq!(
        device.acknowledge_to(tick(EPOCH), Region::NtscUs),
        Err(VdpScheduleError::EmptyAcknowledgeInterval)
    );
    assert_eq!(
        device.acknowledge_to(tick(EPOCH - 1), Region::NtscUs),
        Err(VdpInterruptError::TimeReversal {
            current: tick(EPOCH),
            requested: tick(EPOCH - 1)
        }
        .into())
    );
    assert_eq!(device, initial);
    device
        .advance_to(tick(EPOCH + HORIZONTAL_EDGE + 1), Region::NtscUs)
        .unwrap();
    let write = device.queue_register(0, 0, Region::NtscUs).unwrap();
    let pending = device.clone();
    assert_eq!(
        device.acknowledge_to(write.publish_at, Region::NtscUs),
        Err(VdpScheduleError::CoincidentAcknowledge(write.publish_at))
    );
    assert_eq!(device, pending);
    assert_eq!(
        device.acknowledge_to(tick(write.publish_at.get() + 1), Region::NtscUs),
        Ok(0)
    );
    assert!(device.interrupts().horizontal_pending());
    assert_eq!(device.registers().pending(), None);
}

pub fn maximum_clock_jump() {
    for reload in 0..=u8::MAX {
        let (state, registers) = known(EPOCH, reload, reload.saturating_sub(1));
        let initial = Vdp::from_known_state(state, registers, Region::NtscUs).unwrap();
        let mut device = initial.clone();
        device.advance_to(tick(u64::MAX), Region::NtscUs).unwrap();
        assert_eq!(device.now(), tick(u64::MAX));
        assert_eq!(device.registers().now(), tick(u64::MAX));
        let mut chunked = initial.clone();
        chunked
            .advance_to(
                tick(EPOCH + 3 * FRAME_TICKS + HORIZONTAL_EDGE),
                Region::NtscUs,
            )
            .unwrap();
        chunked.advance_to(tick(u64::MAX), Region::NtscUs).unwrap();
        assert_eq!(device, chunked);
        let last_cycle = EPOCH + (u64::MAX - EPOCH) / FRAME_TICKS * FRAME_TICKS;
        let (_, mut reference_registers) = known(last_cycle, reload, 0);
        let mut reference = VdpInterrupts::at_counters_zero(
            tick(last_cycle),
            reload.saturating_sub(1),
            reload <= 224,
            true,
        );
        drain(&mut reference, &mut reference_registers, u64::MAX);
        assert_eq!(device.interrupts().line_counter(), reference.line_counter());
        assert_eq!(
            device.interrupts().horizontal_pending(),
            reference.horizontal_pending()
        );
        assert_eq!(
            device.interrupts().vertical_pending(),
            reference.vertical_pending()
        );
        assert_eq!(
            device.registers().registers(),
            reference_registers.registers()
        );
        let saved = device.clone();
        device.advance_to(tick(u64::MAX), Region::NtscUs).unwrap();
        assert_eq!(device, saved);
        assert_eq!(
            device.queue_register(RELOAD_REGISTER, 0, Region::NtscUs),
            Err(VdpRegisterError::ClockOverflow.into())
        );
        assert_eq!(device, saved);
        let mut acknowledged = initial.clone();
        assert_eq!(
            acknowledged.acknowledge_to(tick(u64::MAX), Region::NtscUs),
            Ok(6)
        );
        assert!(!acknowledged.interrupts().vertical_pending());
        assert_eq!(
            acknowledged.interrupts().horizontal_pending(),
            reference.horizontal_pending()
        );
        assert_eq!(
            acknowledged.interrupts().line_counter(),
            reference.line_counter()
        );
        assert_eq!(acknowledged.registers(), device.registers());
    }
}
