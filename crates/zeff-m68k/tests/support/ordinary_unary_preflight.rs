use super::*;
use bulk_device::{Device, Event};
use bulk_model::Spec;
use zeff_m68k::timing::physical::{
    EntryBus, EntryOutcome, EntryReport, Group0Capture, ImportedGroup0Boundary, ProcessingClass,
    SswProfile,
};
use zeff_m68k::timing::physical_instruction::{
    InstructionBus, InstructionOutcome, InstructionReport,
};

struct Gate {
    bus: Device,
    double: bool,
}

impl Gate {
    fn read(&mut self, access: BusAccess, start: CpuClocks) -> u16 {
        assert_eq!(self.bus.clock, start.0);
        let value = bulk_model::sample(&self.bus.words, access.logical_address);
        self.bus.events.push(Event::Read(access, start.0, value, 4));
        self.bus.count += 1;
        self.bus.clock += 8;
        value
    }
    fn write(&mut self, access: BusAccess, value: u16, start: CpuClocks) {
        assert_eq!(self.bus.clock, start.0);
        self.bus
            .events
            .push(Event::Write(access, start.0, value, 4));
        self.bus.count += 1;
        self.bus.clock += 8;
    }
}

impl EntryBus for Gate {
    type Error = usize;
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, usize> {
        self.read(access, start);
        EntryReport {
            elapsed: 8,
            outcome: EntryOutcome::CriticalAbortReleased,
        }
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), usize> {
        self.write(access, value, start);
        EntryReport {
            elapsed: 8,
            outcome: EntryOutcome::CriticalAbortReleased,
        }
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.bus.idle(start, clocks);
    }
}

impl InstructionBus for Gate {
    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, usize> {
        self.read(access, start);
        InstructionReport {
            elapsed: 8,
            outcome: if self.double {
                InstructionOutcome::AtGroup0Entry
            } else {
                InstructionOutcome::TimingUnavailable
            },
        }
    }
    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> InstructionReport<(), usize> {
        self.write(access, value, start);
        InstructionReport {
            elapsed: 8,
            outcome: if self.double {
                InstructionOutcome::AtGroup0Entry
            } else {
                InstructionOutcome::TimingUnavailable
            },
        }
    }
}

fn prepared(s: Spec, guard: u8) -> (PrefetchCpu, Device, TimingFault<usize>) {
    let (_, mut bus) = s.seed();
    let mut cpu = PrefetchCpu::from_prefetched(s.r, s.queue(), CpuClocks(0)).unwrap();
    let error = match guard {
        0 => {
            cpu = PrefetchCpu::default();
            cpu.registers = s.r;
            TimingFault::Unprimed
        }
        1 => {
            cpu.registers.pc += 2;
            TimingFault::Unprimed
        }
        2 | 3 => {
            let mut r = s.r;
            r.set_status_register(if guard == 2 {
                r.status_register() | 0x8000
            } else {
                0x271f
            });
            cpu = PrefetchCpu::from_prefetched(
                r,
                [
                    if guard == 2 { 0x4e71 } else { 0x4e72 },
                    if guard == 2 {
                        s.opcode()
                    } else {
                        s.r.status_register()
                    },
                ],
                CpuClocks(0),
            )
            .unwrap();
            bus.word(r.pc + 4, s.queue()[1]);
            cpu.step_instruction(&mut bus).unwrap();
            cpu.registers = s.r;
            if guard == 2 {
                TimingFault::PendingTrace
            } else {
                TimingFault::Stopped
            }
        }
        4 => {
            bus.fail = Some(0);
            assert!(cpu.step_instruction(&mut bus).is_err());
            TimingFault::Frozen
        }
        5 => {
            cpu = PrefetchCpu::from_group0_boundary(ImportedGroup0Boundary {
                registers: s.r,
                clock: CpuClocks(0),
                capture: Group0Capture {
                    vector: 2,
                    internal_pc: s.r.pc + 2,
                    phase_sr: s.r.status_register(),
                    ird: s.opcode(),
                    logical_address: bulk_model::address(s, s.extension),
                    function_code: if s.r.status_register() & 0x2000 != 0 {
                        FunctionCode::SupervisorData
                    } else {
                        FunctionCode::UserData
                    },
                    direction: Direction::Read,
                    processing_class: ProcessingClass::InstructionOrGroup2,
                    ssw_profile: SswProfile::SelectedIrdUpperBits,
                },
            })
            .unwrap();
            TimingFault::ImportedBoundaryPending
        }
        6 | 7 => {
            let mut gate = Gate {
                bus,
                double: guard == 7,
            };
            assert!(cpu.step_physical_instruction(&mut gate).is_err());
            bus = gate.bus;
            if guard == 6 {
                TimingFault::PhysicalFaultUnqualified
            } else {
                TimingFault::DoubleFaultHalted
            }
        }
        _ => unreachable!(),
    };
    (cpu, bus, error)
}

pub(super) fn preflight() -> (u32, u32) {
    let mut count = bulk_check::Count::default();
    for base in bulk_model::encoded() {
        for bank in [false, true] {
            let s = base.bank(bank, 0x1f);
            for guard in 0..8 {
                let (mut cpu, mut bus, expected) = prepared(s, guard);
                let saved = cpu.clone();
                let old = bus.clone();
                let good =
                    cpu.step_instruction(&mut bus) == Err(expected) && cpu == saved && bus == old;
                count.count += 1;
                count.bad += u32::from(!good);
            }
        }
    }
    count.result()
}
