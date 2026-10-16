use super::{Builder, Case, flags};
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, ByteLanes};

fn result(family: &str, value: u32, sr: u16, mask: u32, sign: u32) -> (u32, u16) {
    let x = sr & 16 != 0;
    if family == "NOT" {
        let result = !value & mask;
        return (
            result,
            flags(sr, result, sign, (x, false, false, result == 0)),
        );
    }
    let extend = family == "NEGX";
    let borrow = u64::from(value) + u64::from(extend && x) != 0;
    let result = 0u32
        .wrapping_sub(value)
        .wrapping_sub(u32::from(extend && x))
        & mask;
    let overflow = value & sign != 0 && result & sign != 0;
    let zero = result == 0 && (!extend || sr & 4 != 0);
    (
        result,
        flags(sr, result, sign, (borrow, borrow, overflow, zero)),
    )
}

pub(super) fn derive(case: &Case, b: &mut Builder<'_>) -> Result<(Registers, [u16; 2]), String> {
    let original = case.initial.registers()?;
    let mut expected = original;
    let op = case.initial.prefetch[0];
    let size = (op >> 6) & 3;
    let mode = (op >> 3) & 7;
    let r = usize::from(op & 7);
    if size == 3 || mode == 1 || mode == 7 && r > 1 {
        return Err("invalid ordinary unary operand".into());
    }
    let width = 1u32 << size;
    let mask = if size == 2 {
        u32::MAX
    } else {
        (1u32 << (width * 8)) - 1
    };
    let sign = 1u32 << (width * 8 - 1);
    let family = case.cohort.split('.').next().unwrap();
    let pfc = if original.supervisor() { 6 } else { 2 };
    if mode == 0 {
        let value = original.d[r] & mask;
        let (result, sr) = result(family, value, original.status_register(), mask, sign);
        let tail = b.fetch(original.pc.wrapping_add(4), pfc)?;
        if size == 2 {
            b.idle(2)?;
        }
        expected.d[r] = (original.d[r] & !mask) | result;
        expected.set_status_register(sr);
        expected.pc = original.pc.wrapping_add(2);
        return Ok((expected, [case.initial.prefetch[1], tail]));
    }
    let base = original.address_register(r).unwrap();
    let increment = if size == 0 && r == 7 { 2 } else { width };
    let extension = case.initial.prefetch[1];
    let index_register = usize::from((extension >> 12) & 7);
    let index_value = if extension & 0x8000 != 0 {
        original.address_register(index_register).unwrap()
    } else {
        original.d[index_register]
    };
    let index = if extension & 0x800 != 0 {
        index_value as i32
    } else {
        i32::from(index_value as i16)
    };
    let indexed = |a: u32| {
        a.wrapping_add_signed(index)
            .wrapping_add_signed(i32::from(extension as u8 as i8))
    };
    let (address, extensions) = match (mode, r) {
        (2 | 3, _) => (base, 0),
        (4, _) => (base.wrapping_sub(increment), 0),
        (5, _) => (base.wrapping_add_signed(i32::from(extension as i16)), 1),
        (6, _) => (indexed(base), 1),
        (7, 0) => (i32::from(extension as i16) as u32, 1),
        (7, 1) => (
            (u32::from(extension) << 16) | u32::from(b.word(original.pc.wrapping_add(4))?),
            2,
        ),
        _ => return Err("invalid ordinary unary EA".into()),
    };
    if mode == 4 || mode == 6 {
        b.idle(2)?;
    }
    let mut first = case.initial.prefetch[1];
    for n in 0..extensions {
        first = b.fetch(original.pc.wrapping_add(4 + n * 2), pfc)?;
    }
    let dfc = pfc - 1;
    let lanes = if size != 0 {
        ByteLanes::Both
    } else if address & 1 == 0 {
        ByteLanes::Upper
    } else {
        ByteLanes::Lower
    };
    let bus_address = address & !1;
    let high = b.transfer(AccessKind::Data, bus_address, dfc, lanes, None)?;
    let value = if size == 2 {
        (u32::from(high) << 16)
            | u32::from(b.transfer(
                AccessKind::Data,
                address.wrapping_add(2),
                dfc,
                ByteLanes::Both,
                None,
            )?)
    } else if size == 0 {
        u32::from(if lanes == ByteLanes::Upper {
            high >> 8
        } else {
            high
        })
    } else {
        u32::from(high)
    };
    if mode == 3 {
        expected.set_address_register(r, base.wrapping_add(increment));
    }
    if mode == 4 {
        expected.set_address_register(r, address);
    }
    let (result, sr) = result(family, value, original.status_register(), mask, sign);
    expected.set_status_register(sr);
    expected.pc = original.pc.wrapping_add(2 + extensions * 2);
    let tail = b.fetch(expected.pc.wrapping_add(2), pfc)?;
    if size == 2 {
        b.transfer(
            AccessKind::Data,
            address.wrapping_add(2),
            dfc,
            ByteLanes::Both,
            Some(result as u16),
        )?;
        b.transfer(
            AccessKind::Data,
            address,
            dfc,
            ByteLanes::Both,
            Some((result >> 16) as u16),
        )?;
    } else {
        let data = if lanes == ByteLanes::Upper {
            (result as u16) << 8
        } else {
            result as u16
        };
        b.transfer(AccessKind::Data, bus_address, dfc, lanes, Some(data))?;
    }
    Ok((expected, [first, tail]))
}
