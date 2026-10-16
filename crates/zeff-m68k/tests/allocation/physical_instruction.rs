use super::{ALLOCATIONS, MemoryBus, black_box};
use std::sync::atomic::Ordering;
use zeff_m68k::Registers;
use zeff_m68k::bus::{BusAccess, Direction, M68kBus};
use zeff_m68k::timing::physical::{
    EntryBus, EntryOutcome, EntryReport, ExternalResetBoundary, ResetQualification,
};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionFault, InstructionOutcome, InstructionReport,
    PhysicalInstructionStep,
};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

struct Device<'a> {
    memory: &'a mut MemoryBus,
    clock: u64,
    reads: usize,
    fault: Option<(usize, u8)>,
}

impl Device<'_> {
    fn advance(&mut self, start: CpuClocks) {
        assert_eq!(self.clock, start.0);
        self.clock += 7;
    }
}

impl EntryBus for Device<'_> {
    type Error = ();
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, ()> {
        self.advance(start);
        EntryReport {
            elapsed: 7,
            outcome: EntryOutcome::Complete(self.memory.read(access).unwrap()),
        }
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), ()> {
        self.advance(start);
        self.memory.write(access, value).unwrap();
        EntryReport {
            elapsed: 7,
            outcome: EntryOutcome::Complete(()),
        }
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(self.clock, start.0);
        self.clock += u64::from(clocks);
    }
}

impl InstructionBus for Device<'_> {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, ()> {
        self.advance(start);
        let index = self.reads;
        self.reads += 1;
        InstructionReport {
            elapsed: 7,
            outcome: match self.fault.filter(|(at, _)| *at == index) {
                Some((_, 0)) => InstructionOutcome::HostFailure(()),
                Some((_, 1)) => InstructionOutcome::AtGroup0Entry,
                Some(_) => InstructionOutcome::TimingUnavailable,
                None => InstructionOutcome::Complete(self.memory.read(access).unwrap()),
            },
        }
    }
    fn write_instruction(
        &mut self,
        _: BusAccess,
        _: u16,
        _: CpuClocks,
    ) -> InstructionReport<(), ()> {
        panic!("unexpected instruction write")
    }
    fn detect_instruction_address_error(
        &mut self,
        _: BusAccess,
        direction: Direction,
        start: CpuClocks,
    ) -> InstructionReport<(), ()> {
        assert_eq!(direction, Direction::Read);
        self.advance(start);
        InstructionReport {
            elapsed: 7,
            outcome: if self.fault.is_some() {
                InstructionOutcome::TimingUnavailable
            } else {
                InstructionOutcome::AtGroup0Entry
            },
        }
    }
}

fn cpu(opcode: u16, second: u16) -> PrefetchCpu {
    let mut registers = Registers::new();
    registers.pc = 0x2000;
    registers.ssp = 0x5000;
    registers.usp = 0x6000;
    registers.set_status_register(0x201f);
    PrefetchCpu::from_prefetched(registers, [opcode, second], CpuClocks(17)).unwrap()
}

pub(super) fn instruction_steps(memory: &mut MemoryBus) {
    for address in [4, 8, 12] {
        memory.word(address, 0);
        memory.word(address + 2, 0x1000);
    }
    memory.word(0, 0);
    memory.word(2, 0x5000);
    memory.word(0x1000, 0x4e71);
    memory.word(0x1002, 0x7201);
    memory.word(0x2004, 0x4e71);
    memory.word(0x2006, 0x7201);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut checked = 0;
    for _ in 0..100 {
        for (opcode, second, nominal) in [
            (0x4e71, 0x7201, 7),
            (0x70ff, 0x7201, 7),
            (0x6002, 0, 16),
            (0x6000, 2, 16),
        ] {
            let mut cpu = cpu(opcode, second);
            let mut bus = Device {
                memory,
                clock: 17,
                reads: 0,
                fault: None,
            };
            assert!(matches!(black_box(cpu.step_physical_instruction(&mut bus)),
                Ok(PhysicalInstructionStep::Instruction(step)) if step.clocks == nominal));
            assert_eq!(cpu.clock().0, bus.clock);
            checked += 1;
        }
        for (opcode, second, failed) in [
            (0x4e71, 0x7201, 0),
            (0x70ff, 0x7201, 0),
            (0x6002, 0, 0),
            (0x6000, 2, 1),
        ] {
            for outcome in 0..3 {
                let mut cpu = cpu(opcode, second);
                let mut bus = Device {
                    memory,
                    clock: 17,
                    reads: 0,
                    fault: Some((failed, outcome)),
                };
                let result = black_box(cpu.step_physical_instruction(&mut bus));
                assert_eq!(result.is_ok(), outcome == 1);
                assert_eq!(cpu.clock().0, bus.clock);
                assert_eq!(bus.reads, failed + 1);
                checked += 1;
            }
        }
        for branch in [false, true] {
            memory.word(0x2004, 0xa000);
            let mut cpu = if branch {
                cpu(0x6002, 0)
            } else {
                cpu(0x4e71, 0xa000)
            };
            let mut bus = Device {
                memory,
                clock: 17,
                reads: 0,
                fault: Some((usize::from(branch), 1)),
            };
            assert!(matches!(
                black_box(cpu.step_physical_instruction(&mut bus)),
                Err(InstructionFault::CaptureUnavailable { .. })
            ));
            assert_eq!(cpu.clock().0, bus.clock);
            checked += 1;
            bus.fault = None;
            let release = cpu.clock();
            assert!(
                black_box(cpu.external_warm_reset(
                    &mut bus,
                    ExternalResetBoundary {
                        sampled_release: release,
                        qualification: ResetQualification::QualifiedSafeAssertionAndRelease,
                    }
                ))
                .is_ok()
            );
            assert_eq!(cpu.clock().0, bus.clock);
            checked += 1;
        }
        memory.word(0x2004, 0x4e71);
        for unavailable in [false, true] {
            let mut cpu = cpu(0x6001, 0);
            let mut bus = Device {
                memory,
                clock: 17,
                reads: 0,
                fault: unavailable.then_some((0, 2)),
            };
            assert_eq!(
                black_box(cpu.step_physical_instruction(&mut bus)).is_ok(),
                !unavailable
            );
            assert_eq!(cpu.clock().0, bus.clock);
            assert_eq!(bus.reads, 0);
            checked += 1;
        }
    }
    assert_eq!(checked, 2_200);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
