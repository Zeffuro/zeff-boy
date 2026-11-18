use super::*;
use oracle::Oracle;

struct Trace<'a> {
    inner: ScheduledBus<'a>,
    expected: Oracle,
    case: Case,
    events: Vec<&'static str>,
    waits: u64,
    sampled: u8,
    future: Option<(
        crate::hardware::bus::Bus,
        zeff_z80::Cpu,
        SoundSchedule,
        Oracle,
    )>,
}

impl Trace<'_> {
    fn check(&mut self, event: &'static str) {
        self.expected.check(&self.inner);
        if self.case.group == "F-epoch-trace"
            && matches!(event, "read" | "idle" | "write" | "end" | "prefetch")
        {
            let mut cpu = self.inner.cpu.clone();
            let mut schedule = self.inner.schedule.clone();
            schedule.materialize_epoch(
                &mut cpu,
                &self.inner.bus.sound,
                &mut crate::sound_trace::NoSoundCapture,
            );
            if self.case.epoch == Some(false) && !self.case.trace {
                if matches!(event, "read" | "idle" | "write" | "end") {
                    assert!(self.inner.schedule.epoch.is_some());
                    assert!(self.inner.schedule.pending.is_none());
                    let pending = schedule.pending.as_ref().unwrap();
                    assert_eq!((pending.start, pending.deadline), (1275, 1335));
                    assert_eq!(*self.inner.cpu, cpu);
                } else if self.events.len() == 7 {
                    assert!(self.inner.schedule.pending.is_none());
                    let pending = schedule.pending.as_ref().unwrap();
                    assert_eq!((pending.start, pending.deadline), (1335, 1395));
                    assert_ne!(*self.inner.cpu, cpu);
                    assert_eq!(cpu.cycles(), self.inner.cpu.cycles() + 4);
                }
            } else {
                assert_eq!(*self.inner.cpu, cpu);
                assert_eq!(self.inner.schedule.pending, schedule.pending);
            }
        }
        self.events.push(event);
    }

    fn operand(&self, access: BusAccess) {
        assert_eq!(access.logical_address, 0xffff_000e + self.case.lane);
        assert_eq!(access.bus_address, 0xff000e);
        assert_eq!(access.kind, AccessKind::Data);
        assert_eq!(access.function_code, FunctionCode::SupervisorData);
        assert_eq!(
            access.lanes,
            if self.case.lane == 0 {
                ByteLanes::Upper
            } else {
                ByteLanes::Lower
            }
        );
    }
}

impl ClockedBus for Trace<'_> {
    type Error = BusError;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, BusError> {
        assert_eq!(start, self.expected.bus.cpu_clock);
        let wait = if access.kind == AccessKind::Data {
            self.operand(access);
            self.expected.read()
        } else {
            self.expected.ordinary()
        };
        let result = self.inner.read_cycle(access, start);
        assert_eq!(result.wait_clocks, wait);
        self.waits += u64::from(wait);
        if access.kind == AccessKind::Data {
            let value =
                u16::from_be_bytes([self.expected.bus.ram[0x0e], self.expected.bus.ram[0x0f]]);
            assert_eq!(result.result, Ok(value));
            self.sampled = self.expected.bus.ram[0x0e + self.case.lane as usize];
            self.check("read");
        } else {
            assert!(result.result.is_ok());
            self.check("prefetch");
        }
        result
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), BusError> {
        self.operand(access);
        assert_eq!(start, self.expected.bus.cpu_clock);
        let expected = if self.case.lane == 0 {
            u16::from(self.sampled | 0x80) << 8
        } else {
            u16::from(self.sampled | 0x80)
        };
        assert_eq!(value, expected);
        self.expected.held(28);
        let result = self.inner.write_cycle(access, value, start);
        assert_eq!(result.result, Ok(()));
        assert_eq!(result.wait_clocks, 0);
        assert!(self.expected.deferrals <= 1);
        self.check("write");
        self.future = Some((
            self.inner.bus.clone(),
            self.inner.cpu.clone(),
            self.inner.schedule.clone(),
            self.expected.clone(),
        ));
        result
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(start, self.expected.bus.cpu_clock);
        assert_eq!(clocks, 2);
        self.expected.held(14);
        self.inner.idle(start, clocks);
        self.check("idle");
        if matches!(
            self.case.kind,
            Kind::LocalRead | Kind::LocalWrite | Kind::LocalPsg | Kind::LocalOpn2
        ) {
            assert_eq!(self.expected.target_publications, 1);
            assert_eq!(self.expected.deferrals, 0);
            assert!(self.inner.schedule.rmw_checkpoint.is_none());
            match self.case.kind {
                Kind::LocalRead => assert_eq!(self.inner.cpu.regs().a, 0x5a),
                Kind::LocalWrite => assert_eq!(self.inner.bus.sound.ram[0x1000], 0xa6),
                _ => {}
            }
        }
    }

    fn begin_rmw(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> Result<BusResponse<()>, BusError> {
        self.operand(access);
        assert_eq!(start, self.expected.bus.cpu_clock);
        let result = self.inner.begin_rmw(access, start);
        assert_eq!(result, Ok(BusResponse::Complete(())));
        assert!(self.inner.bus.rmw.is_some());
        self.check("begin");
        result
    }

    fn end_rmw(&mut self, end: CpuClocks) {
        assert_eq!(end, self.expected.bus.cpu_clock);
        self.expected.end();
        self.inner.end_rmw(end);
        assert!(self.inner.bus.rmw.is_none());
        self.check("end");
    }
}

fn advance(
    bus: &mut crate::hardware::bus::Bus,
    cpu: &mut zeff_z80::Cpu,
    schedule: &mut SoundSchedule,
    ticks: u64,
) {
    ScheduledBus::new(bus, cpu, schedule)
        .advance_machine(ticks)
        .unwrap();
}

fn future_publication(
    mut saved: (
        crate::hardware::bus::Bus,
        zeff_z80::Cpu,
        SoundSchedule,
        Oracle,
    ),
) {
    let (bus, cpu, schedule, expected) = &mut saved;
    let release = bus.now.get();
    ScheduledBus::new(bus, cpu, schedule).end_rmw(CpuClocks(release / 7));
    expected.end();
    let mut prediction = expected.clone();
    prediction.future(release + 900);
    let (_, deadline) = prediction.target.expect("target segment must be reached");
    assert_eq!(prediction.target_publications, 1);
    if deadline > release {
        let ticks = deadline - release - 1;
        expected.future(deadline - 1);
        advance(bus, cpu, schedule, ticks);
        expected.check(&ScheduledBus::new(bus, cpu, schedule));
        expected.future(deadline);
        advance(bus, cpu, schedule, 1);
        expected.check(&ScheduledBus::new(bus, cpu, schedule));
    }
    let end = bus.now.get() + 180;
    expected.future(end);
    advance(bus, cpu, schedule, 180);
    expected.check(&ScheduledBus::new(bus, cpu, schedule));
    assert_eq!(expected.target_publications, 1);
    assert!(bus.rmw.is_none());
    assert!(schedule.rmw_checkpoint.is_none());
}

pub(super) fn run(case: Case) {
    assert!(!case.group.is_empty());
    let nops = case.nops();
    let mut program = vec![0x4e71; nops];
    program.extend([0x4af9, 0xffff, 0x000e + case.lane as u16, 0x4e71, 0x4e71]);
    let mut machine = if case.kind == Kind::BankRegister {
        let mut image = crate::cartridge::registers::tests::rom(0);
        image[4..8].copy_from_slice(&0x100_u32.to_be_bytes());
        for (i, word) in program.iter().enumerate() {
            image[0x100 + i * 2..0x102 + i * 2].copy_from_slice(&word.to_be_bytes());
        }
        Emulator::new(image, LoadConfig::default()).unwrap()
    } else {
        super::super::super::machine(&program)
    };
    machine.bus.ram.fill(0xde);
    machine.bus.ram[0x0e + case.lane as usize] = 0;
    machine.bus.sound.bank = case.bank();
    let sound_program = case.program();
    machine.bus.sound.ram[..sound_program.len()].copy_from_slice(&sound_program);
    machine.bus.sound.ram[0x1000] = if matches!(case.kind, Kind::LocalRead | Kind::LocalWrite) {
        0x5a
    } else {
        0xa6
    };
    if case.trace {
        use zeff_emu_common::audio_trace::AudioTraceTiming;
        machine.bus.audio.trace = machine
            .bus
            .audio
            .trace
            .prepare_with_clock(
                64,
                machine.bus.rate(),
                crate::hardware::audio::trace::trace_chip(machine.region()),
                AudioTraceTiming::BusServiceBoundary,
            )
            .unwrap();
    }
    let mut prepared = machine.clone();
    prepared.bus.sound.set_reset(false);
    for _ in 0..nops {
        machine.step_instruction().unwrap();
    }
    let boundary = case.nominal_a() - 56;
    assert_eq!(machine.cpu.clock(), CpuClocks(8 + nops as u64 * 4));
    assert_eq!(machine.bus.cpu_clock, machine.cpu.clock());
    assert_eq!(machine.bus.now.get(), boundary);
    assert_eq!(case.nominal_a() % 15, case.residue);
    assert!(case.origin() >= 56 && case.origin() < boundary);
    let mut expected = Oracle::new(&prepared, &case);
    expected.advance_sound(case.origin(), boundary, None);
    prepared.sound_schedule.advance(
        &mut prepared.sound_cpu,
        &mut prepared.bus.sound,
        &mut prepared.bus.cartridge,
        &mut prepared.bus.ram,
        SoundInterval::new(case.origin()..boundary, &mut prepared.bus.audio),
    );
    machine.sound_cpu = prepared.sound_cpu;
    machine.sound_schedule = prepared.sound_schedule;
    machine.bus.sound = prepared.bus.sound;
    machine.bus.ram = prepared.bus.ram;
    machine.bus.cartridge.registers = prepared.bus.cartridge.registers;
    expected.bus.now = machine.bus.now;
    expected.bus.cpu_clock = machine.bus.cpu_clock;
    expected.bus.vdp = machine.bus.vdp.clone();
    expected.bus.video = machine.bus.video.clone();
    assert_eq!(expected.bus.audio, machine.bus.audio);
    machine.bus.audio = expected.bus.audio.clone();
    machine.concurrent_sound = true;
    if let Some(blocked) = case.epoch {
        machine.sound_schedule.begin_epoch_field();
        if blocked {
            machine.sound_schedule.block_epoch(
                &mut machine.sound_cpu,
                &machine.bus.sound,
                &mut crate::sound_trace::NoSoundCapture,
            );
        }
    }
    machine.cpu.registers.set_status_register(0x271f);
    assert_eq!(
        machine.cpu,
        PrefetchCpu::from_prefetched(machine.cpu.registers, [0x4af9, 0xffff], machine.cpu.clock())
            .unwrap()
    );
    let mut registers = machine.cpu.registers;
    registers.pc += 6;
    let initial_clock = machine.cpu.clock().0;
    let mut trace = Trace {
        inner: ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        ),
        expected,
        case: case.clone(),
        events: Vec::new(),
        waits: 0,
        sampled: 0,
        future: None,
    };
    trace.expected.check(&trace.inner);
    let report = machine.cpu.step_instruction(&mut trace).unwrap();
    assert_eq!(report.opcode, 0x4af9);
    assert_eq!(report.clocks, 22 + trace.waits);
    assert_eq!(
        trace.events,
        [
            "prefetch", "prefetch", "begin", "read", "idle", "write", "end", "prefetch"
        ]
    );
    registers.set_status_register(
        0x2710
            | if trace.sampled == 0 { 4 } else { 0 }
            | if trace.sampled & 0x80 != 0 { 8 } else { 0 },
    );
    assert_eq!(machine.cpu.registers, registers);
    assert_eq!(machine.cpu.clock().0, initial_clock + report.clocks);
    assert_eq!(machine.cpu.queued_words(), Some([0x4e71, 0x4e71]));
    assert_eq!(
        machine.cpu,
        PrefetchCpu::from_prefetched(
            registers,
            [0x4e71, 0x4e71],
            CpuClocks(initial_clock + report.clocks)
        )
        .unwrap()
    );
    future_publication(trace.future.take().unwrap());
    drop(trace);
    assert!(machine.bus.rmw.is_none());
    assert!(machine.sound_schedule.rmw_checkpoint.is_none());
    assert_eq!(machine.sound_fault(), None);
    machine.sound_schedule.finish_epoch_field(
        &mut machine.sound_cpu,
        &machine.bus.sound,
        &mut crate::sound_trace::NoSoundCapture,
    );
    machine.encode_state().unwrap();
}
