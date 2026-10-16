use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct Case {
    pub subtract: bool,
    pub long: bool,
    pub family: u8,
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
            subtract: false,
            long: false,
            family: 2,
            source: 7,
            destination: 1,
            supervisor: false,
            status: 0x071f,
            value: 2,
            desired: 0x1fffe,
            extension: 0x0870,
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
        if self.family == 8 {
            2
        } else {
            usize::from(self.family >= 5)
        }
    }
    pub fn count(self) -> usize {
        self.extensions() + 2 + usize::from(self.long)
    }
    pub fn stride(self) -> u32 {
        if self.long { 4 } else { 2 }
    }
    pub fn idle(self) -> u64 {
        if matches!(self.family, 4 | 6 | 10) {
            2
        } else {
            0
        }
    }
    pub fn tail(self) -> u64 {
        if self.long { 2 } else { 4 }
    }
    pub fn end(self) -> u32 {
        self.pc.wrapping_add(2 + 2 * self.extensions() as u32)
    }
    pub fn opcode(self) -> u16 {
        (if self.subtract { 0x9000 } else { 0xd000 })
            | (self.destination as u16) << 9
            | if self.long { 0x1c0 } else { 0xc0 }
            | if self.family <= 6 {
                u16::from(self.family) << 3 | self.source as u16
            } else {
                0x38 + u16::from(self.family - 7)
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
        let desired = if self.source == self.destination {
            if self.family == 3 {
                self.desired.wrapping_sub(self.stride())
            } else if self.family == 4 {
                self.desired.wrapping_add(self.stride())
            } else {
                self.desired
            }
        } else {
            self.desired
        };
        r.set_address_register(self.destination, desired);
        if let Some(base) = self.source_base {
            r.set_address_register(self.source, base);
        }
        if let Some(value) = self.d0 {
            r.d[0] = value;
        }
        r
    }
    pub fn retained(self) -> u16 {
        match self.family {
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
        if self.family == 8 {
            put(&mut words, self.pc.wrapping_add(4), self.absolute as u16);
        }
        put(&mut words, self.end(), NEXT);
        put(&mut words, self.end().wrapping_add(2), FETCH);
        let ea = oracle::address(self, r, self.absolute as u16);
        put(
            &mut words,
            ea,
            if self.long {
                (self.value >> 16) as u16
            } else {
                self.value as u16
            },
        );
        if self.long {
            put(&mut words, ea.wrapping_add(2), self.value as u16);
        }
        if let Some((a, value)) = self.extra {
            put(
                &mut words,
                a,
                if self.long {
                    (value >> 16) as u16
                } else {
                    value as u16
                },
            );
            if self.long {
                put(&mut words, a.wrapping_add(2), value as u16);
            }
        }
        words
    }
}

pub(super) fn pairs(subtract: bool, long: bool) -> [(u32, u32); 4] {
    [
        (0, 0),
        (2, 2),
        (if long { 0x8000_0001 } else { 0x8001 }, 0),
        (2, if subtract { 0x20000 } else { 0x1fffe }),
    ]
}
