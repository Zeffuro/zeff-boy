use super::*;
use common::*;
use zeff_m68k::bus::Direction;

fn shifted(
    original: &crate::hardware::sound::schedule::PendingSound,
    waits: u32,
) -> crate::hardware::sound::schedule::PendingSound {
    let mut expected = original.clone();
    expected.start += u64::from(waits) * 15;
    expected.deadline += u64::from(waits) * 15;
    assert!(
        expected
            .cpu
            .extend_bus_segment(&mut expected.instruction, waits)
    );
    expected
}

struct Probe<'a> {
    inner: Adapter<'a>,
    original: crate::hardware::sound::schedule::PendingSound,
    completed_cpu: zeff_z80::Cpu,
    hardware: crate::hardware::bus::Bus,
    idle: bool,
    events: Vec<&'static str>,
}

impl Probe<'_> {
    fn check_hardware(&self) {
        let bus = &self.inner.inner.bus;
        assert_eq!(bus.ram, self.hardware.ram);
        assert_eq!(bus.sound, self.hardware.sound);
        assert_eq!(bus.cartridge, self.hardware.cartridge);
        assert_eq!(bus.audio, self.hardware.audio);
        assert_eq!(bus.vdp, self.hardware.vdp);
        assert_eq!(bus.video, self.hardware.video);
        assert_eq!(bus.now, self.hardware.now);
        assert_eq!(bus.cpu_clock, self.hardware.cpu_clock);
    }

    fn check_held(&self) {
        self.check_hardware();
        let schedule = &self.inner.inner.schedule;
        assert_eq!(*self.inner.inner.cpu, self.completed_cpu);
        assert_eq!(schedule.pending.as_ref(), Some(&shifted(&self.original, 4)));
        assert_eq!(schedule.rmw_checkpoint.as_deref(), Some(&self.original));
        let lock = self
            .inner
            .inner
            .bus
            .rmw
            .as_ref()
            .unwrap()
            .reservation
            .as_ref()
            .unwrap();
        assert_eq!(lock.requester, Requester::MAIN_RMW);
        assert_eq!(lock.ticks, 1260..1330);
    }
}

impl ClockedBus for Probe<'_> {
    type Error = BusError;

    fn begin_rmw(&mut self, a: BusAccess, t: CpuClocks) -> Result<BusResponse<()>, BusError> {
        assert_eq!(a, access(0));
        assert_eq!(t, CpuClocks(180));
        self.events.push("begin");
        let result = self.inner.begin_rmw(a, t);
        assert_eq!(result, Ok(BusResponse::Complete(())));
        self.check_hardware();
        result
    }

    fn read_cycle(&mut self, a: BusAccess, t: CpuClocks) -> CycleResult<u16, BusError> {
        assert_eq!(a, access(0));
        assert_eq!(t, CpuClocks(180));
        self.events.push("read");
        let mut result = self.inner.read_cycle(a, t);
        assert_eq!(result, CycleResult::ready(0x00de));
        self.hardware.advance_machine(28).unwrap();
        self.hardware.cpu_clock.0 += 4;
        self.check_held();
        if !self.idle {
            result.result = Err(BusError::UnsupportedAccess {
                address: a.bus_address,
                direction: Direction::Read,
            });
        }
        result
    }

    fn idle(&mut self, t: CpuClocks, n: u32) {
        assert!(self.idle);
        assert_eq!((t, n), (CpuClocks(184), 2));
        self.events.push("idle");
        self.inner.idle(t, n);
        self.hardware.advance_machine(14).unwrap();
        self.hardware.cpu_clock.0 += 2;
        self.check_held();
    }

    fn write_cycle(&mut self, a: BusAccess, v: u16, t: CpuClocks) -> CycleResult<(), BusError> {
        assert!(self.idle);
        assert_eq!((a, v, t), (access(0), 0x8000, CpuClocks(186)));
        self.events.push("write");
        self.check_held();
        CycleResult {
            result: Err(BusError::UnsupportedAccess {
                address: a.bus_address,
                direction: Direction::Write,
            }),
            wait_clocks: 0,
        }
    }

    fn end_rmw(&mut self, t: CpuClocks) {
        assert_eq!(t, CpuClocks(if self.idle { 190 } else { 184 }));
        self.events.push("end");
        self.inner.end_rmw(t);
        self.check_hardware();
        let schedule = &self.inner.inner.schedule;
        assert_eq!(
            schedule.pending.as_ref(),
            Some(&shifted(&self.original, if self.idle { 2 } else { 1 }))
        );
        assert_eq!(*self.inner.inner.cpu, self.completed_cpu);
        assert!(schedule.rmw_checkpoint.is_none());
        assert!(self.inner.inner.bus.rmw.is_none());
    }
}

fn seed_replay() -> Emulator {
    let (mut machine, _) = seed(
        false,
        &Case {
            kind: Kind::Immediate,
            residue: 0,
            edge: 0,
            lane: 0,
        },
    );
    machine.bus.cartridge =
        crate::cartridge::Cartridge::new(crate::cartridge::registers::tests::rom(0)).unwrap();
    machine
        .bus
        .cartridge
        .registers
        .as_mut()
        .unwrap()
        .write(0, 0x42);
    machine.bus.sound = SoundMemory::default();
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 0x80;
    machine.bus.sound.ram[..7].copy_from_slice(&[0x21, 0x00, 0x80, 0x3e, 0x55, 0x7e, 0x76]);
    let mut audio = crate::hardware::audio::Audio::default();
    audio.write_psg(56, 0x80);
    audio.write_psg(56, 0x01);
    audio.write_psg(56, 0x90);
    machine.sound_schedule.advance(
        &mut machine.sound_cpu,
        &mut machine.bus.sound,
        &mut machine.bus.cartridge,
        &mut machine.bus.ram,
        SoundInterval::new(960..1260, &mut audio),
    );
    assert_eq!(audio, machine.bus.audio);
    assert_eq!(machine.sound_cpu.cycles(), 17);
    assert_eq!(machine.sound_cpu.regs().a, 0x55);
    let pending = machine.sound_schedule.pending.as_ref().unwrap();
    assert_eq!((pending.start, pending.deadline), (1215, 1275));
    assert!(!pending.any_bank_access);
    assert_eq!(machine.sound_fault(), None);
    machine.concurrent_sound = true;
    machine
}

fn run(idle: bool, reset: bool) {
    let mut machine = seed_replay();
    let mut hardware = machine.bus.clone();
    let mut unreserved = machine.clone();
    advance(&mut unreserved, 16);
    let source_cpu = unreserved.sound_cpu.clone();
    let source_state = unreserved.sound_schedule.state;
    assert_eq!(source_cpu.cycles(), 21);
    let original = unreserved.sound_schedule.pending.as_ref().unwrap().clone();
    assert_eq!((original.start, original.deadline), (1275, 1320));
    assert_eq!(
        original.bank_access,
        Some(BankAccess {
            address: 0x400000,
            direction: Direction::Read,
            value: 0x42
        })
    );
    assert!(original.any_bank_access);
    let mut registers = machine.cpu.registers;
    let mut cpu = machine.cpu.clone();
    let mut probe = Probe {
        inner: adapter(&mut machine),
        original: original.clone(),
        completed_cpu: source_cpu.clone(),
        hardware: hardware.clone(),
        idle,
        events: Vec::new(),
    };
    assert!(cpu.step_instruction(&mut probe).is_err());
    assert_eq!(
        probe.events,
        if idle {
            vec!["begin", "read", "idle", "write", "end"]
        } else {
            vec!["begin", "read", "end"]
        },
    );
    drop(probe);
    registers.set_status_register(if idle { 0x2714 } else { 0x271f });
    assert_eq!(cpu.registers, registers);
    assert_eq!(cpu.queued_words(), Some([0x4ad0, 0x4e71]));
    assert_eq!(cpu.clock(), CpuClocks(if idle { 190 } else { 184 }));
    machine.cpu = cpu;
    let mut calls = 0;
    frozen(&mut machine, &mut calls);
    assert_eq!(machine.sound_cpu, source_cpu);

    machine
        .bus
        .cartridge
        .registers
        .as_mut()
        .unwrap()
        .write(0, 0xa6);
    hardware
        .cartridge
        .registers
        .as_mut()
        .unwrap()
        .write(0, 0xa6);
    machine.bus.sound.bank = 0x1ff;
    let waits = if idle { 2 } else { 1 };
    let deadline = if idle { 1350 } else { 1335 };
    let mut expected_memory = machine.bus.sound.clone();
    expected_memory.bank = 0x80;
    let mut expected_cpu = source_cpu.clone();
    let mut expected_state = source_state;
    let mut candidate = CandidateBus::new(&mut expected_memory, &machine.bus.cartridge);
    let mut instruction = expected_cpu
        .step_bus_segment(&mut candidate, &mut expected_state)
        .unwrap()
        .instruction;
    assert_eq!(candidate.fault.get(), None);
    assert_eq!(instruction.cycles, 3);
    assert!(expected_cpu.extend_bus_segment(&mut instruction, waits));
    assert_eq!(expected_cpu.regs().a, 0xa6);
    assert_eq!(expected_cpu.cycles(), if idle { 26 } else { 25 });

    if reset {
        machine.reset_cpu().unwrap();
        assert_eq!(machine.cpu.clock(), CpuClocks(15));
        assert_eq!(machine.cpu.registers.pc, 0x200);
        assert_eq!(machine.cpu.queued_words(), Some([0x60fe, 0x4e71]));
        assert_eq!(machine.bus.now.get(), if idle { 1407 } else { 1393 });
        let pending = machine.sound_schedule.pending.as_ref().unwrap();
        assert_eq!((pending.start, pending.deadline), (deadline, deadline + 60));
        assert!(!pending.any_bank_access);
    } else {
        let remaining = deadline - machine.bus.now.get();
        advance(&mut machine, remaining - 1);
        assert_eq!(machine.sound_cpu, source_cpu);
        assert_eq!(
            machine.sound_schedule.pending.as_ref(),
            Some(&shifted(&original, waits))
        );
        advance(&mut machine, 1);
        assert_eq!(machine.bus.now.get(), deadline);
        assert!(machine.sound_schedule.pending.is_none());
    }
    assert_eq!(machine.sound_cpu, expected_cpu);
    assert_eq!(machine.sound_schedule.state, expected_state);
    assert_eq!(machine.bus.sound, expected_memory);
    assert_eq!(machine.sound_fault(), None);
    assert!(machine.bus.rmw.is_none());
    assert!(machine.sound_schedule.rmw_checkpoint.is_none());
    hardware
        .advance_machine(machine.bus.now.get() - hardware.now.get())
        .unwrap();
    assert_eq!(machine.bus.ram, hardware.ram);
    assert_eq!(machine.bus.cartridge, hardware.cartridge);
    assert_eq!(machine.bus.audio, hardware.audio);
    assert_eq!(machine.bus.vdp, hardware.vdp);
    assert_eq!(machine.bus.video, hardware.video);
}

pub(super) fn rows() {
    for idle in [false, true] {
        for reset in [false, true] {
            run(idle, reset);
        }
    }
}
