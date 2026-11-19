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

const FIELDS: [u16; 9] = [0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x39, 0x3a, 0x3b];

struct Device<'a> {
    memory: &'a mut MemoryBus,
    clock: u64,
    transfers: usize,
    failure: Option<(usize, u8)>,
    detector: u8,
}

impl Device<'_> {
    fn advance(&mut self, start: CpuClocks, elapsed: u32) {
        assert_eq!(self.clock, start.0);
        self.clock += u64::from(elapsed);
    }
}

impl EntryBus for Device<'_> {
    type Error = ();
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, ()> {
        self.advance(start, 4);
        EntryReport {
            elapsed: 4,
            outcome: EntryOutcome::Complete(self.memory.read(access).unwrap()),
        }
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), ()> {
        self.advance(start, 4);
        self.memory.write(access, value).unwrap();
        EntryReport {
            elapsed: 4,
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
        assert_eq!(access.logical_address & 1, 0);
        let value = self.memory.read(access).unwrap();
        let failed = self.failure.filter(|(at, _)| *at == self.transfers);
        let elapsed = if failed.is_some_and(|(_, kind)| kind == 3) {
            3
        } else {
            7
        };
        self.advance(start, elapsed);
        self.transfers += 1;
        InstructionReport {
            elapsed,
            outcome: match failed {
                Some((_, 0)) => InstructionOutcome::HostFailure(()),
                Some((_, 1)) => InstructionOutcome::AtGroup0Entry,
                Some((_, 2)) => InstructionOutcome::TimingUnavailable,
                _ => InstructionOutcome::Complete(value),
            },
        }
    }
    fn write_instruction(
        &mut self,
        _: BusAccess,
        _: u16,
        _: CpuClocks,
    ) -> InstructionReport<(), ()> {
        panic!("status write")
    }
    fn detect_instruction_address_error(
        &mut self,
        access: BusAccess,
        _: Direction,
        start: CpuClocks,
    ) -> InstructionReport<(), ()> {
        assert_eq!(access.logical_address & 1, 1);
        let elapsed = if self.detector == 0 { 0 } else { 17 };
        self.advance(start, elapsed);
        InstructionReport {
            elapsed,
            outcome: match self.detector {
                1 => InstructionOutcome::Complete(()),
                2 => InstructionOutcome::HostFailure(()),
                4 => InstructionOutcome::AtGroup0Entry,
                _ => InstructionOutcome::TimingUnavailable,
            },
        }
    }
}

fn extensions(source: usize) -> usize {
    if source == 6 {
        2
    } else {
        usize::from(source >= 3)
    }
}

fn run(
    memory: &mut MemoryBus,
    form: (bool, usize, usize),
    supervisor: bool,
    failure: Option<(usize, u8)>,
    odd: Option<u8>,
) {
    let (to_sr, source, register) = form;
    let operand_at = extensions(source);
    let count = operand_at + 3;
    let operand = 0x3000 + u32::from(odd.is_some());
    let opcode = if to_sr { 0x46c0 } else { 0x44c0 }
        | FIELDS[source]
        | if source < 5 { register as u16 } else { 0 };
    let anchor = 0x2002 + operand_at as u32 * 2;
    let mut r = Registers::new();
    r.pc = 0x2000;
    r.usp = 0x6000;
    r.ssp = 0x5000;
    r.d.fill(0x1234_5678);
    r.d[7] = if source == 8 { 0x0ffe } else { 0x1000 };
    r.set_status_register(0x001f | if supervisor { 0x2000 } else { 0 });
    if source < 5 {
        r.set_address_register(
            register,
            if source == 4 {
                0x2000
            } else {
                operand + u32::from(source == 2) * 2
            },
        );
    }
    let extension = match source {
        0..=2 => 0x4afc,
        3 | 6 => 0,
        4 | 8 => 0x7800 | u16::from(odd.is_some()),
        5 => operand as u16,
        7 => operand.wrapping_sub(0x2002) as u16,
        _ => unreachable!(),
    };
    if source == 6 {
        memory.word(0x2004, operand as u16);
    }
    memory.word(anchor as usize, 0x4e71);
    memory.word((anchor + 2) as usize, 0x7201);
    memory.word((operand & !1) as usize, 0x5810);
    let queue = [opcode, extension];
    let mut cpu = PrefetchCpu::from_prefetched(r, queue, CpuClocks(17)).unwrap();
    let mut bus = Device {
        memory,
        clock: 17,
        transfers: 0,
        failure,
        detector: odd.unwrap_or(0),
    };
    let result = black_box(cpu.step_physical_instruction(&mut bus));
    let at = failure.map_or(count - 1, |(at, _)| at);
    let mut phase = r;
    if matches!(source, 1 | 2) && at >= operand_at {
        phase.set_address_register(register, if source == 2 { operand } else { operand + 2 });
    }
    if at > operand_at && odd.is_none() {
        phase.set_status_register(if to_sr {
            0x0010
        } else {
            (r.status_register() & !31) | 16
        });
    }
    let source_idle = u64::from(matches!(source, 2 | 4 | 8)) * 2;
    let publication_idle = if at > operand_at { 4 } else { 0 };
    if let Some(kind) = odd {
        assert_eq!(bus.transfers, operand_at);
        if kind == 4 {
            assert!(
                matches!(result, Ok(PhysicalInstructionStep::Group0 { clocks, .. })
                if clocks == operand_at as u64 * 7 + source_idle + 17 + 50)
            );
        } else {
            assert!(result.is_err());
            assert_eq!(cpu.registers, phase);
        }
    } else if let Some((at, kind)) = failure {
        assert_eq!(bus.transfers, at + 1);
        if kind == 1 {
            assert!(
                matches!(result, Ok(PhysicalInstructionStep::Group0 { capture, clocks, .. })
                if clocks == (at as u64 + 1) * 7 + source_idle + publication_idle + 50
                    && capture.phase_sr == phase.status_register()
                    && capture.ird == if at == count - 1 { 0x4e71 } else { opcode })
            );
        } else {
            assert!(result.is_err());
            assert_eq!(cpu.registers, phase);
            assert_eq!(
                cpu.queued_words(),
                if kind == 3 { Some(queue) } else { None }
            );
        }
    } else {
        phase.pc = anchor;
        assert_eq!(cpu.registers, phase);
        assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
        assert_eq!(bus.transfers, count);
        assert!(
            matches!(result, Ok(PhysicalInstructionStep::Instruction(step))
            if step.clocks == count as u64 * 7 + source_idle + 4)
        );
    }
    if failure.is_some_and(|(_, kind)| kind != 1) || odd.is_some_and(|kind| kind != 4) {
        let before = cpu.clone();
        let consumed = bus.transfers;
        let clock = bus.clock;
        assert!(cpu.step_physical_instruction(&mut bus).is_err());
        assert_eq!(cpu, before);
        assert_eq!(bus.transfers, consumed);
        assert_eq!(bus.clock, clock);
    }
    assert_eq!(cpu.clock().0, bus.clock);
    assert!(!matches!(
        result,
        Err(InstructionFault::CaptureUnavailable { .. })
    ));
}

pub(super) fn status_steps(memory: &mut MemoryBus) {
    for vector in [2, 3] {
        memory.word(vector * 4, 0);
        memory.word(vector * 4 + 2, 0x1000);
    }
    memory.word(0x1000, 0x4e71);
    memory.word(0x1002, 0x7201);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut checked = 0;
    for _ in 0..100 {
        for (to_sr, supervisor) in [(false, false), (false, true), (true, true)] {
            for source in 0..9 {
                let count = extensions(source) + 3;
                for register in 0..if source < 5 { 8 } else { 1 } {
                    let form = (to_sr, source, register);
                    run(memory, form, supervisor, None, None);
                    checked += 1;
                    for at in 0..count {
                        for kind in 0..4 {
                            run(memory, form, supervisor, Some((at, kind)), None);
                            checked += 1;
                        }
                    }
                }
                for detector in 0..5 {
                    run(memory, (to_sr, source, 0), supervisor, None, Some(detector));
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 210_300);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
