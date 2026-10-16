use super::{ALLOCATIONS, MemoryBus, black_box};
use std::sync::atomic::Ordering;
use zeff_m68k::Registers;
use zeff_m68k::bus::{BusAccess, Direction, M68kBus};
use zeff_m68k::timing::physical::{EntryBus, EntryOutcome, EntryReport};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionFault, InstructionOutcome, InstructionReport,
    PhysicalInstructionStep,
};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

pub(super) struct Device<'a> {
    pub(super) memory: &'a mut MemoryBus,
    pub(super) clock: u64,
    pub(super) transfers: usize,
    pub(super) failure: Option<(usize, u8)>,
    pub(super) odd: bool,
}

impl Device<'_> {
    fn advance(&mut self, start: CpuClocks, elapsed: u32) {
        assert_eq!(self.clock, start.0);
        self.clock += u64::from(elapsed);
    }
    fn report<T>(&mut self, start: CpuClocks, value: T) -> InstructionReport<T, ()> {
        self.advance(start, 7);
        let index = self.transfers;
        self.transfers += 1;
        InstructionReport {
            elapsed: 7,
            outcome: match self.failure.filter(|(at, _)| *at == index) {
                Some((_, 0)) => InstructionOutcome::HostFailure(()),
                Some((_, 1)) => InstructionOutcome::AtGroup0Entry,
                Some(_) => InstructionOutcome::TimingUnavailable,
                None => InstructionOutcome::Complete(value),
            },
        }
    }
}

impl EntryBus for Device<'_> {
    type Error = ();
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, ()> {
        self.advance(start, 5);
        EntryReport {
            elapsed: 5,
            outcome: EntryOutcome::Complete(self.memory.read(access).unwrap()),
        }
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), ()> {
        self.advance(start, 5);
        self.memory.write(access, value).unwrap();
        EntryReport {
            elapsed: 5,
            outcome: EntryOutcome::Complete(()),
        }
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.advance(start, clocks);
    }
}

impl InstructionBus for Device<'_> {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, ()> {
        assert!(!self.odd || access.logical_address & 1 == 0);
        let value = self.memory.read(access).unwrap();
        self.report(start, value)
    }
    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> InstructionReport<(), ()> {
        assert!(!self.odd);
        self.memory.write(access, value).unwrap();
        self.report(start, ())
    }
    fn detect_instruction_address_error(
        &mut self,
        access: BusAccess,
        _: Direction,
        start: CpuClocks,
    ) -> InstructionReport<(), ()> {
        assert!(self.odd && access.logical_address & 1 != 0);
        self.advance(start, 8);
        InstructionReport {
            elapsed: 8,
            outcome: InstructionOutcome::AtGroup0Entry,
        }
    }
}

pub(super) fn update_steps(memory: &mut MemoryBus) {
    for vector in [2, 3] {
        memory.word(vector * 4, 0);
        memory.word(vector * 4 + 2, 0x1000);
    }
    memory.word(0x1000, 0x4e71);
    memory.word(0x1002, 0x7201);
    memory.word(0x2004, 0x7480);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut checked = 0;
    for _ in 0..100 {
        for opcode in [
            0x101f, 0x301f, 0x201f, 0x1027, 0x3027, 0x2027, 0x1ec0, 0x3ec0, 0x2ec0, 0x1f00, 0x3f00,
            0x2f00,
        ] {
            let count = if opcode >> 12 == 2 { 3 } else { 2 };
            let pre_load = (opcode >> 3) & 7 == 4;
            let pre_store = (opcode >> 6) & 7 == 4;
            for failure in [None, Some((0, 0)), Some((0, 1)), Some((0, 2))]
                .into_iter()
                .chain(
                    (1..count).flat_map(|index| (0..3).map(move |outcome| Some((index, outcome)))),
                )
            {
                let mut registers = Registers::new();
                registers.pc = 0x2000;
                registers.usp = 0x3000;
                registers.ssp = 0x5000;
                registers.d[0] = 0x8000_0001;
                registers.set_status_register(0x001f);
                for address in [0x2ffc, 0x2ffe, 0x3000, 0x3002] {
                    memory.word(address, 0x8001);
                }
                let mut cpu =
                    PrefetchCpu::from_prefetched(registers, [opcode, 0x7201], CpuClocks(17))
                        .unwrap();
                let mut bus = Device {
                    memory,
                    clock: 17,
                    transfers: 0,
                    failure,
                    odd: false,
                };
                let result = black_box(cpu.step_physical_instruction(&mut bus));
                assert_eq!(result.is_ok(), failure.is_none_or(|(_, kind)| kind == 1));
                if failure.is_none() {
                    assert!(
                        matches!(result, Ok(PhysicalInstructionStep::Instruction(step)) if step.clocks == count as u64 * 7 + if pre_load { 2 } else { 0 })
                    );
                } else if failure.is_some_and(|(_, kind)| kind == 2) {
                    assert!(matches!(
                        result,
                        Err(InstructionFault::TransitionTimingUnavailable { .. })
                    ));
                }
                assert_eq!(cpu.clock().0, bus.clock);
                checked += 1;
            }
            if opcode >> 12 != 1 {
                let mut registers = Registers::new();
                registers.pc = 0x2000;
                registers.usp = 0x3001;
                registers.ssp = 0x5000;
                registers.d[0] = 0x8000_0001;
                registers.set_status_register(0x001f);
                let mut cpu =
                    PrefetchCpu::from_prefetched(registers, [opcode, 0x7201], CpuClocks(17))
                        .unwrap();
                let mut bus = Device {
                    memory,
                    clock: 17,
                    transfers: 0,
                    failure: None,
                    odd: true,
                };
                let clocks = 69
                    + if pre_load {
                        2
                    } else if pre_store {
                        7
                    } else {
                        0
                    };
                assert!(matches!(black_box(cpu.step_physical_instruction(&mut bus)),
                    Ok(PhysicalInstructionStep::Group0 { clocks: actual, .. }) if actual == clocks));
                assert_eq!(bus.transfers, usize::from(pre_store));
                assert_eq!(cpu.clock().0, bus.clock);
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 10_400);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
