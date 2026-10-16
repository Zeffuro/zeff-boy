pub(super) fn known(w: u16) -> bool {
    let (top, op, mode, reg) = (w >> 12, w >> 6 & 7, w >> 3 & 7, w & 7);
    let data = mode == 0 || matches!(mode, 2..=6) || mode == 7 && reg <= 1;
    let source = mode != 7 || reg <= 4;
    if w == 0x4e71 || w & 0xf100 == 0x7000 || top == 6 {
        return true;
    }
    if matches!(
        w & 0xff00,
        0x4a00 | 0x0c00 | 0x4000 | 0x4200 | 0x4400 | 0x4600
    ) && w >> 6 & 3 < 3
        && data
    {
        return true;
    }
    if w & 0xffc0 == 0x40c0 && data {
        return true;
    }
    if matches!(w & 0xffc0, 0x44c0 | 0x46c0)
        && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && reg <= 4)
    {
        return true;
    }
    if top == 11 && source && (op <= 2 && (op != 0 || mode != 1) || matches!(op, 3 | 7)) {
        return true;
    }
    if matches!(top, 9 | 13) && source && matches!(op, 3 | 7) {
        return true;
    }
    if matches!(top, 8 | 9 | 12 | 13)
        && op <= 2
        && source
        && !(mode == 1 && (op == 0 || matches!(top, 8 | 12)))
    {
        return true;
    }
    matches!(top, 1..=3)
        && source
        && (w >> 6 & 7 != 7 || w >> 9 & 7 <= 1)
        && !(top == 1 && (mode == 1 || w >> 6 & 7 == 1))
}

pub(super) fn selected(w: u16, supervisor: bool) -> bool {
    let (top, op, mode, reg) = (w >> 12, w >> 6 & 7, w >> 3 & 7, w & 7);
    let memory = matches!(mode, 2..=6) || mode == 7 && reg <= 3;
    let alterable = matches!(mode, 2..=6) || mode == 7 && reg <= 1;
    w == 0x4e71
        || w & 0xf100 == 0x7000
        || w & 0xff00 == 0x6000
        || matches!(top, 1..=3) && known(w)
        || w & 0xff00 == 0x4a00 && w >> 6 & 3 < 3 && (mode == 0 || alterable)
        || w & 0xff00 == 0x0c00 && w >> 6 & 3 < 3 && (mode == 0 || alterable)
        || matches!(w & 0xff00, 0x4000 | 0x4200 | 0x4400 | 0x4600)
            && matches!(w >> 6 & 3, 1 | 2)
            && alterable
        || matches!(w & 0xff00, 0x4000 | 0x4200 | 0x4400 | 0x4600)
            && w >> 6 & 3 < 3
            && (mode == 0 || alterable)
        || top == 11 && known(w)
        || matches!(top, 9 | 13) && matches!(op, 3 | 7) && memory
        || matches!(top, 8 | 9 | 12 | 13) && matches!(op, 1 | 2) && memory
        || (w & 0xffc0 == 0x44c0 || supervisor && w & 0xffc0 == 0x46c0)
            && (mode == 0 || memory || mode == 7 && reg == 4)
        || w & 0xffc0 == 0x40c0 && (mode == 0 || alterable)
}
