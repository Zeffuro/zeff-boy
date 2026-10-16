use super::*;
use device::Device;
use model::{Spec, access, active, mask, sample, set_active, status};

#[derive(Clone, Debug)]
pub(super) enum Action {
    Idle(u32),
    State(Registers),
    Reserve,
    Read(BusAccess, u16),
    Write(BusAccess, u16),
    Commit(Registers, [u16; 2]),
}
#[derive(Clone, Debug)]
pub(super) struct Plan {
    pub actions: Vec<Action>,
    pub address: u32,
    pub operand: u32,
    pub immediate: u32,
    pub final_read: usize,
}
struct Builder {
    s: Spec,
    words: BTreeMap<u32, u16>,
    mutation: Option<(usize, u32, u16)>,
    actions: Vec<Action>,
    count: usize,
    cursor: u32,
    retained: u16,
}
impl Builder {
    fn read(&mut self, address: u32, data: bool, size: u8) -> u16 {
        if let Some((at, location, value)) = self.mutation
            && at == self.count
        {
            self.words.insert(location & 0x00ff_fffe, value);
            self.mutation = None;
        }
        let value = sample(&self.words, address);
        self.actions
            .push(Action::Read(access(self.s.r, address, data, size), value));
        self.count += 1;
        if size == 1 {
            if address & 1 == 0 {
                value >> 8
            } else {
                value & 0xff
            }
        } else {
            value
        }
    }
    fn take(&mut self) -> u16 {
        let value = self.retained;
        self.retained = self.read(self.cursor.wrapping_add(2), false, 2);
        self.cursor = self.cursor.wrapping_add(2);
        value
    }
    fn state(&mut self, r: Registers) {
        self.s.r = r;
        self.actions.push(Action::State(r));
    }
    fn write(&mut self, address: u32, value: u16, size: u8) {
        let value = if size == 1 && address & 1 == 0 {
            (value & 0xff) << 8
        } else if size == 1 {
            value & 0xff
        } else {
            value
        };
        self.actions
            .push(Action::Write(access(self.s.r, address, true, size), value));
        self.count += 1;
    }
}
pub(super) fn build(s: Spec, bus: &Device) -> Plan {
    let mut b = Builder {
        s,
        words: bus.words.clone(),
        mutation: bus.mutation,
        actions: Vec::new(),
        count: 0,
        cursor: s.r.pc.wrapping_add(2),
        retained: s.queue()[1],
    };
    let first = b.take();
    let immediate = if s.size == 4 {
        u32::from(first) << 16 | u32::from(b.take())
    } else {
        u32::from(first) & mask(s.size)
    };
    let base = active(s.r, s.reg);
    let address = if s.mode == 0 {
        0
    } else if s.mode == 4 {
        b.actions.push(Action::Idle(2));
        let address = base.wrapping_sub(s.increment());
        let mut r = b.s.r;
        set_active(&mut r, s.reg, address);
        b.state(r);
        address
    } else if s.mode >= 5 {
        if s.mode == 6 {
            b.actions.push(Action::Idle(2));
        }
        let first = b.take();
        match s.mode {
            5 => base.wrapping_add_signed(i32::from(first as i16)),
            6 => base
                .wrapping_add(model::index(s.r, first))
                .wrapping_add_signed(i32::from(first as u8 as i8)),
            7 if s.reg == 0 => i32::from(first as i16) as u32,
            7 => u32::from(first) << 16 | u32::from(b.take()),
            _ => unreachable!(),
        }
    } else {
        base
    };
    let operand = if s.mode == 0 {
        s.r.d[s.reg] & mask(s.size)
    } else {
        if s.mode == 3 && s.size != 4 {
            let mut r = b.s.r;
            set_active(&mut r, s.reg, base.wrapping_add(s.increment()));
            b.state(r);
        }
        let high = b.read(address, true, s.size.min(2));
        if s.size == 4 {
            if s.mode == 3 {
                let mut r = b.s.r;
                set_active(&mut r, s.reg, base.wrapping_add(4));
                b.state(r);
            }
            u32::from(high) << 16 | u32::from(b.read(address.wrapping_add(2), true, 2))
        } else {
            u32::from(high)
        }
    };
    if s.family == 0x0c00 {
        b.actions.push(Action::Reserve);
    }
    let (result, sr) = arithmetic::evaluate(s, operand, immediate);
    let mut r = status(b.s.r, sr);
    if s.mode == 0 && s.family != 0x0c00 {
        r.d[s.reg] = r.d[s.reg] & !mask(s.size) | result & mask(s.size);
    }
    b.state(r);
    let final_read = b.count;
    let n = b.retained;
    let f = b.read(b.cursor.wrapping_add(2), false, 2);
    if s.mode == 0 && s.size == 4 {
        b.actions
            .push(Action::Idle(if s.family == 0x0c00 { 2 } else { 4 }));
    } else if s.mode != 0 && s.family != 0x0c00 {
        if s.size == 4 {
            b.write(address.wrapping_add(2), result as u16, 2);
            b.write(address, (result >> 16) as u16, 2);
        } else {
            b.write(address, result as u16, s.size);
        }
    }
    r = b.s.r;
    r.pc = b.cursor;
    b.actions.push(Action::Commit(r, [n, f]));
    Plan {
        actions: b.actions,
        address,
        operand,
        immediate,
        final_read,
    }
}
pub(super) fn validate(s: Spec, bus: &Device) -> Plan {
    let p = build(s, bus);
    assert_eq!(p.address, model::address(s, s.extension));
    assert_eq!(p.operand, s.operand & mask(s.size));
    assert_eq!(p.immediate, s.immediate & mask(s.size));
    p
}
