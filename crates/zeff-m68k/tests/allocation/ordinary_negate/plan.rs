use super::*;

#[derive(Clone, Copy)]
pub(super) struct Spec {
    pub family: u16,
    pub long: bool,
    pub mode: u16,
    pub reg: usize,
    pub initial: Registers,
    pub source: u32,
}

impl Spec {
    pub fn new(family: u16, long: bool, mode: u16, reg: usize, sr: u16, source: u32) -> Self {
        let mut initial = Registers::new();
        initial.pc = 0x2000;
        initial.d = [0x1234_5678; 8];
        initial.d[6] = 4;
        initial.usp = 0x4000;
        initial.ssp = 0x5000;
        initial.set_status_register(sr);
        for index in 0..8 {
            initial.set_address_register(index, 0x3008 + index as u32 * 8);
        }
        Self {
            family,
            long,
            mode,
            reg,
            initial,
            source,
        }
    }

    pub fn extensions(self) -> usize {
        if self.mode == 7 && self.reg == 1 {
            2
        } else {
            usize::from(self.mode >= 5)
        }
    }

    pub fn callbacks(self) -> usize {
        self.extensions() + if self.long { 5 } else { 3 }
    }

    pub fn stride(self) -> u32 {
        if self.long { 4 } else { 2 }
    }

    pub fn operand(self) -> (u32, [u16; 2]) {
        let base = self.initial.address_register(self.reg).unwrap();
        match (self.mode, self.reg) {
            (2 | 3, _) => (base, [0, 0]),
            (4, _) => (base - self.stride(), [0, 0]),
            (5, _) => (base + 16, [16, 0]),
            (6, _) => (base + 4, [0x6000, 0]),
            (7, 0) => (0x3010, [0x3010, 0]),
            (7, 1) => (0x1234_3010, [0x1234, 0x3010]),
            _ => unreachable!(),
        }
    }

    pub fn canonical(self) -> u32 {
        self.initial.pc + 2 + self.extensions() as u32 * 2
    }

    pub fn opcode(self) -> u16 {
        self.family | if self.long { 0x80 } else { 0x40 } | self.mode << 3 | self.reg as u16
    }

    pub fn seed(self, device: &mut device::Device) -> [u16; 2] {
        device.words.fill(0x4e71);
        device.count = 0;
        device.length = 0;
        device.events.fill(device::Event::Idle(0, 0));
        let (address, extra) = self.operand();
        device.word(self.initial.pc, self.opcode());
        for (i, word) in extra.into_iter().take(self.extensions()).enumerate() {
            device.word(self.initial.pc + 2 + i as u32 * 2, word);
        }
        device.word(self.canonical(), 0x4e71);
        device.word(self.canonical() + 2, 0x7201);
        device.word(
            address,
            if self.long {
                (self.source >> 16) as u16
            } else {
                self.source as u16
            },
        );
        if self.long {
            device.word(address + 2, self.source as u16);
        }
        [
            self.opcode(),
            device.words[(self.initial.pc as usize + 2) / 2],
        ]
    }
}

pub(super) fn negate(sr: u16, source: u32, long: bool, extend: bool) -> (u32, u16) {
    let mask = if long { u32::MAX } else { 65535 };
    let sign = if long { 0x8000_0000 } else { 0x8000 };
    let source = source & mask;
    let x = u32::from(extend && sr & 16 != 0);
    let value = 0u32.wrapping_sub(source).wrapping_sub(x) & mask;
    let borrow = source != 0 || x != 0;
    let signed = if long {
        i64::from(source as i32)
    } else {
        i64::from(source as u16 as i16)
    };
    let result = -signed - i64::from(x);
    let overflow = if long {
        !(i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&result)
    } else {
        !(i64::from(i16::MIN)..=i64::from(i16::MAX)).contains(&result)
    };
    let zero = value == 0 && (!extend || sr & 4 != 0);
    (
        value,
        (sr & !31)
            | (u16::from(borrow) * 17)
            | (u16::from(value & sign != 0) * 8)
            | (u16::from(zero) * 4)
            | (u16::from(overflow) * 2),
    )
}

pub(super) fn access(address: u32, sr: u16, kind: AccessKind) -> BusAccess {
    let fc = match (sr & 0x2000 != 0, kind) {
        (false, AccessKind::Prefetch) => FunctionCode::UserProgram,
        (true, AccessKind::Prefetch) => FunctionCode::SupervisorProgram,
        (false, _) => FunctionCode::UserData,
        (true, _) => FunctionCode::SupervisorData,
    };
    BusAccess {
        logical_address: address,
        bus_address: address & 0x00ff_fffe,
        function_code: fc,
        kind,
        lanes: ByteLanes::Both,
    }
}
