use super::{ALLOCATIONS, MemoryBus, black_box};
use std::sync::atomic::Ordering;
use zeff_m68k::Registers;
use zeff_m68k::bus::{BusAccess, Direction, FunctionCode, M68kBus};
use zeff_m68k::timing::physical::{
    EntryBus, EntryOutcome, EntryReport, ExternalResetBoundary, Group0Capture,
    ImportedGroup0Boundary, ProcessingClass, ResetQualification, SswProfile,
};
use zeff_m68k::timing::{CpuClocks, PrefetchCpu};

struct Device<'a> {
    memory: &'a mut MemoryBus,
    clock: u64,
    transfers: usize,
    failure: Option<(usize, u8)>,
}

impl Device<'_> {
    fn finish<T>(&mut self, start: CpuClocks, value: T) -> EntryReport<T, ()> {
        assert_eq!(start.0, self.clock);
        self.clock += 7;
        let index = self.transfers;
        self.transfers += 1;
        let outcome = match self.failure.filter(|(at, _)| *at == index) {
            Some((_, 0)) => EntryOutcome::HostFailure(()),
            Some((_, 1)) => EntryOutcome::CriticalAbortReleased,
            Some(_) => EntryOutcome::TimingUnavailable,
            None => EntryOutcome::Complete(value),
        };
        EntryReport {
            elapsed: 7,
            outcome,
        }
    }
}

impl EntryBus for Device<'_> {
    type Error = ();

    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, ()> {
        let value = self.memory.read(access).unwrap();
        self.finish(start, value)
    }

    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), ()> {
        self.memory.write(access, value).unwrap();
        self.finish(start, ())
    }

    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(start.0, self.clock);
        self.clock += u64::from(clocks);
    }
}

fn boundary(vector: u8, sr: u16) -> ImportedGroup0Boundary {
    let mut registers = Registers::new();
    registers.ssp = 0x5000;
    registers.usp = 0x6000;
    registers.set_status_register(sr);
    ImportedGroup0Boundary {
        registers,
        clock: CpuClocks(17),
        capture: Group0Capture {
            vector,
            internal_pc: 0x12345678,
            phase_sr: sr,
            ird: 0x4e71,
            logical_address: 0x87654321,
            function_code: FunctionCode::UserData,
            direction: Direction::Read,
            processing_class: ProcessingClass::InstructionOrGroup2,
            ssw_profile: SswProfile::SelectedIrdUpperBits,
        },
    }
}

pub(super) fn physical_steps(memory: &mut MemoryBus) {
    for address in [8, 12, 4] {
        memory.word(address, 0);
        memory.word(address + 2, 0x1000);
    }
    memory.word(0, 0);
    memory.word(2, 0x5000);
    memory.word(0x1000, 0x4e71);
    memory.word(0x1002, 0x7201);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut checked = 0;
    for _ in 0..100 {
        for vector in [2, 3] {
            for sr in [0x001f, 0xa715] {
                let mut cpu = PrefetchCpu::from_group0_boundary(boundary(vector, sr)).unwrap();
                let mut bus = Device {
                    memory,
                    clock: 17,
                    transfers: 0,
                    failure: None,
                };
                let step = black_box(cpu.enter_imported_group0(&mut bus)).unwrap();
                assert_eq!(step.clocks, 83);
                assert_eq!(cpu.clock().0, bus.clock);
                assert_eq!(cpu.registers.ssp, 0x4ff2);
                assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
                checked += 1;
            }
        }
        for (reset, transfers) in [(false, 11), (true, 6)] {
            for failed in 0..transfers {
                for outcome in 0..3 {
                    let mut cpu = PrefetchCpu::from_group0_boundary(boundary(2, 0x201f)).unwrap();
                    let mut bus = Device {
                        memory,
                        clock: 17,
                        transfers: 0,
                        failure: Some((failed, outcome)),
                    };
                    let result = if reset {
                        cpu.external_warm_reset(
                            &mut bus,
                            ExternalResetBoundary {
                                sampled_release: CpuClocks(17),
                                qualification: ResetQualification::QualifiedSafeAssertionAndRelease,
                            },
                        )
                    } else {
                        cpu.enter_imported_group0(&mut bus)
                    };
                    assert!(black_box(result).is_err());
                    assert_eq!(cpu.clock().0, bus.clock);
                    assert_eq!(bus.transfers, failed + 1);
                    assert_eq!(cpu.queued_words(), None);
                    checked += 1;
                }
            }
        }
        let mut cpu = PrefetchCpu::from_group0_boundary(boundary(2, 0xa715)).unwrap();
        let mut bus = Device {
            memory,
            clock: 17,
            transfers: 0,
            failure: Some((0, 1)),
        };
        assert!(cpu.enter_imported_group0(&mut bus).is_err());
        checked += 1;
        bus.failure = None;
        bus.transfers = 0;
        let release = cpu.clock();
        let step = black_box(cpu.external_warm_reset(
            &mut bus,
            ExternalResetBoundary {
                sampled_release: release,
                qualification: ResetQualification::QualifiedSafeAssertionAndRelease,
            },
        ))
        .unwrap();
        assert_eq!(step.clocks, 58);
        assert_eq!(cpu.clock().0, bus.clock);
        assert_eq!(cpu.registers.ssp, 0x5000);
        assert_eq!(cpu.registers.status_register(), 0x2715);
        checked += 1;
    }
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(checked, 5_700);
    assert_eq!(after - before, 0);
}
