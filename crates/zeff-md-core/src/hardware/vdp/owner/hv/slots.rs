pub(super) fn available(h: u16, wide: bool, active: bool, m5: bool) -> bool {
    if h & 1 != 0 {
        return false;
    }
    if !active {
        return h & 63 != 50;
    }
    if !m5 {
        return ![
            (263, 6),
            (263, 4),
            (271, 8),
            (271, 0),
            (509, 480),
            (509, 468),
            (509, 276),
            (509, 264),
            (509, 272),
            (509, 488),
            (509, 476),
            (509, 284),
            (509, 484),
            (509, 472),
            (509, 280),
            (509, 268),
            (279, 18),
            (287, 10),
            (497, 496),
        ]
        .iter()
        .any(|&(mask, value)| h & mask == value);
    }
    let common = [486, 510, 502, 508, 500, 504, 496].contains(&h)
        || [
            (271, 10),
            (503, 498),
            (505, 488),
            (509, 480),
            (497, 464),
            (263, 6),
            (263, 4),
            (271, 8),
            (271, 0),
            (319, 50),
        ]
        .iter()
        .any(|&(mask, value)| h & mask == value);
    let width = if wide {
        [484, 460, 458, 364, 306].contains(&h)
            || [
                (463, 266),
                (505, 344),
                (509, 360),
                (505, 352),
                (505, 336),
                (505, 328),
                (509, 324),
                (455, 262),
                (455, 260),
                (463, 264),
                (463, 256),
            ]
            .iter()
            .any(|&(mask, value)| h & mask == value)
    } else {
        h == 290
            || [(509, 292), (505, 280), (505, 264), (509, 260), (505, 272)]
                .iter()
                .any(|&(mask, value)| h & mask == value)
    };
    !(common || width)
}
pub(super) fn refresh(h: u16, wide: bool, active: bool, m5: bool) -> bool {
    if active {
        m5 && (wide && h == 306 || h & 319 == 50)
    } else {
        h & 63 == 50
    }
}
