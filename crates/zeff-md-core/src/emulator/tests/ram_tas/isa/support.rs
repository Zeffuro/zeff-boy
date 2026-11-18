use super::*;

pub(super) const OLD_BYTES: [u8; 5] = [0, 1, 0x7f, 0x80, 0xff];
pub(super) const QUEUE: [u16; 2] = [0x7201, 0x7402];

pub(super) fn seed(
    opcode: u16,
    extensions: &[u16],
    concurrent: bool,
    supervisor: bool,
    extend: bool,
) -> Emulator {
    let program: Vec<_> = [opcode]
        .into_iter()
        .chain(extensions.iter().copied())
        .chain([0x7201, 0x7402, 0x7603])
        .collect();
    let mut machine = super::super::super::machine(&program);
    machine.concurrent_sound = concurrent;
    machine.bus.ram.fill(0x5a);
    for register in 0..8 {
        machine.cpu.registers.d[register] = 0x1020_3040 + register as u32 * 0x0101_0101;
    }
    for register in 0..7 {
        machine.cpu.registers.a[register] = 0xffff_3000 + register as u32 * 0x100;
    }
    machine.cpu.registers.usp = 0x1111_8888;
    machine.cpu.registers.ssp = 0x2222_9998;
    machine.cpu.registers.set_status_register(
        0x070f | if supervisor { 0x2000 } else { 0 } | if extend { 0x10 } else { 0 },
    );
    assert_eq!(machine.cpu.clock(), CpuClocks(8));
    assert_eq!(machine.bus.now.get(), 56);
    machine
}

pub(super) fn access(logical: u32, supervisor: bool, word: bool) -> BusAccess {
    BusAccess {
        logical_address: logical,
        bus_address: logical & 0x00ff_fffe,
        function_code: if supervisor {
            FunctionCode::SupervisorData
        } else {
            FunctionCode::UserData
        },
        kind: AccessKind::Data,
        lanes: if word {
            ByteLanes::Both
        } else if logical & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        },
    }
}

pub(super) fn fetch(logical: u32, clock: u64, supervisor: bool) -> Event {
    Event::Read(
        BusAccess {
            logical_address: logical,
            bus_address: logical & 0x00ff_fffe,
            function_code: if supervisor {
                FunctionCode::SupervisorProgram
            } else {
                FunctionCode::UserProgram
            },
            kind: AccessKind::Prefetch,
            lanes: ByteLanes::Both,
        },
        clock,
        0,
    )
}

pub(super) fn packed(logical: u32, value: u16, word: bool) -> u16 {
    if word {
        value
    } else if logical & 1 == 0 {
        (value & 255) << 8
    } else {
        value & 255
    }
}

pub(super) fn tas_status(before: u16, old: u8) -> u16 {
    (before & !0x000f)
        | match old {
            0 => 4,
            0x80..=0xff => 8,
            _ => 0,
        }
}

fn step_on<B: ClockedBus>(
    cpu: &mut PrefetchCpu,
    inner: &mut B,
) -> (zeff_m68k::timing::TimedStep, Vec<Event>)
where
    B::Error: std::fmt::Debug,
{
    let mut observed = Observed {
        inner,
        events: Vec::new(),
    };
    let step = cpu.step_instruction(&mut observed).unwrap();
    assert_eq!(step.exception, None);
    (step, observed.events)
}

pub(super) fn run(machine: &mut Emulator) -> (zeff_m68k::timing::TimedStep, Vec<Event>) {
    let sound = machine.bus.sound.clone();
    let sound_cpu = machine.sound_cpu.clone();
    assert!(sound.reset_asserted);
    let result = if machine.concurrent_sound {
        step_on(
            &mut machine.cpu,
            &mut CompactBus(ScheduledBus::new(
                &mut machine.bus,
                &mut machine.sound_cpu,
                &mut machine.sound_schedule,
            )),
        )
    } else {
        step_on(&mut machine.cpu, &mut machine.bus)
    };
    assert_eq!(machine.bus.sound, sound);
    assert_eq!(machine.sound_cpu, sound_cpu);
    result
}

pub(super) fn boundary(machine: &Emulator, expected: Registers, clocks: u64) {
    let cpu = PrefetchCpu::from_prefetched(expected, QUEUE, CpuClocks(8 + clocks)).unwrap();
    assert_eq!(machine.cpu, cpu);
    assert_eq!(machine.bus.cpu_clock, CpuClocks(8 + clocks));
    assert_eq!(machine.bus.now.get(), (8 + clocks) * 7);
    assert!(!machine.cpu.trace_pending());
    assert!(!machine.bus.clock_fault);
    assert!(!machine.bus.reset_output);
    assert!(machine.bus.rmw.is_none());
    assert!(machine.sound_schedule.rmw_checkpoint.is_none());
    assert!(machine.sound_schedule.pending.is_none());
    assert!(machine.sound_schedule.fault.is_none());
}

pub(super) struct MemoryExpectation {
    pub opcode: u16,
    pub logical: u32,
    pub old: u8,
    pub prefix: Vec<Event>,
    pub begin: u64,
    pub clocks: u64,
    pub registers: Registers,
}

pub(super) fn memory_tas(machine: &mut Emulator, mut expected: MemoryExpectation) {
    let ram = machine.bus.ram.clone();
    let supervisor = expected.registers.supervisor();
    let data = access(expected.logical, supervisor, false);
    expected.prefix.extend([
        Event::Begin(data, expected.begin),
        Event::Read(data, expected.begin, 0),
        Event::Idle(expected.begin + 4, 2),
        Event::Write(
            data,
            packed(expected.logical, u16::from(expected.old | 0x80), false),
            expected.begin + 6,
            0,
        ),
        Event::End(expected.begin + 10),
        fetch(expected.registers.pc + 2, expected.begin + 10, supervisor),
    ]);
    let (step, events) = run(machine);
    assert_eq!(step.opcode, expected.opcode);
    assert_eq!(step.clocks, expected.clocks);
    assert_eq!(events, expected.prefix);
    assert_eq!(machine.bus.ram, ram);
    boundary(machine, expected.registers, expected.clocks);
}
