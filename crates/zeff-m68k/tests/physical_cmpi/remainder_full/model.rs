use super::*;
pub(super) fn each(mut f: impl FnMut(Case)) {
    for bank in [false, true] {
        for word in remainder::WORDS {
            for lane in 0..if word >> 3 & 7 == 0 { 1 } else { 2 } {
                f(remainder::case(word, bank, lane));
            }
        }
    }
}
pub(super) fn values(size: Size) -> [(u32, u32); 12] {
    let (sign, mask) = match size {
        Size::Byte => (128, 255),
        Size::Word => (32768, 65535),
        Size::Long => (0x8000_0000, u32::MAX),
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
        (mask, mask),
    ]
}
pub(super) fn pair(mut c: Case, pair: (u32, u32), high: u16) -> Case {
    c.value = pair.0;
    c.immediate = pair.1
        | if c.form.size == Size::Byte {
            u32::from(high & 0xff00)
        } else {
            0
        };
    if c.form.ea == Ea::Dn {
        c.r.d[usize::from(c.register)] = pair.0
            | match c.form.size {
                Size::Byte => 0xa5ff_ff00,
                Size::Word => 0xffa5_0000,
                Size::Long => 0,
            };
    }
    c
}
pub(super) fn status(mut c: Case, ccr: u16) -> Case {
    c.r.set_status_register((c.r.status_register() & !31) | ccr);
    c
}
