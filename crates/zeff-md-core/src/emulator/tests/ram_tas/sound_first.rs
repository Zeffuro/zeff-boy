use super::*;
use crate::hardware::bus::ownership::Requester;
use zeff_m68k::bus::Direction;

type PendingRow = (u64, u64, Option<(u32, Direction, u8)>, bool);

#[derive(Debug, PartialEq, Eq)]
struct Row {
    phase: &'static str,
    master: u64,
    main: u64,
    sound: u64,
    value: u8,
    waits: u32,
    owner: bool,
    reservation: Option<(u64, u64)>,
    pending: Option<PendingRow>,
    checkpoint: bool,
}

struct Trace<'a> {
    inner: ScheduledBus<'a>,
    rows: Vec<Row>,
    future: Option<(
        crate::hardware::bus::Bus,
        zeff_z80::Cpu,
        crate::hardware::sound::schedule::SoundSchedule,
    )>,
}

impl Trace<'_> {
    fn record(&mut self, phase: &'static str, waits: u32) {
        assert!(self.inner.schedule.epoch.is_none());
        let reservation = self
            .inner
            .bus
            .rmw
            .as_ref()
            .and_then(|scope| scope.reservation.as_ref())
            .map(|slot| {
                assert_eq!(slot.requester, Requester::MAIN_RMW);
                (slot.ticks.start, slot.ticks.end)
            });
        let pending = self.inner.schedule.pending.as_ref().map(|pending| {
            (
                pending.start,
                pending.deadline,
                pending
                    .bank_access
                    .as_ref()
                    .map(|bank| (bank.address, bank.direction, bank.value)),
                pending.any_bank_access,
            )
        });
        self.rows.push(Row {
            phase,
            master: self.inner.bus.now.get(),
            main: self.inner.bus.cpu_clock.0,
            sound: self.inner.cpu.cycles(),
            value: self.inner.bus.ram[0x8000],
            waits,
            owner: self.inner.bus.rmw.is_some(),
            reservation,
            pending,
            checkpoint: self.inner.schedule.rmw_checkpoint.is_some(),
        });
    }
}

impl ClockedBus for Trace<'_> {
    type Error = BusError;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, BusError> {
        let report = self.inner.read_cycle(access, start);
        self.record(
            if access.kind == AccessKind::Data {
                assert_eq!(access.logical_address, 0xffff_8000);
                assert_eq!(access.bus_address, 0xff8000);
                assert_eq!(access.lanes, ByteLanes::Upper);
                assert_eq!(report.result, Ok(0x55de));
                "read"
            } else {
                "prefetch"
            },
            report.wait_clocks,
        );
        report
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), BusError> {
        assert_eq!(access.logical_address, 0xffff_8000);
        assert_eq!(access.bus_address, 0xff8000);
        assert_eq!(access.lanes, ByteLanes::Upper);
        assert_eq!(value, 0xd500);
        let report = self.inner.write_cycle(access, value, start);
        assert_eq!(report.result, Ok(()));
        self.record("write", report.wait_clocks);
        self.future = Some((
            self.inner.bus.clone(),
            self.inner.cpu.clone(),
            self.inner.schedule.clone(),
        ));
        report
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(clocks, 2);
        self.inner.idle(start, clocks);
        self.record("idle", 0);
    }

    fn begin_rmw(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> Result<BusResponse<()>, BusError> {
        let result = self.inner.begin_rmw(access, start);
        assert_eq!(result, Ok(BusResponse::Complete(())));
        self.record("begin", 0);
        result
    }

    fn end_rmw(&mut self, end: CpuClocks) {
        self.inner.end_rmw(end);
        self.record("end", 0);
    }
}

fn expected(
    phase: &'static str,
    clocks: (u64, u64, u64),
    value: u8,
    pending: (u64, u64, Option<u8>, bool),
    reservation: Option<(u64, u64)>,
    waits: u32,
) -> Row {
    Row {
        phase,
        master: clocks.0,
        main: clocks.1,
        sound: clocks.2,
        value,
        waits,
        owner: matches!(phase, "begin" | "read" | "idle" | "write"),
        reservation,
        pending: Some((
            pending.0,
            pending.1,
            pending.2.map(|value| (0xff8000, Direction::Write, value)),
            pending.3,
        )),
        checkpoint: matches!(phase, "idle" | "write") && pending.2 == Some(0xa6),
    }
}

struct Case {
    nops: usize,
    program: &'static [u8],
    initial: (u32, u64, u64, u64),
    instruction_clocks: u64,
    final_pc: u32,
    final_clock: u64,
    rows: [Row; 8],
    publication: (u64, u64),
}

fn run(case: Case) {
    let mut program = vec![0x4e71; case.nops];
    program.extend([0x4af9, 0xffff, 0x8000, 0x4e71, 0x4e71]);
    let mut machine = super::super::machine(&program);
    machine.concurrent_sound = true;
    machine.bus.ram.fill(0xde);
    machine.bus.ram[0x8000] = 0;
    machine.bus.sound.bank = 0x1ff;
    machine.bus.sound.ram[..case.program.len()].copy_from_slice(case.program);
    machine.bus.sound.set_reset(false);
    for _ in 0..case.nops {
        machine.step_instruction().unwrap();
    }
    assert_eq!(machine.cpu.registers.pc, case.initial.0);
    assert_eq!(machine.cpu.clock(), CpuClocks(case.initial.1));
    assert_eq!(machine.bus.now.get(), case.initial.2);
    assert_eq!(machine.bus.ram[0x8000], 0);
    assert_eq!(machine.sound_cpu.cycles(), case.initial.3);
    assert!(machine.sound_schedule.epoch.is_none());
    machine.cpu.registers.set_status_register(0x271f);
    let mut registers = machine.cpu.registers;
    registers.pc = case.final_pc;
    registers.set_status_register(0x2710);
    let mut ram = machine.bus.ram.clone();
    ram[0x8000] = 0x55;
    let mut trace = Trace {
        inner: ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        ),
        rows: Vec::new(),
        future: None,
    };
    let report = machine.cpu.step_instruction(&mut trace).unwrap();
    assert_eq!(report.opcode, 0x4af9);
    assert_eq!(report.clocks, case.instruction_clocks);
    assert_eq!(trace.rows, case.rows);
    let (mut bus, mut cpu, mut schedule) = trace.future.take().unwrap();
    drop(trace);
    assert_eq!(machine.cpu.registers, registers);
    assert_eq!(machine.cpu.clock(), CpuClocks(case.final_clock));
    assert_eq!(machine.cpu.queued_words(), Some([0x4e71, 0x4e71]));
    let mut completed_ram = ram.clone();
    completed_ram[0x8000] = 0xa6;
    assert_eq!(machine.bus.ram, completed_ram);
    assert!(machine.bus.rmw.is_none());
    assert!(machine.sound_schedule.rmw_checkpoint.is_none());
    assert!(machine.sound_schedule.epoch.is_none());
    assert_eq!(machine.sound_fault(), None);
    machine.encode_state().unwrap();

    let release = bus.cpu_clock;
    ScheduledBus::new(&mut bus, &mut cpu, &mut schedule).end_rmw(release);
    let before = case.publication.0 - bus.now.get() - 1;
    let sound_before = cpu.cycles();
    ScheduledBus::new(&mut bus, &mut cpu, &mut schedule)
        .advance_machine(before)
        .unwrap();
    assert_eq!(bus.now.get(), case.publication.0 - 1);
    assert_eq!(cpu.cycles(), sound_before);
    assert_eq!(bus.ram, ram);
    assert!(schedule.epoch.is_none());
    assert_eq!(schedule.fault, None);
    ScheduledBus::new(&mut bus, &mut cpu, &mut schedule)
        .advance_machine(1)
        .unwrap();
    ram[0x8000] = 0xa6;
    assert_eq!(bus.now.get(), case.publication.0);
    assert_eq!(cpu.cycles(), case.publication.1);
    assert_eq!(bus.ram, ram);
    assert!(schedule.pending.is_none());
    assert!(bus.rmw.is_none());
    assert!(schedule.rmw_checkpoint.is_none());
    assert!(schedule.epoch.is_none());
    assert_eq!(schedule.fault, None);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn actual_cpu_ram_tas_grants_unclassified_prior_write_and_holds_unclassified_future_write() {
    let prior = (480, 525, Some(0x55), true);
    let local = (525, 585, None, false);
    let future = (630, 675, Some(0xa6), true);
    let halted = (675, 735, None, false);
    let hold = Some((553, 623));
    run(Case {
        nops: 14,
        program: &[0x21, 0x00, 0x80, 0x3e, 0x55, 0x06, 0xa6, 0x77, 0x70, 0x76],
        initial: (0x11c, 64, 448, 24),
        instruction_clocks: 37,
        final_pc: 0x122,
        final_clock: 101,
        rows: [
            expected("prefetch", (476, 68, 28), 0, prior, None, 0),
            expected("prefetch", (553, 79, 32), 0x55, local, None, 7),
            expected("begin", (553, 79, 32), 0x55, local, None, 0),
            expected("read", (581, 83, 32), 0x55, local, hold, 0),
            expected("idle", (595, 85, 36), 0x55, future, hold, 0),
            expected("write", (623, 89, 36), 0x55, future, hold, 0),
            expected("end", (623, 89, 36), 0x55, future, None, 0),
            expected("prefetch", (707, 101, 42), 0xa6, halted, None, 8),
        ],
        publication: (675, 42),
    });
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn actual_cpu_ram_tas_grants_qualified_prior_write_and_holds_unclassified_future_write() {
    let immediate = (465, 510, None, false);
    let prior = (540, 585, Some(0x55), true);
    let local = (585, 645, None, false);
    let future = (660, 705, Some(0xa6), true);
    let halted = (705, 765, None, false);
    let hold = Some((588, 658));
    run(Case {
        nops: 15,
        program: &[
            0x21, 0x00, 0x80, 0x3e, 0x55, 0x06, 0xa6, 0x36, 0x55, 0x70, 0x76,
        ],
        initial: (0x11e, 68, 476, 28),
        instruction_clocks: 37,
        final_pc: 0x124,
        final_clock: 105,
        rows: [
            expected("prefetch", (504, 72, 28), 0, immediate, None, 0),
            expected("prefetch", (532, 76, 31), 0, prior, None, 0),
            expected("begin", (532, 76, 31), 0, prior, None, 0),
            expected("read", (616, 88, 36), 0x55, local, hold, 8),
            expected("idle", (630, 90, 36), 0x55, local, hold, 0),
            expected("write", (658, 94, 40), 0x55, future, hold, 0),
            expected("end", (658, 94, 40), 0x55, future, None, 0),
            expected("prefetch", (735, 105, 44), 0xa6, halted, None, 7),
        ],
        publication: (705, 44),
    });
}
