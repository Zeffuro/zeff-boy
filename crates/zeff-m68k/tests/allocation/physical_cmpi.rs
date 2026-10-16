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

const FIELDS: [u16; 7] = [0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x39];

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
        let kind = self.failure.filter(|(at, _)| *at == self.transfers);
        let elapsed = if kind.is_some_and(|(_, kind)| kind == 3) {
            3
        } else {
            7
        };
        self.advance(start, elapsed);
        self.transfers += 1;
        InstructionReport {
            elapsed,
            outcome: match kind {
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
        panic!("comparison write")
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
    let (long, source, register) = form;
    let immediate_reads = if long { 2 } else { 1 };
    let operand_at = immediate_reads + extensions(source);
    let count = operand_at + 2 + usize::from(long);
    let stride = if long { 4 } else { 2 };
    let operand = 0x3000 + u32::from(odd.is_some());
    let opcode = 0x0c00
        | if long { 0x80 } else { 0x40 }
        | FIELDS[source]
        | if source < 5 { register as u16 } else { 0 };
    let anchor = 0x2002 + stride + extensions(source) as u32 * 2;
    let mut r = Registers::new();
    r.pc = 0x2000;
    r.usp = 0x6000;
    r.ssp = 0x5000;
    r.d.fill(0x1234_5678);
    r.d[7] = 0x1000;
    r.set_status_register(0x001f | if supervisor { 0x2000 } else { 0 });
    if source < 5 {
        r.set_address_register(
            register,
            if source == 4 {
                0x2000
            } else {
                operand + if source == 2 { stride } else { 0 }
            },
        );
    }
    let extension = match source {
        0..=2 => 0x4e71,
        3 | 6 => 0,
        4 => 0x7800 | u16::from(odd.is_some()),
        5 => operand as u16,
        _ => unreachable!(),
    };
    let base = r.pc + stride;
    if long {
        memory.word(0x2004, 1);
    }
    memory.word((base + 2) as usize, extension);
    if source == 6 {
        memory.word((base + 4) as usize, operand as u16);
    }
    memory.word(anchor as usize, 0x4e71);
    memory.word((anchor + 2) as usize, 0x7201);
    memory.word((operand & !1) as usize, if long { 0 } else { 2 });
    memory.word(((operand + 2) & !1) as usize, 2);
    let queue = [opcode, if long { 0 } else { 1 }];
    let mut cpu = PrefetchCpu::from_prefetched(r, queue, CpuClocks(17)).unwrap();
    let mut bus = Device {
        memory,
        clock: 17,
        transfers: 0,
        failure,
        detector: odd.unwrap_or(0),
    };
    let result = black_box(cpu.step_physical_instruction(&mut bus));
    let mut phase = r;
    let at = failure.map_or(count - 1, |(at, _)| at);
    if source == 2 && at >= operand_at || source == 1 && at >= operand_at + usize::from(long) {
        phase.set_address_register(
            register,
            if source == 2 {
                operand
            } else {
                operand + stride
            },
        );
    }
    if at == count - 1 {
        phase.set_status_register((r.status_register() & !15) | 16);
    }
    let source_idle = u64::from(matches!(source, 2 | 4)) * 2;
    if let Some(kind) = odd {
        assert_eq!(bus.transfers, operand_at);
        if kind == 4 {
            assert!(
                matches!(result, Ok(PhysicalInstructionStep::Group0 { clocks, .. })
                if clocks == operand_at as u64 * 7 + source_idle + 17 + 50)
            );
        } else {
            assert!(result.is_err());
        }
    } else if let Some((at, kind)) = failure {
        assert_eq!(bus.transfers, at + 1);
        if kind == 1 {
            let idle = if at >= immediate_reads {
                source_idle
            } else {
                0
            };
            assert!(
                matches!(result, Ok(PhysicalInstructionStep::Group0 { capture, clocks, .. })
                if clocks == (at as u64 + 1) * 7 + idle + 50
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
            if step.clocks == count as u64 * 7 + source_idle)
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

pub(super) fn compare_steps(memory: &mut MemoryBus) {
    for vector in [2, 3] {
        memory.word(vector * 4, 0);
        memory.word(vector * 4 + 2, 0x1000);
    }
    memory.word(0x1000, 0x4e71);
    memory.word(0x1002, 0x7201);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut checked = 0;
    for _ in 0..100 {
        for supervisor in [false, true] {
            for long in [false, true] {
                for source in 0..7 {
                    let count = if long { 5 } else { 3 } + extensions(source);
                    for register in 0..if source < 5 { 8 } else { 1 } {
                        let form = (long, source, register);
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
                        run(memory, (long, source, 0), supervisor, None, Some(detector));
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 330_000);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
