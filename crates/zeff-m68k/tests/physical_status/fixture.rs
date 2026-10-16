use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct Transfer {
    pub at: u64,
    pub access: BusAccess,
    pub pc: u32,
    pub ird: u16,
    pub phase: InstructionPhase,
    pub r: Registers,
    pub value: u16,
}
pub(super) struct Case {
    pub opcode: u16,
    pub queued: u16,
    pub r: Registers,
    pub bus: Device,
    pub transfers: Vec<Transfer>,
    pub idles: Vec<(u64, u32)>,
    pub final_r: Registers,
    pub final_queue: [u16; 2],
    pub duration: u64,
}
pub(super) fn access(address: u32, fc: u8, kind: AccessKind) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0xfffffe,
        function_code: match fc {
            1 => FunctionCode::UserData,
            2 => FunctionCode::UserProgram,
            5 => FunctionCode::SupervisorData,
            6 => FunctionCode::SupervisorProgram,
            _ => panic!("literal FC"),
        },
        kind,
        lanes: ByteLanes::Both,
    }
}
pub(super) fn registers(sr: u16) -> Registers {
    let mut r = Registers::new();
    r.d = std::array::from_fn(|i| 0xa100_0100 + i as u32 * 0x20);
    r.a = std::array::from_fn(|i| 0xb200_0200 + i as u32 * 0x40);
    r.usp = 0x5000;
    r.ssp = 0x3000;
    r.pc = 0x1000;
    r.set_status_register(sr);
    r
}
impl Case {
    pub fn new(opcode: u16, queued: u16, r: Registers) -> Self {
        let mut bus = Device::new(0);
        bus.word(0x1000, opcode);
        bus.word(0x1002, queued);
        Self {
            opcode,
            queued,
            r,
            bus,
            transfers: Vec::new(),
            idles: Vec::new(),
            final_r: r,
            final_queue: [0x4e71, 0x7000],
            duration: 0,
        }
    }
    pub fn read(
        &mut self,
        at: u64,
        address: u32,
        fc: u8,
        capture: (AccessKind, u32, u16, InstructionPhase),
        sample: (Registers, u16),
    ) {
        let (kind, pc, ird, phase) = capture;
        let (r, value) = sample;
        self.transfers.push(Transfer {
            at,
            access: access(address, fc, kind),
            pc,
            ird,
            phase,
            r,
            value,
        });
        self.bus.word(address, value);
    }
    pub fn seed(&self) -> PrefetchCpu {
        PrefetchCpu::from_prefetched(
            self.r,
            [self.opcode, self.queued],
            CpuClocks(self.bus.clock),
        )
        .unwrap()
    }
    pub fn events(&self, reads: usize, until: u64) -> Vec<Event> {
        let mut result: Vec<_> = self.transfers[..reads]
            .iter()
            .map(|t| Event::Read(t.access, t.at, t.value))
            .collect();
        result.extend(
            self.idles
                .iter()
                .filter(|(at, _)| *at < until)
                .map(|&(at, n)| Event::Idle(at, n)),
        );
        result.sort_by_key(|e| match *e {
            Event::Read(_, at, _) | Event::Idle(at, _) => at,
            _ => unreachable!(),
        });
        result
    }
}
pub(super) fn status(mut r: Registers, sr: u16) -> Registers {
    r.set_status_register(sr);
    r
}
