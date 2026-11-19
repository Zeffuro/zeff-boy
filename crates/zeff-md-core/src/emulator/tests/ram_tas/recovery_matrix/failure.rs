use super::*;
use common::*;
use std::collections::VecDeque;
use zeff_m68k::bus::Direction;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    BeginUnsupported,
    BeginError,
    ReadBefore,
    ReadAfter,
    IdleAfter,
    WriteAfter,
    TailBefore,
    TailAfter,
    ReadClock,
    IdleOverflow,
    GenericIdleOverflow,
    WriteClock,
    Envelope,
    CapacityRead,
    CapacityWrite,
}

#[derive(Clone, Debug)]
enum Action {
    Begin(Result<BusResponse<()>, BusError>),
    Read(CycleResult<u16, BusError>),
    Idle,
    Write(CycleResult<(), BusError>),
    End,
}

struct Script(VecDeque<Action>);
impl ClockedBus for Script {
    type Error = BusError;
    fn begin_rmw(&mut self, _: BusAccess, _: CpuClocks) -> Result<BusResponse<()>, BusError> {
        match self.0.pop_front().unwrap() {
            Action::Begin(v) => v,
            a => panic!("{a:?}"),
        }
    }
    fn read_cycle(&mut self, _: BusAccess, _: CpuClocks) -> CycleResult<u16, BusError> {
        match self.0.pop_front().unwrap() {
            Action::Read(v) => v,
            a => panic!("{a:?}"),
        }
    }
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), BusError> {
        match self.0.pop_front().unwrap() {
            Action::Write(v) => v,
            a => panic!("{a:?}"),
        }
    }
    fn idle(&mut self, _: CpuClocks, n: u32) {
        assert_eq!(n, 2);
        assert!(matches!(self.0.pop_front(), Some(Action::Idle)));
    }
    fn end_rmw(&mut self, _: CpuClocks) {
        assert!(matches!(self.0.pop_front(), Some(Action::End)));
    }
}

struct Probe<'a> {
    inner: Adapter<'a>,
    expected: Option<oracle::Oracle>,
    direct: crate::hardware::bus::Bus,
    failure: Failure,
    actions: VecDeque<Action>,
    ends: usize,
    callbacks: usize,
    checkpoint_seen: bool,
}

fn injected(a: BusAccess, direction: Direction) -> BusError {
    BusError::UnsupportedAccess {
        address: a.bus_address,
        direction,
    }
}

impl Probe<'_> {
    fn check(&mut self) {
        if let Some(expected) = &self.expected {
            expected.check(&self.inner.inner);
        } else {
            assert_eq!(self.inner.inner.bus.ram, self.direct.ram);
            assert_eq!(self.inner.inner.bus.sound, self.direct.sound);
            assert_eq!(self.inner.inner.bus.cartridge, self.direct.cartridge);
            assert_eq!(self.inner.inner.bus.vdp, self.direct.vdp);
            assert_eq!(self.inner.inner.bus.video, self.direct.video);
            assert_eq!(self.inner.inner.bus.audio, self.direct.audio);
            assert_eq!(self.inner.inner.bus.now, self.direct.now);
            assert_eq!(self.inner.inner.bus.cpu_clock, self.direct.cpu_clock);
        }
        self.checkpoint_seen |= self.inner.inner.schedule.rmw_checkpoint.is_some();
    }
    fn direct_advance(&mut self, ticks: u64) {
        self.direct.advance_machine(ticks).unwrap();
        self.direct.cpu_clock.0 += ticks / 7;
    }
}

impl ClockedBus for Probe<'_> {
    type Error = BusError;
    fn begin_rmw(&mut self, a: BusAccess, t: CpuClocks) -> Result<BusResponse<()>, BusError> {
        self.callbacks += 1;
        assert_eq!(a, access(0));
        let predicted = match self.failure {
            Failure::BeginUnsupported => Ok(BusResponse::Unsupported),
            Failure::BeginError => Err(injected(a, Direction::Read)),
            Failure::Envelope => Err(BusError::ClockOverflow),
            _ => Ok(BusResponse::Complete(())),
        };
        let actual = if self.failure == Failure::BeginUnsupported {
            predicted.clone()
        } else if self.failure == Failure::BeginError {
            assert_eq!(self.inner.begin_rmw(a, t), Ok(BusResponse::Complete(())));
            self.inner.end_rmw(t);
            predicted.clone()
        } else {
            self.inner.begin_rmw(a, t)
        };
        assert_eq!(actual, predicted);
        self.actions.push_back(Action::Begin(predicted));
        self.check();
        actual
    }
    fn read_cycle(&mut self, a: BusAccess, t: CpuClocks) -> CycleResult<u16, BusError> {
        self.callbacks += 1;
        let data = a.kind == AccessKind::Data;
        let before = data && self.failure == Failure::ReadBefore
            || !data && self.failure == Failure::TailBefore;
        let (predicted, actual) = if before {
            let p = CycleResult {
                result: Err(injected(a, Direction::Read)),
                wait_clocks: 0,
            };
            (p.clone(), p)
        } else if data && self.failure == Failure::ReadClock {
            let p = CycleResult {
                result: Err(BusError::ClockMismatch {
                    expected: t,
                    actual: CpuClocks(t.0 + 1),
                }),
                wait_clocks: 0,
            };
            (p, self.inner.read_cycle(a, CpuClocks(t.0 + 1)))
        } else {
            let wait = if let Some(expected) = &mut self.expected {
                if data {
                    expected.read()
                } else {
                    expected.ordinary()
                }
            } else {
                self.direct_advance(28);
                0
            };
            let ram = self
                .expected
                .as_ref()
                .map_or(&self.direct.ram, |e| &e.bus.ram);
            let mut p = CycleResult {
                result: Ok(if data {
                    u16::from_be_bytes([ram[0x0e], ram[0x0f]])
                } else {
                    0x4e71
                }),
                wait_clocks: wait,
            };
            let mut actual = self.inner.read_cycle(a, t);
            assert_eq!(actual, p);
            if data && self.failure == Failure::ReadAfter
                || !data && self.failure == Failure::TailAfter
            {
                p.result = Err(injected(a, Direction::Read));
                actual.result = p.result.clone();
            }
            if data && self.failure == Failure::GenericIdleOverflow {
                assert_eq!(wait, 0);
                p.wait_clocks = u32::MAX;
                actual.wait_clocks = u32::MAX;
            }
            (p, actual)
        };
        assert_eq!(actual, predicted);
        self.actions.push_back(Action::Read(predicted));
        self.check();
        if data && self.failure == Failure::IdleOverflow {
            self.inner.inner.bus.now = MasterTicks::new(u64::MAX - 13);
            self.direct.now = self.inner.inner.bus.now;
            if let Some(expected) = &mut self.expected {
                expected.bus.now = self.inner.inner.bus.now;
            }
        }
        actual
    }
    fn idle(&mut self, t: CpuClocks, n: u32) {
        self.callbacks += 1;
        assert_eq!(n, 2);
        if self.failure == Failure::IdleOverflow {
            let mut before = self.inner.inner.bus.clone();
            self.inner.idle(t, n);
            before.clock_fault = true;
            assert_eq!(*self.inner.inner.bus, before);
            self.actions.push_back(Action::Idle);
            self.check();
            return;
        }
        if let Some(expected) = &mut self.expected {
            expected.held(14);
        } else {
            self.direct_advance(14);
        }
        self.inner.idle(t, n);
        self.actions.push_back(Action::Idle);
        self.check();
    }
    fn write_cycle(&mut self, a: BusAccess, v: u16, t: CpuClocks) -> CycleResult<(), BusError> {
        self.callbacks += 1;
        assert_eq!(a, access(0));
        assert_eq!(v, 0x8000);
        if matches!(self.failure, Failure::IdleOverflow | Failure::WriteClock) {
            let before = (
                self.inner.inner.bus.clone(),
                self.inner.inner.cpu.clone(),
                self.inner.inner.schedule.clone(),
            );
            let (clock, error) = if self.failure == Failure::IdleOverflow {
                (t, BusError::ClockOverflow)
            } else {
                (
                    CpuClocks(t.0 + 1),
                    BusError::ClockMismatch {
                        expected: t,
                        actual: CpuClocks(t.0 + 1),
                    },
                )
            };
            let predicted = CycleResult {
                result: Err(error),
                wait_clocks: 0,
            };
            assert_eq!(self.inner.write_cycle(a, v, clock), predicted);
            assert_eq!(
                (
                    &*self.inner.inner.bus,
                    &*self.inner.inner.cpu,
                    &*self.inner.inner.schedule
                ),
                (&before.0, &before.1, &before.2),
            );
            self.actions.push_back(Action::Write(predicted.clone()));
            self.check();
            return predicted;
        }
        let predicted = CycleResult {
            result: if matches!(self.failure, Failure::IdleAfter | Failure::WriteAfter) {
                Err(injected(a, Direction::Write))
            } else {
                Ok(())
            },
            wait_clocks: 0,
        };
        if self.failure != Failure::IdleAfter {
            if let Some(expected) = &mut self.expected {
                expected.held(28);
            } else {
                self.direct_advance(28);
            }
            assert_eq!(self.inner.write_cycle(a, v, t), CycleResult::ready(()));
        }
        self.actions.push_back(Action::Write(predicted.clone()));
        self.check();
        predicted
    }
    fn end_rmw(&mut self, t: CpuClocks) {
        self.callbacks += 1;
        self.ends += 1;
        if let Some(expected) = &mut self.expected
            && expected.lock.is_some()
        {
            expected.end();
        }
        self.inner.end_rmw(t);
        assert!(self.inner.inner.bus.rmw.is_none());
        assert!(self.inner.inner.schedule.rmw_checkpoint.is_none());
        self.actions.push_back(Action::End);
        self.check();
    }
}

pub(super) fn abort(concurrent: bool, failure: Failure) {
    let case = Case {
        kind: Kind::Immediate,
        residue: 0,
        edge: 0,
        lane: 0,
    };
    let (mut machine, mut expected) = seed(concurrent, &case);
    if matches!(
        failure,
        Failure::GenericIdleOverflow | Failure::CapacityRead | Failure::CapacityWrite
    ) {
        let gap = match failure {
            Failure::GenericIdleOverflow => 5,
            Failure::CapacityRead => 3,
            _ => 9,
        };
        let clock = u64::MAX - u64::from(u32::MAX) - gap;
        machine.cpu = complete_cpu(machine.cpu.registers, [0x4ad0, 0x4e71], clock);
        machine.bus.cpu_clock = CpuClocks(clock);
        if let Some(expected) = &mut expected {
            expected.bus.cpu_clock = CpuClocks(clock);
        }
    }
    if failure == Failure::Envelope {
        machine.bus.now = MasterTicks::new(u64::MAX - 69);
        if let Some(expected) = &mut expected {
            expected.bus.now = machine.bus.now;
        }
    }
    let mut reference_cpu = machine.cpu.clone();
    let initial_registers = machine.cpu.registers;
    let initial_clock = machine.cpu.clock().0;
    let initial_bus_clock = machine.bus.cpu_clock.0;
    let initial_master = machine.bus.now.get();
    let before_ram = machine.bus.ram.clone();
    let mut probe = Probe {
        direct: machine.bus.clone(),
        inner: Adapter {
            concurrent,
            inner: ScheduledBus::new(
                &mut machine.bus,
                &mut machine.sound_cpu,
                &mut machine.sound_schedule,
            ),
        },
        expected,
        failure,
        actions: VecDeque::new(),
        ends: 0,
        callbacks: 0,
        checkpoint_seen: false,
    };
    let result = machine.cpu.step_instruction(&mut probe);
    assert!(result.is_err());
    let mut script = Script(probe.actions.clone());
    assert_eq!(reference_cpu.step_instruction(&mut script), result);
    assert!(script.0.is_empty());
    assert_eq!(machine.cpu, reference_cpu);
    assert_eq!(machine.cpu.queued_words(), Some([0x4ad0, 0x4e71]));
    assert_eq!(machine.cpu.registers.pc, initial_registers.pc);
    let flags = if matches!(
        failure,
        Failure::IdleAfter
            | Failure::WriteAfter
            | Failure::TailBefore
            | Failure::TailAfter
            | Failure::IdleOverflow
            | Failure::GenericIdleOverflow
            | Failure::WriteClock
            | Failure::CapacityWrite
    ) {
        0x2714
    } else {
        0x271f
    };
    assert_eq!(machine.cpu.registers.status_register(), flags);
    let expected_ends = usize::from(!matches!(
        failure,
        Failure::BeginUnsupported | Failure::BeginError | Failure::Envelope
    ));
    assert_eq!(probe.ends, expected_ends);
    if concurrent
        && matches!(
            failure,
            Failure::ReadAfter
                | Failure::IdleAfter
                | Failure::WriteAfter
                | Failure::TailBefore
                | Failure::TailAfter
                | Failure::IdleOverflow
                | Failure::GenericIdleOverflow
                | Failure::WriteClock
                | Failure::CapacityWrite
        )
    {
        assert!(probe.checkpoint_seen);
    }
    if matches!(
        failure,
        Failure::BeginUnsupported | Failure::BeginError | Failure::Envelope | Failure::CapacityRead
    ) {
        assert_eq!(machine.cpu.clock().0, initial_clock);
    }
    let mut calls = probe.callbacks;
    let expected = probe.expected.take();
    drop(probe);
    let (cpu_clocks, bus_clocks) = match failure {
        Failure::BeginUnsupported
        | Failure::BeginError
        | Failure::Envelope
        | Failure::CapacityRead => (0, 0),
        Failure::ReadBefore | Failure::ReadClock => (4, 0),
        Failure::ReadAfter => (4, 4),
        Failure::GenericIdleOverflow => (u64::from(u32::MAX) + 4, 4),
        Failure::CapacityWrite => (6, 6),
        Failure::IdleOverflow => (10, 4),
        Failure::IdleAfter | Failure::WriteClock => (10, 6),
        Failure::WriteAfter => (10, 10),
        Failure::TailBefore => (14, 10),
        Failure::TailAfter if concurrent => (22, 22),
        Failure::TailAfter => (14, 14),
    };
    assert_eq!(machine.cpu.clock().0, initial_clock + cpu_clocks);
    assert_eq!(machine.bus.cpu_clock.0, initial_bus_clock + bus_clocks);
    assert_eq!(
        machine.bus.now.get(),
        if failure == Failure::IdleOverflow {
            u64::MAX - 13
        } else {
            initial_master + bus_clocks * 7
        },
    );
    assert_eq!(machine.bus.clock_fault, failure == Failure::IdleOverflow);
    frozen(&mut machine, &mut calls);
    if let Some(expected) = expected {
        if !matches!(failure, Failure::Envelope | Failure::IdleOverflow) {
            future(&mut machine, expected);
        }
    } else {
        assert_eq!(machine.bus.ram, before_ram);
    }
}

fn audio_origin(audio: &mut crate::hardware::audio::Audio, at: u64) {
    use zeff_emu_common::save_state::{StateReader, StateWriter};
    let mut w = StateWriter::new();
    audio.write_state(&mut w).unwrap();
    let mut bytes = w.into_bytes();
    let offset = bytes.len() - 24;
    bytes[offset..offset + 8].copy_from_slice(&at.to_le_bytes());
    audio
        .read_state(&mut StateReader::new(&bytes), Region::NtscUs)
        .unwrap();
    assert_eq!(audio.state_now(), at);
}

fn sound_arithmetic(reserve: bool) {
    let mut machine = super::super::super::machine(&[0x4ad0, 0x4e71, 0x4e71]);
    machine.concurrent_sound = true;
    machine.cpu.registers.a[0] = 0xffff_000e;
    machine.step_instruction().unwrap();
    assert_eq!(machine.bus.ram[0x0e], 0);
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 0x1ff;
    machine.bus.sound.ram[..5].copy_from_slice(&[0x21, 0x0e, 0x80, 0x36, 0xa6]);
    let a = u64::MAX - if reserve { 165 } else { 90 };
    let origin = a - 17 * 15;
    audio_origin(&mut machine.bus.audio, origin);
    machine.sound_schedule.advance(
        &mut machine.sound_cpu,
        &mut machine.bus.sound,
        &mut machine.bus.cartridge,
        &mut machine.bus.ram,
        SoundInterval::new(origin..a, &mut machine.bus.audio),
    );
    assert!(machine.sound_schedule.pending.is_none());
    assert_eq!(machine.sound_cpu.cycles(), 17);
    let cpu = machine.sound_cpu.clone();
    let ram = machine.bus.ram.clone();
    let memory = machine.bus.sound.clone();
    let (start, end) = if reserve {
        (a, a + 1)
    } else {
        (u64::MAX - 14, u64::MAX - 13)
    };
    if !reserve {
        audio_origin(&mut machine.bus.audio, start);
    }
    let owned = [Reservation {
        requester: Requester::MAIN_RMW,
        ticks: a..a + 70,
    }];
    let mut interval = SoundInterval::new(start..end, &mut machine.bus.audio);
    if reserve {
        interval.occupied = &owned;
    }
    machine.sound_schedule.advance(
        &mut machine.sound_cpu,
        &mut machine.bus.sound,
        &mut machine.bus.cartridge,
        &mut machine.bus.ram,
        interval,
    );
    if reserve {
        assert_eq!(machine.sound_fault(), None);
        let checkpoint = machine.sound_schedule.rmw_checkpoint.as_ref().unwrap();
        assert_eq!((checkpoint.start, checkpoint.deadline), (a, a + 45));
        let pending = machine.sound_schedule.pending.as_ref().unwrap();
        assert_eq!((pending.start, pending.deadline), (a + 75, a + 120));
        let original = checkpoint.clone();
        let replacement = [Reservation {
            requester: Requester::MAIN_RMW,
            ticks: a + 75..a + 145,
        }];
        let mut interval = SoundInterval::new(a + 1..a + 2, &mut machine.bus.audio);
        interval.occupied = &replacement;
        machine.sound_schedule.advance(
            &mut machine.sound_cpu,
            &mut machine.bus.sound,
            &mut machine.bus.cartridge,
            &mut machine.bus.ram,
            interval,
        );
        assert_eq!(
            machine.sound_schedule.rmw_checkpoint.as_ref(),
            Some(&original)
        );
    }
    assert_eq!(
        machine.sound_fault(),
        Some(crate::SoundFault::ClockOverflow)
    );
    assert!(machine.sound_schedule.pending.is_none());
    assert_eq!(machine.sound_schedule.rmw_checkpoint.is_some(), reserve);
    assert_eq!(machine.sound_cpu, cpu);
    assert_eq!(machine.bus.sound, memory);
    assert_eq!(machine.bus.ram, ram);
    machine.sound_schedule.finish_rmw(
        &owned[0],
        if reserve { a + 2 } else { end },
        &mut crate::sound_trace::NoSoundCapture,
    );
    assert!(machine.sound_schedule.pending.is_none());
    assert!(machine.sound_schedule.rmw_checkpoint.is_none());
    assert_eq!(
        machine.sound_fault(),
        Some(crate::SoundFault::ClockOverflow)
    );
}

fn dma_overflow(concurrent: bool) {
    let case = Case {
        kind: Kind::Immediate,
        residue: 0,
        edge: 0,
        lane: 0,
    };
    let (mut machine, _) = seed(concurrent, &case);
    prior_dma::arm_external(&mut machine, 0);
    let clock = u64::MAX - 10;
    machine.bus.cpu_clock = CpuClocks(clock);
    machine.cpu = complete_cpu(machine.cpu.registers, [0x4ad0, 0x4e71], clock);
    let before = machine.clone();
    let mut cpu = machine.cpu.clone();
    assert_eq!(
        cpu.step_instruction(&mut adapter(&mut machine)),
        Err(TimingFault::ClockOverflow)
    );
    assert!(machine.bus.rmw.is_none());
    let mut bus = adapter(&mut machine);
    assert_eq!(
        bus.begin_rmw(access(0), CpuClocks(clock)),
        Ok(BusResponse::Complete(()))
    );
    let r = bus.read_cycle(access(0), CpuClocks(clock));
    assert_eq!(r.result, Err(BusError::ClockOverflow));
    assert_eq!(r.wait_clocks, 0);
    bus.end_rmw(CpuClocks(clock));
    assert_eq!(machine.bus, before.bus);
    assert_eq!(machine.sound_cpu, before.sound_cpu);
    assert_eq!(machine.sound_schedule, before.sound_schedule);
}

pub(super) fn rows() {
    let mut intake = 0;
    for failure in [
        Failure::BeginUnsupported,
        Failure::BeginError,
        Failure::ReadBefore,
        Failure::IdleOverflow,
        Failure::WriteClock,
        Failure::TailBefore,
        Failure::ReadClock,
        Failure::Envelope,
    ] {
        for concurrent in [false, true] {
            abort(concurrent, failure);
            intake += 1;
        }
    }
    assert_eq!(intake, 16);
    let mut after = 0;
    for failure in [
        Failure::ReadBefore,
        Failure::ReadAfter,
        Failure::IdleAfter,
        Failure::WriteAfter,
        Failure::TailAfter,
    ] {
        for concurrent in [false, true] {
            if concurrent && failure == Failure::IdleAfter {
                continue;
            }
            abort(concurrent, failure);
            after += 1;
        }
    }
    assert_eq!(after, 9);
    for failure in [Failure::CapacityRead, Failure::CapacityWrite] {
        for concurrent in [false, true] {
            abort(concurrent, failure);
        }
    }
    for concurrent in [false, true] {
        dma_overflow(concurrent);
        abort(concurrent, Failure::Envelope);
    }
    sound_arithmetic(false);
    sound_arithmetic(true);
    for concurrent in [false, true] {
        abort(concurrent, Failure::ReadAfter);
    }
}
