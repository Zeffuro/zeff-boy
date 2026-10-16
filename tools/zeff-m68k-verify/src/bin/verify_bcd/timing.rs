use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, PrefetchCpu};
use zeff_m68k::{CARRY, EXTEND, Registers, ZERO};

struct Bus {
    words: [u16; 2],
    clock: u64,
    transfers: usize,
    valid: bool,
    nbcd: bool,
}

impl Bus {
    fn observe(&mut self, access: BusAccess, start: CpuClocks, write: bool) {
        let index = self.transfers;
        let (address, kind, lanes, fc) = if index == if self.nbcd { 1 } else { 2 } {
            (
                0x2004,
                AccessKind::Prefetch,
                ByteLanes::Both,
                FunctionCode::SupervisorProgram,
            )
        } else {
            let address = if index == 0 || self.nbcd {
                0x3000
            } else {
                0x3010
            };
            (
                address,
                AccessKind::Data,
                ByteLanes::Upper,
                FunctionCode::SupervisorData,
            )
        };
        self.valid &= start.0 == self.clock
            && access.logical_address == address
            && access.bus_address == address
            && access.kind == kind
            && access.lanes == lanes
            && access.function_code == fc
            && write == (index == if self.nbcd { 2 } else { 3 });
        self.clock += 4;
        self.transfers += 1;
    }
}

impl ClockedBus for Bus {
    type Error = ();
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, ()> {
        self.observe(access, start, false);
        let result = match access.bus_address {
            0x3000 => Ok(self.words[0]),
            0x3010 => Ok(self.words[1]),
            0x2004 => Ok(0x7402),
            _ => Err(()),
        };
        CycleResult {
            result,
            wait_clocks: 0,
        }
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), ()> {
        self.observe(access, start, true);
        let index = usize::from(access.bus_address == 0x3010);
        let result = if matches!(access.bus_address, 0x3000 | 0x3010) {
            self.words[index] = (self.words[index] & 0xff) | (value & 0xff00);
            Ok(())
        } else {
            Err(())
        };
        CycleResult {
            result,
            wait_clocks: 0,
        }
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.valid &= !self.nbcd && self.transfers == 0 && start.0 == 0 && clocks == 2;
        self.clock += u64::from(clocks);
    }
}

pub(super) fn witness(
    opcode: u16,
    source: u8,
    destination: u8,
    x: bool,
    z: bool,
    expected: &[u8],
) -> bool {
    let nbcd = opcode == 0x4800;
    let opcode = if nbcd { 0x4810 } else { opcode | 8 };
    let mut registers = Registers::new();
    for i in 0..8 {
        registers.d[i] = 0xabcd_1200 + i as u32 * 0x100;
    }
    registers.pc = 0x2000;
    registers.a[0] = if nbcd { 0x3000 } else { 0x3001 };
    registers.a[1] = 0x3011;
    registers.ssp = 0x4000;
    registers.usp = 0x5000;
    registers.set_status_register(
        0x2500 | if x { CARRY | EXTEND } else { 0 } | if z { ZERO } else { 0 },
    );
    let mut cpu = PrefetchCpu::from_prefetched(registers, [opcode, 0x7201], CpuClocks(0)).unwrap();
    let mut bus = Bus {
        words: [
            u16::from(source) << 8 | 0x5a,
            u16::from(destination) << 8 | 0xa5,
        ],
        clock: 0,
        transfers: 0,
        valid: true,
        nbcd,
    };
    let mut memory = bus.words;
    let index = usize::from(!nbcd);
    memory[index] = u16::from(expected[1]) << 8 | (memory[index] & 0xff);
    registers.a[0] = 0x3000;
    if !nbcd {
        registers.a[1] = 0x3010;
    }
    registers.pc = 0x2002;
    registers.set_status_register(0x2500 | u16::from(expected[0]));
    matches!(cpu.step_instruction(&mut bus), Ok(s) if s.clocks==if nbcd {12} else {18} && s.exception.is_none())
        && cpu.registers == registers
        && cpu.queued_words() == Some([0x7201, 0x7402])
        && bus.words == memory
        && bus.valid
        && bus.clock == cpu.clock().0
        && bus.transfers == if nbcd { 3 } else { 4 }
        && !cpu.trace_pending()
}
