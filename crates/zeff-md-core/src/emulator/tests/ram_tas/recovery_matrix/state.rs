use super::*;
use common::*;

fn raw20(machine: &Emulator) -> Vec<u8> {
    let raw = machine.encode_state().unwrap();
    let offset = crate::save_state::STATE_MAGIC.len();
    assert_eq!(
        &raw[offset..offset + 4],
        &crate::save_state::STATE_VERSION.to_le_bytes()
    );
    raw
}

fn active(concurrent: bool, idle: bool) -> (Emulator, Option<oracle::Oracle>) {
    let case = Case {
        kind: Kind::Immediate,
        residue: 0,
        edge: 0,
        lane: 0,
    };
    let (mut machine, mut expected) = seed_ram_mode(concurrent, &case, false, false);
    let clock = machine.bus.cpu_clock;
    assert_eq!(
        adapter(&mut machine).begin_rmw(access(0), clock),
        Ok(BusResponse::Complete(()))
    );
    if idle {
        if let Some(expected) = &mut expected {
            expected.read();
        }
        assert_eq!(
            adapter(&mut machine).read_cycle(access(0), clock),
            CycleResult::ready(0x00de)
        );
        let clock = machine.bus.cpu_clock;
        if let Some(expected) = &mut expected {
            expected.held(14);
        }
        adapter(&mut machine).idle(clock, 2);
        if concurrent {
            assert!(machine.sound_schedule.rmw_checkpoint.is_some());
        }
    }
    machine.cpu = complete_cpu(
        machine.cpu.registers,
        [0x4ad0, 0x4e71],
        machine.bus.cpu_clock.0,
    );
    (machine, expected)
}

fn encode_guards() {
    for concurrent in [false, true] {
        let (mut machine, _) = active(concurrent, false);
        let before = machine.clone();
        assert!(machine.encode_state().is_err());
        assert!(machine.encode_frontend_state().is_err());
        assert_eq!(machine, before);
        let clock = machine.bus.cpu_clock;
        adapter(&mut machine).end_rmw(clock);
        raw20(&machine);
    }
    let (mut machine, mut expected) = active(true, true);
    let scope = machine.bus.rmw.take().unwrap();
    assert!(machine.sound_schedule.rmw_checkpoint.is_some());
    let before = machine.clone();
    assert!(machine.encode_state().is_err());
    assert!(machine.encode_frontend_state().is_err());
    assert_eq!(machine, before);
    expected.as_mut().unwrap().end();
    machine.sound_schedule.finish_rmw(
        scope.reservation.as_ref().unwrap(),
        machine.bus.now.get(),
        &mut crate::sound_trace::NoSoundCapture,
    );
    expected.unwrap().check(&adapter(&mut machine).inner);
    raw20(&machine);
}

fn decode_guards() {
    for concurrent in [false, true] {
        for malformed in [false, true] {
            let case = Case {
                kind: Kind::Immediate,
                residue: 0,
                edge: 0,
                lane: 0,
            };
            let (mut baseline, _) = seed(concurrent, &case);
            let raw = raw20(&baseline);
            baseline.decode_state(&raw).unwrap();
            let (mut machine, _) = active(concurrent, true);
            let before = machine.clone();
            let mut bytes = raw.clone();
            if malformed {
                bytes.push(0x5a);
            }
            let result = machine.decode_state(&bytes);
            if malformed {
                assert!(result.is_err());
                assert_eq!(machine, before);
                assert!(machine.bus.rmw.is_some());
                if concurrent {
                    assert!(machine.sound_schedule.rmw_checkpoint.is_some());
                }
            } else {
                result.unwrap();
                assert!(machine.bus.rmw.is_none());
                assert!(machine.sound_schedule.rmw_checkpoint.is_none());
                assert_eq!(raw20(&machine), raw);
                hardware_equal(&machine, &baseline);
            }
        }
    }
}

fn resets() {
    for concurrent in [false, true] {
        for kind in 0..3 {
            let (mut machine, mut expected) = active(concurrent, true);
            if kind == 0 {
                machine.bus.cartridge.initial.pc = 0xc0000e;
                let before = machine.clone();
                assert!(machine.reset_cpu().is_err());
                assert_eq!(machine, before);
                assert!(machine.bus.rmw.is_some());
                if concurrent {
                    assert!(machine.sound_schedule.rmw_checkpoint.is_some());
                }
            } else if kind == 1 {
                let mut expected_ram = machine.bus.ram.clone();
                if concurrent {
                    expected_ram[0x0e] = 0xa6;
                }
                let mut reference = machine.clone();
                let clock = reference.bus.cpu_clock;
                adapter(&mut reference).end_rmw(clock);
                if let Some(expected) = &mut expected {
                    expected.end();
                    expected.check(&adapter(&mut reference).inner);
                }
                reference.reset_cpu().unwrap();
                machine.reset_cpu().unwrap();
                assert_eq!(machine, reference);
                assert_eq!(machine.bus.ram, expected_ram);
                assert_eq!(machine.bus.now.get(), if concurrent { 1407 } else { 1358 });
                assert_eq!(
                    machine.cpu.clock(),
                    CpuClocks(if concurrent { 15 } else { 8 })
                );
                assert_eq!(machine.sound_cpu.cycles(), if concurrent { 50 } else { 0 });
                if concurrent {
                    let pending = machine.sound_schedule.pending.as_ref().unwrap();
                    assert_eq!((pending.start, pending.deadline), (1350, 1410));
                    assert!(!pending.any_bank_access);
                }
                assert!(machine.bus.rmw.is_none());
                assert!(machine.sound_schedule.rmw_checkpoint.is_none());
                raw20(&machine);
            } else {
                let reference = Emulator::new(
                    machine.bus.cartridge.bytes.to_vec(),
                    LoadConfig {
                        concurrent_sound: concurrent,
                        ..Default::default()
                    },
                )
                .unwrap();
                Reset::reset(&mut machine);
                assert_eq!(machine, reference);
                assert!(machine.bus.rmw.is_none());
                assert!(machine.sound_schedule.rmw_checkpoint.is_none());
                assert_eq!(machine.sound_fault(), None);
                assert_eq!(raw20(&machine), raw20(&reference));
            }
        }
    }
}

fn four_fields(actual: &mut Emulator, expected: &mut Emulator) {
    for _ in 0..4 {
        actual.step_field().unwrap();
        expected.step_field().unwrap();
        assert_eq!(
            actual.framebuffer_dimensions(),
            expected.framebuffer_dimensions()
        );
        assert_eq!(actual.framebuffer(), expected.framebuffer());
        assert_eq!(actual.frame_ready(), expected.frame_ready());
        assert_eq!(actual.video_error(), expected.video_error());
        assert_eq!(
            actual
                .drain_audio_samples()
                .into_iter()
                .map(f32::to_bits)
                .collect::<Vec<_>>(),
            expected
                .drain_audio_samples()
                .into_iter()
                .map(f32::to_bits)
                .collect::<Vec<_>>()
        );
        assert_eq!(raw20(actual), raw20(expected));
        assert_eq!(
            actual.encode_frontend_state().unwrap(),
            expected.encode_frontend_state().unwrap()
        );
        hardware_equal(actual, expected);
    }
}

fn persistence() {
    for concurrent in [false, true] {
        for initial in 0..3 {
            let case = Case {
                kind: Kind::Immediate,
                residue: 0,
                edge: 0,
                lane: 0,
            };
            let (mut machine, _) = seed(concurrent, &case);
            let before = machine.encode_frontend_state().unwrap();
            let raw_before = raw20(&machine);
            let mut reference = machine.clone();
            if initial == 1 {
                machine.step_instruction().unwrap();
                machine.decode_frontend_state(&before).unwrap();
            } else if initial == 2 {
                let mut history = std::collections::VecDeque::from([before.clone()]);
                machine.step_instruction().unwrap();
                for _ in 0..8 {
                    machine.step_instruction().unwrap();
                }
                history.push_back(machine.encode_frontend_state().unwrap());
                history.pop_back();
                machine
                    .decode_frontend_state(&history.pop_back().unwrap())
                    .unwrap();
            }
            assert_eq!(raw20(&machine), raw_before);
            assert_eq!(machine.encode_frontend_state().unwrap(), before);
            assert_eq!(machine.step_instruction(), reference.step_instruction());
            assert!(machine.bus.rmw.is_none());
            assert!(machine.sound_schedule.rmw_checkpoint.is_none());
            let after = machine.encode_frontend_state().unwrap();
            let raw_after = raw20(&machine);
            let mut restored = machine.clone();
            restored.decode_frontend_state(&after).unwrap();
            assert_eq!(raw20(&restored), raw_after);
            for _ in 0..8 {
                assert_eq!(machine.step_instruction(), restored.step_instruction());
                assert_eq!(raw20(&machine), raw20(&restored));
                hardware_equal(&machine, &restored);
            }
            four_fields(&mut machine, &mut restored);
        }
    }
}

struct After<'a> {
    inner: Adapter<'a>,
    ended: bool,
    post: usize,
    iack: usize,
}
impl After<'_> {
    fn guard(&mut self) {
        if self.ended {
            assert!(self.inner.inner.bus.rmw.is_none());
            assert!(self.inner.inner.schedule.rmw_checkpoint.is_none());
            self.post += 1;
        }
    }
}
impl ClockedBus for After<'_> {
    type Error = BusError;
    fn read_cycle(&mut self, a: BusAccess, t: CpuClocks) -> CycleResult<u16, BusError> {
        self.guard();
        self.inner.read_cycle(a, t)
    }
    fn write_cycle(&mut self, a: BusAccess, v: u16, t: CpuClocks) -> CycleResult<(), BusError> {
        self.guard();
        self.inner.write_cycle(a, v, t)
    }
    fn idle(&mut self, t: CpuClocks, n: u32) {
        self.guard();
        self.inner.idle(t, n);
    }
    fn begin_rmw(&mut self, a: BusAccess, t: CpuClocks) -> Result<BusResponse<()>, BusError> {
        assert!(!self.ended);
        self.inner.begin_rmw(a, t)
    }
    fn end_rmw(&mut self, t: CpuClocks) {
        self.inner.end_rmw(t);
        self.ended = true;
        self.guard();
    }
    fn acknowledge_cycle(
        &mut self,
        a: BusAccess,
        t: CpuClocks,
    ) -> CycleResult<BusResponse<zeff_m68k::timing::InterruptVector>, BusError> {
        assert!(self.ended);
        self.guard();
        self.iack += 1;
        self.inner.acknowledge_cycle(a, t)
    }
}

fn trace_iack() {
    for concurrent in [false, true] {
        for trace in [false, true] {
            let case = Case {
                kind: Kind::Immediate,
                residue: 0,
                edge: 0,
                lane: 0,
            };
            let (mut machine, _) = seed(concurrent, &case);
            let vector = if trace { 9 } else { 27 };
            let rom = std::sync::Arc::make_mut(&mut machine.bus.cartridge.bytes);
            rom[vector * 4..vector * 4 + 4].copy_from_slice(&0x180_u32.to_be_bytes());
            rom[0x180..0x188].copy_from_slice(&[0x4e, 0x71, 0x4e, 0x71, 0x60, 0xfe, 0x4e, 0x71]);
            machine
                .cpu
                .registers
                .set_status_register(if trace { 0xa71f } else { 0x201f });
            let stack = machine.cpu.registers.ssp;
            let mut cpu = machine.cpu.clone();
            let mut bus = After {
                inner: adapter(&mut machine),
                ended: false,
                post: 0,
                iack: 0,
            };
            cpu.step_instruction(&mut bus).unwrap();
            assert!(bus.ended);
            assert!(bus.post > 0);
            if trace {
                assert!(cpu.trace_pending());
                assert!(cpu.service_pending_trace(&mut bus).unwrap().is_some());
                assert_eq!(bus.iack, 0);
            } else {
                cpu.interrupt_input().set_level(3);
                bus.inner.inner.bus.irq_level = 3;
                assert!(matches!(
                    cpu.step_boundary(&mut bus).unwrap(),
                    TimedOutcome::Interrupt { .. }
                ));
                assert_eq!(bus.iack, 1);
            }
            assert!(bus.post >= 5);
            assert_eq!(cpu.registers.pc, 0x180);
            assert_eq!(cpu.registers.ssp, stack - 6);
            assert_eq!(cpu.queued_words(), Some([0x4e71, 0x4e71]));
            machine.cpu = cpu;
            assert!(machine.bus.rmw.is_none());
            assert!(machine.sound_schedule.rmw_checkpoint.is_none());
            raw20(&machine);
        }
    }
}

pub(super) fn rows() {
    persistence();
    encode_guards();
    decode_guards();
    resets();
    trace_iack();
}

pub(super) fn failed_second_prime() {
    for concurrent in [false, true] {
        let (mut machine, _) = active(concurrent, true);
        let mut rom = machine.bus.cartridge.bytes.to_vec();
        rom.resize(0x400000, 0);
        rom[0x3ffffe..].copy_from_slice(&0xa55a_u16.to_be_bytes());
        machine.bus.cartridge.bytes = rom.into();
        machine.bus.cartridge.initial.pc = 0x3ffffe;
        let before = machine.clone();
        assert!(matches!(
            machine.reset_cpu(),
            Err(TimingFault::Bus {
                access: BusAccess {
                    bus_address: 0x400000,
                    kind: AccessKind::Prefetch,
                    ..
                },
                direction: zeff_m68k::bus::Direction::Read,
                source: BusError::UnsupportedAccess {
                    address: 0x400000,
                    direction: zeff_m68k::bus::Direction::Read,
                },
            })
        ));
        assert_eq!(machine, before);
        assert!(machine.bus.rmw.is_some());
        assert_eq!(machine.sound_schedule.rmw_checkpoint.is_some(), concurrent);
    }
}
