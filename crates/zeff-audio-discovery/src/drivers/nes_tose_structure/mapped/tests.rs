use std::sync::atomic::AtomicBool;

use super::*;

fn selector(kind: Kind, table: u16, masks: u16) -> Vec<u8> {
    let [lo, hi] = table.to_le_bytes();
    let [mask_lo, mask_hi] = masks.to_le_bytes();
    let (ram, zp, mask) = if kind == Kind::AccumulatorPointer {
        (0x20, 0x8b, 0xcc)
    } else {
        (0, 0x5f, 0xac)
    };
    if kind == Kind::YPointerHelper {
        return vec![
            0x84, 1, 0x84, 4, 0x86, 3, 0xa9, 0, 0xa8, 0x06, 1, 0x2a, 0x06, 1, 0x2a, 0x85, 2, 0xa9,
            lo, 0x18, 0x65, 1, 0x85, 1, 0xa9, hi, 0x65, 2, 0x85, 2, 0xb1, 1, 0xaa, 0xbd, 0, 4,
            0xc9, 0xff, 0xf0, 15, 0xbd, 1, 4, 0x29, 3, 0xa8, 0xb9, mask_lo, mask_hi, 0x2d, 0xac, 4,
            0x8d, 0xac, 4, 0xa0, 1, 0xb1, 1, 0x9d, 1, 4, 0xc8, 0xb1, 1, 0x9d, 2, 4, 0xc8, 0xb1, 1,
            0x9d, 3, 4, 0xa9, 0, 0x9d, 0, 4, 0xa6, 3, 0xa4, 4, 0xc8, 0x60,
        ];
    }
    vec![
        0xa0,
        0,
        0x84,
        zp + 1,
        0x0a,
        0x26,
        zp + 1,
        0x0a,
        0x26,
        zp + 1,
        0x18,
        0x69,
        lo,
        0x85,
        zp,
        0xa9,
        hi,
        0x65,
        zp + 1,
        0x85,
        zp + 1,
        0x8a,
        0x48,
        0xb1,
        zp,
        0xaa,
        0xbd,
        ram,
        7,
        0xc9,
        0xff,
        0xf0,
        15,
        0xbd,
        ram + 1,
        7,
        0x29,
        3,
        0xa8,
        0xb9,
        mask_lo,
        mask_hi,
        0x2d,
        mask,
        7,
        0x8d,
        mask,
        7,
        0xa0,
        1,
        0xb1,
        zp,
        0x9d,
        ram + 1,
        7,
        0xc8,
        0xb1,
        zp,
        0x9d,
        ram + 2,
        7,
        0xc8,
        0xb1,
        zp,
        0x9d,
        ram + 3,
        7,
        0xa9,
        0,
        0x9d,
        ram,
        7,
        0x68,
        0xaa,
        0x60,
    ]
}

fn fixture(kind: Kind, base: usize, width: usize) -> (Vec<u8>, Engine) {
    let mut bank = vec![0; width];
    let config = contract(kind);
    let tick = 0x200;
    let masks = tick
        + match kind {
            Kind::AccumulatorPointer => 0x413,
            Kind::YPointerHelper => 0x6af,
            Kind::VariableSlotLimit => 0x691,
            _ => unreachable!(),
        };
    let code = selector(kind, (base + 0x1000) as u16, (base + masks) as u16);
    bank[0x100..0x100 + code.len()].copy_from_slice(&code);
    bank[0x80..0x80 + config.init.len()].copy_from_slice(&config.init);
    bank[masks..masks + 4].copy_from_slice(&config.masks);
    let engine = Engine {
        cpu_base: base,
        kind,
        tick,
        end: 0x600,
        code_ranges: vec![(tick, 0x600)],
        writers: vec![],
        routing: None,
    };
    (bank, engine)
}

fn check(bank: &[u8], engine: Engine) -> Option<Layout> {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    resolve(bank, engine, &mut budget).unwrap()
}

#[test]
fn mapped_selector_contracts_derive_full_input_domain_and_mask_evidence() {
    for (kind, base, width) in [
        (Kind::AccumulatorPointer, 0xc000, 0x4000),
        (Kind::VariableSlotLimit, 0xc000, 0x4000),
        (Kind::YPointerHelper, 0xa000, 0x2000),
    ] {
        let (bank, engine) = fixture(kind, base, width);
        let layout = check(&bank, engine).unwrap();
        assert_eq!(layout.table, 0x1000);
        assert_eq!(layout.selectors, 256);
        assert!(layout.selector_data.is_some());
    }
}

#[test]
fn mapped_selector_rejects_ram_mask_pointer_and_duplicate_abi_changes() {
    for mutation in 0..5 {
        let (mut bank, engine) = fixture(Kind::AccumulatorPointer, 0xc000, 0x4000);
        match mutation {
            0 => bank[0x100 + 27] ^= 1,
            1 => bank[0x613] ^= 1,
            2 => bank[0x100 + 16] = 0x80,
            3 => {
                let init = contract(Kind::AccumulatorPointer).init;
                bank[0x900..0x900 + init.len()].copy_from_slice(&init);
            }
            _ => bank[0x80 + 3] ^= 1,
        }
        assert!(check(&bank, engine).is_none());
    }
}
