use super::*;
use bulk_device::Device;
use bulk_model::{Spec, access, active, sample, set_active};

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
    pub retained: u16,
    pub fetched: u16,
    pub f_index: usize,
}

struct Builder {
    spec: Spec,
    words: BTreeMap<u32, u16>,
    mutation: Option<(usize, u32, u16)>,
    actions: Vec<Action>,
    count: usize,
    cursor: u32,
    retained: u16,
}

impl Builder {
    fn mutate(&mut self) {
        if let Some((at, address, value)) = self.mutation
            && at == self.count
        {
            self.words.insert(address & 0x00ff_fffe, value);
            self.mutation = None;
        }
    }

    fn read(&mut self, address: u32, data: bool, size: u8) -> u16 {
        self.mutate();
        let value = sample(&self.words, address);
        self.actions.push(Action::Read(
            access(self.spec.r, address, data, size),
            value,
        ));
        self.count += 1;
        if size != 1 {
            value
        } else if address & 1 == 0 {
            value >> 8
        } else {
            value & 0xff
        }
    }

    fn take(&mut self) -> u16 {
        let extension = self.retained;
        self.retained = self.read(self.cursor.wrapping_add(2), false, 2);
        self.cursor = self.cursor.wrapping_add(2);
        extension
    }

    fn state(&mut self, registers: Registers) {
        self.spec.r = registers;
        self.actions.push(Action::State(registers));
    }

    fn flags(&mut self, sr: u16) {
        let mut r = self.spec.r;
        r.set_status_register(sr);
        self.state(r);
    }

    fn write(&mut self, address: u32, result: u16, size: u8) {
        self.mutate();
        let value = if size == 1 && address & 1 == 0 {
            result << 8
        } else if size == 1 {
            result & 0xff
        } else {
            result
        };
        self.actions.push(Action::Write(
            access(self.spec.r, address, true, size),
            value,
        ));
        self.count += 1;
    }
}

pub(super) fn build(spec: Spec, device: &Device) -> Plan {
    let mut builder = Builder {
        spec,
        words: device.words.clone(),
        mutation: device.mutation,
        actions: Vec::new(),
        count: 0,
        cursor: spec.r.pc.wrapping_add(2),
        retained: spec.queue()[1],
    };
    if spec.mode == 0 {
        let operand = spec.r.d[spec.reg] & bulk_model::mask(spec.size);
        let (value, sr) =
            bulk_alu::evaluate(spec.family, spec.size, spec.r.status_register(), operand);
        let publication = matches!(spec.family, 0x4000 | 0x4400);
        if publication {
            builder.actions.push(Action::Reserve);
            let mut r = spec.r;
            let mask = if spec.size == 4 {
                0xffff
            } else {
                bulk_model::mask(spec.size)
            };
            r.d[spec.reg] = r.d[spec.reg] & !mask | value & mask;
            r.set_status_register(sr);
            builder.state(r);
        }
        let fetched = builder.read(spec.r.pc.wrapping_add(4), false, 2);
        if spec.size == 4 {
            if publication {
                let mut r = builder.spec.r;
                r.d[spec.reg] = value;
                builder.state(r);
            }
            builder.actions.push(Action::Idle(2));
        }
        let mut r = spec.r;
        r.d[spec.reg] = r.d[spec.reg] & !bulk_model::mask(spec.size) | value;
        r.set_status_register(sr);
        r.pc = spec.r.pc.wrapping_add(2);
        builder
            .actions
            .push(Action::Commit(r, [spec.retained, fetched]));
        return Plan {
            actions: builder.actions,
            address: 0,
            operand,
            retained: spec.retained,
            fetched,
            f_index: 0,
        };
    }
    let base = active(spec.r, spec.reg);
    let address = match spec.mode {
        2 | 3 => base,
        4 => {
            builder.actions.push(Action::Idle(2));
            let ea = base.wrapping_sub(spec.increment());
            let mut r = builder.spec.r;
            set_active(&mut r, spec.reg, ea);
            builder.state(r);
            ea
        }
        5 => base.wrapping_add_signed(i32::from(builder.take() as i16)),
        6 => {
            builder.actions.push(Action::Idle(2));
            let extension = builder.take();
            base.wrapping_add(bulk_model::index(spec.r, extension))
                .wrapping_add_signed(i32::from(extension as u8 as i8))
        }
        7 if spec.reg == 0 => i32::from(builder.take() as i16) as u32,
        7 => u32::from(builder.take()) << 16 | u32::from(builder.take()),
        _ => panic!("mode"),
    };
    if spec.mode == 3 && spec.size != 4 {
        let mut r = builder.spec.r;
        set_active(&mut r, spec.reg, base.wrapping_add(spec.increment()));
        builder.state(r);
    }
    let high = builder.read(address, true, spec.size.min(2));
    let operand = if spec.size == 4 {
        if spec.mode == 3 {
            let mut r = builder.spec.r;
            set_active(&mut r, spec.reg, base.wrapping_add(4));
            builder.state(r);
        }
        u32::from(high) << 16 | u32::from(builder.read(address.wrapping_add(2), true, 2))
    } else {
        u32::from(high)
    };
    let (result, low_sr, full_sr) =
        bulk_alu::stages(spec.family, spec.size, spec.r.status_register(), operand);
    if spec.staged() {
        builder.actions.push(Action::Reserve);
        builder.flags(low_sr);
    } else {
        builder.flags(full_sr);
    }
    let f_index = builder.count;
    let retained = builder.retained;
    let fetched = builder.read(builder.cursor.wrapping_add(2), false, 2);
    if spec.family != 0x4a00 {
        if spec.size == 4 {
            if spec.staged() {
                builder.actions.push(Action::Reserve);
                builder.flags(full_sr);
            }
            builder.write(address.wrapping_add(2), result as u16, 2);
            builder.write(address, (result >> 16) as u16, 2);
        } else {
            builder.write(address, result as u16, spec.size);
        }
    }
    let mut r = builder.spec.r;
    r.pc = builder.cursor;
    builder.actions.push(Action::Commit(r, [retained, fetched]));
    assert_eq!(f_index, spec.f_index());
    assert_eq!(builder.count, spec.callbacks());
    Plan {
        actions: builder.actions,
        address,
        operand,
        retained,
        fetched,
        f_index,
    }
}
