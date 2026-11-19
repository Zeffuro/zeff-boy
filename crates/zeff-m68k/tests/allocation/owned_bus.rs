use super::*;
use zeff_m68k::{Registers, SUPERVISOR};

struct OwnedBus<'a> {
    memory: &'a mut MemoryBus,
    clock: u64,
    pulse: bool,
    rmw: Option<BusAccess>,
    pulses: usize,
    locks: usize,
}

impl ClockedBus for OwnedBus<'_> {
    type Error = ();

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, ()> {
        assert_eq!(start.0, self.clock);
        assert!(!self.pulse);
        if let Some(held) = self.rmw {
            assert_eq!(access, held);
        }
        self.clock += 4;
        CycleResult::ready(self.memory.read(access).unwrap())
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), ()> {
        assert_eq!(start.0, self.clock);
        assert_eq!(Some(access), self.rmw);
        assert!(!self.pulse);
        self.clock += 4;
        self.memory.write(access, value).unwrap();
        CycleResult::ready(())
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        assert_eq!(start.0, self.clock);
        if self.pulse {
            assert_eq!(clocks, 124);
        }
        if self.rmw.is_some() {
            assert_eq!(clocks, 2);
        }
        self.clock += u64::from(clocks);
    }

    fn begin_reset_output(&mut self, start: CpuClocks) -> Result<BusResponse<()>, ()> {
        assert_eq!(start.0, 4);
        assert!(!self.pulse && self.rmw.is_none());
        self.pulse = true;
        Ok(BusResponse::Complete(()))
    }

    fn end_reset_output(&mut self, end: CpuClocks) {
        assert!(self.pulse);
        assert_eq!(end.0, 128);
        self.pulse = false;
        self.pulses += 1;
    }

    fn begin_rmw(&mut self, access: BusAccess, start: CpuClocks) -> Result<BusResponse<()>, ()> {
        assert_eq!(start.0, self.clock);
        assert!(!self.pulse && self.rmw.is_none());
        self.rmw = Some(access);
        Ok(BusResponse::Complete(()))
    }

    fn end_rmw(&mut self, end: CpuClocks) {
        assert_eq!(end.0, self.clock);
        assert!(self.rmw.take().is_some());
        self.locks += 1;
    }
}

pub(super) fn owned_steps(memory: &mut MemoryBus) {
    let mut registers = Registers::new();
    registers.pc = 0x2000;
    registers.ssp = 0x5000;
    registers.usp = 0x6000;
    registers.a.fill(0x3008);
    registers.d.fill(0x1234_5678);
    registers.d[7] = 4;
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut executed = 0;
    for _ in 0..100 {
        for bank in [0, SUPERVISOR] {
            registers.set_status_register(bank | 31);
            for mode in [0, 2, 3, 4, 5, 6, 7] {
                for reg in 0..if mode == 7 { 2 } else { 8 } {
                    for odd in [0, 1] {
                        let mut case = registers;
                        if mode != 0 && mode != 7 {
                            case.set_address_register(usize::from(reg), 0x3008 + odd);
                        }
                        let extensions: &[u16] = match (mode, reg) {
                            (5, _) => &[16],
                            (6, _) => &[0x7000],
                            (7, 0) => &[0x3010],
                            (7, 1) => &[0, 0x3010],
                            _ => &[],
                        };
                        let opcode = 0x4ac0 | mode << 3 | reg;
                        memory.word(0x2000, opcode);
                        for (index, &value) in extensions.iter().enumerate() {
                            memory.word(0x2002 + index * 2, value);
                        }
                        memory.word(0x2002 + extensions.len() * 2, 0x7201);
                        memory.word(0x2004 + extensions.len() * 2, 0x7402);
                        let mut bus = OwnedBus {
                            memory,
                            clock: 0,
                            pulse: false,
                            rmw: None,
                            pulses: 0,
                            locks: 0,
                        };
                        let mut cpu = PrefetchCpu::from_prefetched(
                            case,
                            [opcode, extensions.first().copied().unwrap_or(0x7201)],
                            CpuClocks(0),
                        )
                        .unwrap();
                        assert!(black_box(cpu.step_instruction(&mut bus)).is_ok());
                        assert_eq!(bus.locks, usize::from(mode != 0));
                        assert!(!bus.pulse && bus.rmw.is_none());
                        assert_eq!(cpu.clock().0, bus.clock);
                        executed += 1;
                    }
                }
            }
        }
        registers.set_status_register(SUPERVISOR | 31);
        let mut bus = OwnedBus {
            memory,
            clock: 0,
            pulse: false,
            rmw: None,
            pulses: 0,
            locks: 0,
        };
        let mut cpu =
            PrefetchCpu::from_prefetched(registers, [0x4e70, 0x7201], CpuClocks(0)).unwrap();
        assert!(black_box(cpu.step_instruction(&mut bus)).is_ok());
        assert_eq!(bus.pulses, 1);
        assert!(!bus.pulse && bus.rmw.is_none());
        assert_eq!(cpu.clock().0, bus.clock);
        executed += 1;
    }
    assert_eq!(executed, 20_100);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed) - before, 0);
}
