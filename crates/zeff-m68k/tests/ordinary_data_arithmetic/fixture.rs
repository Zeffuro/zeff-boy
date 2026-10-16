use super::*;

pub(super) const OPS: [u16; 4] = [13, 9, 12, 8];
pub(super) const STATUSES: [u16; 4] = [0, 0x0115, 0x041a, 0x070f];

#[derive(Clone, Copy, Debug)]
pub(super) struct Case {
    pub operation: u16,
    pub long: bool,
    pub ea: u8,
    pub source: usize,
    pub destination: usize,
    pub supervisor: bool,
    pub status: u16,
    pub value: u32,
    pub desired: u32,
    pub extension: u16,
    pub absolute: u32,
    pub pc: u32,
    pub start: u64,
    pub wait: u32,
    pub failure: Option<usize>,
    pub mutate: Option<(usize, u16)>,
    pub source_base: Option<u32>,
    pub d0: Option<u32>,
    pub extra: Option<(u32, u32)>,
}

impl Default for Case {
    fn default() -> Self {
        Self {
            operation: 13,
            long: false,
            ea: 2,
            source: 7,
            destination: 5,
            supervisor: false,
            status: 0x071f,
            value: 2,
            desired: 0x7ffe,
            extension: 0xa870,
            absolute: 0xabcd_6000,
            pc: PC,
            start: START,
            wait: 0,
            failure: None,
            mutate: None,
            source_base: None,
            d0: None,
            extra: None,
        }
    }
}

impl Case {
    pub fn extensions(self) -> usize {
        if self.ea == 8 {
            2
        } else {
            usize::from(self.ea >= 5)
        }
    }
    pub fn count(self) -> usize {
        self.extensions() + 2 + usize::from(self.long)
    }
    pub fn stride(self) -> u32 {
        if self.long { 4 } else { 2 }
    }
    pub fn idle(self) -> u64 {
        if matches!(self.ea, 4 | 6 | 10) { 2 } else { 0 }
    }
    pub fn tail(self) -> u64 {
        if self.long { 2 } else { 0 }
    }
    pub fn end(self) -> u32 {
        self.pc.wrapping_add(2 + 2 * self.extensions() as u32)
    }
    pub fn opcode(self) -> u16 {
        self.operation << 12
            | (self.destination as u16) << 9
            | if self.long { 0x80 } else { 0x40 }
            | if self.ea <= 6 {
                u16::from(self.ea) << 3 | self.source as u16
            } else {
                0x38 + u16::from(self.ea - 7)
            }
    }
    pub fn initial(self) -> Registers {
        let mut r = Registers::new();
        r.d = std::array::from_fn(|i| 0xaabb_9000 + i as u32 * 2);
        r.a = std::array::from_fn(|i| 0x1234_6000 + i as u32 * 32);
        r.usp = 0x3456_7000;
        r.ssp = 0x8765_8000;
        r.pc = self.pc;
        r.set_status_register(self.status | if self.supervisor { 0x2000 } else { 0 });
        r.d[self.destination] = if self.long {
            self.desired
        } else {
            0xaabb_0000 | (self.desired & 65535)
        };
        if let Some(base) = self.source_base {
            oracle::set_an(&mut r, self.source, base);
        }
        if let Some(value) = self.d0 {
            r.d[0] = value;
        }
        r
    }
    pub fn retained(self) -> u16 {
        match self.ea {
            2..=4 => NEXT,
            7 => self.absolute as u16,
            8 => (self.absolute >> 16) as u16,
            _ => self.extension,
        }
    }
    pub fn memory(self, r: Registers) -> BTreeMap<u32, u16> {
        let mut words = BTreeMap::new();
        put(&mut words, self.pc, self.opcode());
        put(&mut words, self.pc.wrapping_add(2), self.retained());
        if self.ea == 8 {
            put(&mut words, self.pc.wrapping_add(4), self.absolute as u16);
        }
        put(&mut words, self.end(), NEXT);
        put(&mut words, self.end().wrapping_add(2), FETCH);
        let address = oracle::address(self, r, self.absolute as u16);
        self.operand(&mut words, address, self.value);
        if let Some((address, value)) = self.extra {
            self.operand(&mut words, address, value);
        }
        words
    }
    fn operand(self, words: &mut BTreeMap<u32, u16>, address: u32, value: u32) {
        put(
            words,
            address,
            if self.long {
                (value >> 16) as u16
            } else {
                value as u16
            },
        );
        if self.long {
            put(words, address.wrapping_add(2), value as u16);
        }
    }
}

pub(super) fn pairs(long: bool) -> [(u32, u32); 12] {
    let (sign, mask) = if long {
        (0x8000_0000, u32::MAX)
    } else {
        (0x8000, 65535)
    };
    [
        (0, 0),
        (0, 1),
        (1, 0),
        (1, mask),
        (mask, 1),
        (sign - 1, 1),
        (sign, sign),
        (sign, sign - 1),
        (sign - 1, sign),
        (mask, sign - 1),
        (sign + 1, 0),
        (if long { 0x10000 } else { 0x7ffe }, 0),
    ]
}

pub(super) fn index_pairs(long: bool) -> [(u32, u32); 4] {
    let (sign, mask) = if long {
        (0x8000_0000, u32::MAX)
    } else {
        (0x8000, 65535)
    };
    [
        (0, 0),
        (1, mask),
        (sign, sign),
        (if long { 0x10000 } else { 0x8001 }, 0),
    ]
}
