use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Immediate,
    Ldi,
    BankM1,
    BankPsg,
    LocalPsg,
    LocalOpn2,
}

#[derive(Clone, Debug)]
pub(super) struct Case {
    pub kind: Kind,
    pub residue: u64,
    pub edge: i64,
    pub lane: u32,
}

impl Case {
    pub fn prefix_t(&self) -> u64 {
        match self.kind {
            Kind::Immediate => 44,
            Kind::Ldi => 48,
            Kind::BankM1 => 47,
            _ => 41,
        }
    }

    pub fn expected_target(&self) -> (u32, bool, bool) {
        match self.kind {
            Kind::Immediate => (3, true, true),
            Kind::BankM1 => (4, false, true),
            Kind::LocalPsg | Kind::LocalOpn2 => (3, false, false),
            _ => (3, true, true),
        }
    }

    pub fn nops(&self) -> usize {
        (40..55)
            .find(|n| (112 + 28 * n) % 15 == self.residue)
            .unwrap() as usize
            + 2
    }

    pub fn a(&self) -> u64 {
        56 + self.nops() as u64 * 28
    }
    pub fn s(&self) -> u64 {
        ((self.a() as i64 + self.edge + 14) / 15) as u64 * 15
    }
    pub fn origin(&self) -> u64 {
        self.s() - self.prefix_t() * 15
    }

    pub fn program(&self) -> Vec<u8> {
        let target = 0x800e + self.lane as u16;
        let hl = match self.kind {
            Kind::Ldi => 0x1000,
            Kind::BankPsg => 0xff11,
            Kind::LocalPsg => 0x7f11,
            Kind::LocalOpn2 => 0x4000,
            _ => target,
        };
        let mut p = vec![
            0x01,
            0x01,
            0x00,
            0x11,
            target as u8,
            (target >> 8) as u8,
            0x21,
            hl as u8,
            (hl >> 8) as u8,
            0x3e,
            0xa6,
        ];
        match self.kind {
            Kind::Immediate => p.extend([0x36, 0xa6]),
            Kind::Ldi => p.extend([0xed, 0xa0]),
            Kind::BankM1 => p.extend([0xc3, 0x08, 0x80]),
            _ => p.push(0x77),
        }
        p.extend([0x76, 0x00]);
        p
    }
}

pub(super) fn access(lane: u32) -> BusAccess {
    BusAccess {
        logical_address: 0xffff_000e + lane,
        bus_address: 0xff000e,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes: if lane == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        },
    }
}

pub(super) fn seed(concurrent: bool, case: &Case) -> (Emulator, Option<oracle::Oracle>) {
    seed_mode(concurrent, case, false, false)
}

pub(super) fn seed_mode(
    concurrent: bool,
    case: &Case,
    completed: bool,
    deferred: bool,
) -> (Emulator, Option<oracle::Oracle>) {
    seed_mode_with_ram_bank(concurrent, case, completed, deferred, 0x1ff)
}

pub(super) fn seed_ram_mode(
    concurrent: bool,
    case: &Case,
    completed: bool,
    deferred: bool,
) -> (Emulator, Option<oracle::Oracle>) {
    seed_mode_with_ram_bank(concurrent, case, completed, deferred, 0x1fe)
}

fn seed_mode_with_ram_bank(
    concurrent: bool,
    case: &Case,
    completed: bool,
    deferred: bool,
    ram_bank: u16,
) -> (Emulator, Option<oracle::Oracle>) {
    let mut program = vec![0x4e71; case.nops()];
    program.extend([
        0x4ad0, 0x4e71, 0x4e71, 0x4e71, 0x4e71, 0x4e71, 0x4e71, 0x4e71, 0x4e71, 0x4e71, 0x60fe,
        0x4e71,
    ]);
    let mut machine = super::super::super::machine(&program);
    machine.bus.ram.fill(0xde);
    machine.bus.ram[0x0e + case.lane as usize] = 0;
    machine.bus.audio.write_psg(56, 0x80);
    machine.bus.audio.write_psg(56, 0x01);
    machine.bus.audio.write_psg(56, 0x90);
    machine.bus.sound.bank = match case.kind {
        Kind::BankM1 => 0,
        Kind::BankPsg => 0x180,
        _ => ram_bank,
    };
    let sound = case.program();
    machine.bus.sound.ram[..sound.len()].copy_from_slice(&sound);
    machine.bus.sound.ram[0x1000] = 0xa6;
    let mut prepared = machine.clone();
    prepared.bus.sound.set_reset(false);
    for _ in 0..case.nops() {
        machine.step_instruction().unwrap();
    }
    machine.concurrent_sound = concurrent;
    machine.cpu.registers.a[0] = access(case.lane).logical_address;
    machine.cpu.registers.set_status_register(0x271f);
    assert_eq!(machine.bus.now.get(), case.a());
    assert_eq!(machine.cpu.clock(), machine.bus.cpu_clock);
    assert_eq!(case.a() % 15, case.residue);
    if !concurrent {
        return (machine, None);
    }
    let mut expected = oracle::Oracle::new(&prepared, case);
    let end = if completed { case.s() + 45 } else { case.a() };
    let occupied = deferred.then_some((case.s(), case.s() + 15, true));
    expected.advance_sound(case.origin(), end, occupied);
    expected.clear_preparation_checkpoint();
    let reservations = if deferred {
        vec![Reservation {
            requester: Requester::VDP,
            ticks: case.s()..case.s() + 15,
        }]
    } else {
        Vec::new()
    };
    let mut interval = SoundInterval::new(case.origin()..end, &mut prepared.bus.audio);
    interval.occupied = &reservations;
    prepared.sound_schedule.advance(
        &mut prepared.sound_cpu,
        &mut prepared.bus.sound,
        &mut prepared.bus.cartridge,
        &mut prepared.bus.ram,
        interval,
    );
    if completed {
        assert!(prepared.sound_schedule.pending.is_none());
        prepared.bus.sound.request(true);
        expected.bus.sound.request(true);
        expected.advance_sound(end, case.a(), None);
        prepared.sound_schedule.advance(
            &mut prepared.sound_cpu,
            &mut prepared.bus.sound,
            &mut prepared.bus.cartridge,
            &mut prepared.bus.ram,
            SoundInterval::new(end..case.a(), &mut prepared.bus.audio),
        );
        assert!(prepared.sound_schedule.pending.is_none());
    }
    machine.sound_cpu = prepared.sound_cpu;
    machine.sound_schedule = prepared.sound_schedule;
    machine.bus.sound = prepared.bus.sound;
    machine.bus.ram = prepared.bus.ram;
    expected.bus.now = machine.bus.now;
    expected.bus.cpu_clock = machine.bus.cpu_clock;
    expected.bus.vdp = machine.bus.vdp.clone();
    expected.bus.video = machine.bus.video.clone();
    assert_eq!(machine.bus.audio, expected.bus.audio);
    expected.check(&ScheduledBus::new(
        &mut machine.bus,
        &mut machine.sound_cpu,
        &mut machine.sound_schedule,
    ));
    (machine, Some(expected))
}

pub(super) struct Adapter<'a> {
    pub inner: ScheduledBus<'a>,
    pub concurrent: bool,
}

impl ClockedBus for Adapter<'_> {
    type Error = BusError;
    fn read_cycle(&mut self, a: BusAccess, t: CpuClocks) -> CycleResult<u16, BusError> {
        if self.concurrent {
            self.inner.read_cycle(a, t)
        } else {
            self.inner.bus.read_cycle(a, t)
        }
    }
    fn write_cycle(&mut self, a: BusAccess, v: u16, t: CpuClocks) -> CycleResult<(), BusError> {
        if self.concurrent {
            self.inner.write_cycle(a, v, t)
        } else {
            self.inner.bus.write_cycle(a, v, t)
        }
    }
    fn idle(&mut self, t: CpuClocks, n: u32) {
        if self.concurrent {
            self.inner.idle(t, n);
        } else {
            self.inner.bus.idle(t, n);
        }
    }
    fn begin_rmw(&mut self, a: BusAccess, t: CpuClocks) -> Result<BusResponse<()>, BusError> {
        if self.concurrent {
            self.inner.begin_rmw(a, t)
        } else {
            self.inner.bus.begin_rmw(a, t)
        }
    }
    fn end_rmw(&mut self, t: CpuClocks) {
        if self.concurrent {
            self.inner.end_rmw(t);
        } else {
            self.inner.bus.end_rmw(t);
        }
    }
    fn acknowledge_cycle(
        &mut self,
        a: BusAccess,
        t: CpuClocks,
    ) -> CycleResult<BusResponse<zeff_m68k::timing::InterruptVector>, BusError> {
        if self.concurrent {
            self.inner.acknowledge_cycle(a, t)
        } else {
            self.inner.bus.acknowledge_cycle(a, t)
        }
    }
}

pub(super) fn adapter(machine: &mut Emulator) -> Adapter<'_> {
    Adapter {
        concurrent: machine.concurrent_sound,
        inner: ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        ),
    }
}

pub(super) fn advance(machine: &mut Emulator, ticks: u64) {
    if machine.concurrent_sound {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .advance_machine(ticks)
        .unwrap();
    } else {
        machine.bus.advance_machine(ticks).unwrap();
    }
}

pub(super) fn complete_cpu(
    registers: zeff_m68k::Registers,
    queue: [u16; 2],
    clock: u64,
) -> PrefetchCpu {
    PrefetchCpu::from_prefetched(registers, queue, CpuClocks(clock)).unwrap()
}

pub(super) fn frozen(machine: &mut Emulator, calls: &mut usize) {
    let before = machine.clone();
    let initial = *calls;
    struct Rejected<'a>(&'a mut usize);
    impl ClockedBus for Rejected<'_> {
        type Error = BusError;
        fn read_cycle(&mut self, _: BusAccess, _: CpuClocks) -> CycleResult<u16, BusError> {
            *self.0 += 1;
            panic!("frozen read")
        }
        fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), BusError> {
            *self.0 += 1;
            panic!("frozen write")
        }
        fn idle(&mut self, _: CpuClocks, _: u32) {
            *self.0 += 1;
            panic!("frozen idle");
        }
        fn begin_rmw(&mut self, _: BusAccess, _: CpuClocks) -> Result<BusResponse<()>, BusError> {
            *self.0 += 1;
            panic!("frozen begin")
        }
        fn end_rmw(&mut self, _: CpuClocks) {
            *self.0 += 1;
            panic!("frozen end");
        }
    }
    let mut rejected = Rejected(calls);
    assert_eq!(
        machine.cpu.step_instruction(&mut rejected),
        Err(TimingFault::Frozen)
    );
    assert_eq!(
        machine.cpu.step_boundary(&mut rejected),
        Err(TimingFault::Frozen)
    );
    assert_eq!(
        machine.cpu.service_pending_trace(&mut rejected),
        Err(TimingFault::Frozen)
    );
    assert_eq!(*machine, before);
    assert_eq!(*calls, initial);
}

pub(super) fn future(machine: &mut Emulator, mut expected: oracle::Oracle) {
    let mut prediction = expected.clone();
    prediction.future(machine.bus.now.get() + 900);
    let deadline = prediction.target.unwrap().1;
    if deadline > machine.bus.now.get() {
        let ticks = deadline - machine.bus.now.get() - 1;
        expected.future(deadline - 1);
        advance(machine, ticks);
        expected.check(&adapter(machine).inner);
        expected.future(deadline);
        advance(machine, 1);
        expected.check(&adapter(machine).inner);
    }
    let end = machine.bus.now.get() + 180;
    expected.future(end);
    advance(machine, 180);
    expected.check(&adapter(machine).inner);
    assert_eq!(expected.target_publications, 1);
    assert!(machine.bus.rmw.is_none());
    assert!(machine.sound_schedule.rmw_checkpoint.is_none());
}

pub(super) fn hardware_equal(a: &Emulator, b: &Emulator) {
    assert_eq!(a.cpu, b.cpu);
    assert_eq!(a.bus, b.bus);
    assert_eq!(a.sound_cpu, b.sound_cpu);
    assert_eq!(a.sound_schedule, b.sound_schedule);
}
