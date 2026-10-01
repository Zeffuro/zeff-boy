use crate::{Budget, ScanStop};

pub(super) const ENGINE_LEN: u16 = 0x9a8;
pub(super) const COLD_LEN: u16 = 0x84;
pub(super) const BOOTSTRAP: u16 = 0xf800;
const ENGINE_SHA: &str = "482ea6cc31954b8f72a360b2ca55594301f779d1d58575e06110f7beae62ec7c";
const WRAPPER_SHA: &str = "3c52f1df5e990c838e4654da95a3c13d2f55919fa6067da580b678a1053b7081";

pub(super) const WORDS: &[(usize, u16)] = &[
    (0x038, 0x04a),
    (0x054, 0x8fe),
    (0x057, 0x05c),
    (0x06b, 0x8fe),
    (0x082, 0x82c),
    (0x0b1, 0x82c),
    (0x2ee, 0x2fa),
    (0x324, 0x337),
    (0x3cc, 0x3df),
    (0x5b2, 0x25d),
    (0x5c6, 0x822),
    (0x5de, 0x7c7),
    (0x5fa, 0x822),
    (0x612, 0x7c7),
    (0x62e, 0x822),
    (0x646, 0x7c7),
    (0x662, 0x822),
    (0x67a, 0x7c7),
    (0x6a3, 0x25d),
    (0x6ce, 0x7c7),
    (0x6f2, 0x79c),
    (0x70a, 0x7c7),
    (0x72e, 0x79c),
    (0x746, 0x7c7),
    (0x77f, 0x7c7),
    (0x855, 0x268),
    (0x85a, 0x27f),
    (0x85f, 0x8fa),
    (0x86a, 0x82c),
    (0x871, 0x260),
    (0x876, 0x264),
    (0x87b, 0x8fa),
    (0x944, 0x947),
];
pub(super) const SPLITS: &[(usize, usize, u16)] = &[
    (0x028, 0x02c, 0x0dd),
    (0x030, 0x034, 0x13d),
    (0x03b, 0x03f, 0x19d),
    (0x043, 0x047, 0x1fd),
    (0x260, 0x264, 0x296),
    (0x261, 0x265, 0x296),
    (0x262, 0x266, 0x35c),
    (0x263, 0x267, 0x3e3),
    (0x268, 0x27f, 0x4b8),
    (0x269, 0x280, 0x4b8),
    (0x26a, 0x281, 0x4b8),
    (0x26b, 0x282, 0x4b8),
    (0x26c, 0x283, 0x4b8),
    (0x26d, 0x284, 0x4b8),
    (0x26e, 0x285, 0x4b8),
    (0x26f, 0x286, 0x4b8),
    (0x270, 0x287, 0x4b8),
    (0x271, 0x288, 0x4b8),
    (0x272, 0x289, 0x4b8),
    (0x273, 0x28a, 0x4b8),
    (0x274, 0x28b, 0x4b8),
    (0x275, 0x28c, 0x4b8),
    (0x276, 0x28d, 0x4b8),
    (0x277, 0x28e, 0x4b8),
    (0x278, 0x28f, 0x4d0),
    (0x279, 0x290, 0x4f5),
    (0x27a, 0x291, 0x480),
    (0x27b, 0x292, 0x512),
    (0x27c, 0x293, 0x544),
    (0x27d, 0x294, 0x585),
    (0x27e, 0x295, 0x592),
];

pub(super) struct Layout {
    pub reset: u16,
    pub engine: u16,
    pub initialize: u16,
    pub select: u16,
    pub tick: u16,
    pub root: u16,
    pub instruments: u16,
}

fn word(bytes: &[u8], first: usize, second: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        *bytes.get(first)?,
        *bytes.get(second)?,
    ]))
}

pub(super) fn inspect(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<Layout>, ScanStop> {
    budget.charge()?;
    if bytes.len() != 0xa010
        || bytes[..16] != [b'N', b'E', b'S', 0x1a, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        || bytes[0x7810..0x7910].iter().any(|&byte| byte != 0xea)
    {
        return Ok(None);
    }
    if word(bytes, 0x800a, 0x800b) != Some(0xc076)
        || word(bytes, 0x800c, 0x800d) != Some(0xc000)
        || word(bytes, 0x800e, 0x800f) != Some(0xc083)
    {
        return Ok(None);
    }
    let mut wrapper = bytes[0x4010..0x4010 + usize::from(COLD_LEN)].to_vec();
    let (Some(root), Some(instruments), Some(engine), Some(select), Some(update), Some(upload)) = (
        word(&wrapper, 0x4e, 0x52),
        word(&wrapper, 0x5c, 0x60),
        word(&wrapper, 0x64, 0x65),
        word(&wrapper, 0x6d, 0x6e),
        word(&wrapper, 0x71, 0x72),
        word(&wrapper, 0x74, 0x75),
    ) else {
        return Ok(None);
    };
    let Some(engine_end) = engine.checked_add(ENGINE_LEN) else {
        return Ok(None);
    };
    if !(0x8000..0xc000).contains(&root)
        || root & 0xff != 0
        || instruments != root + 4
        || root >> 8 != instruments >> 8
        || engine < 0xc000 + COLD_LEN
        || engine_end >= BOOTSTRAP
        || select != engine + 0x5a9
        || update != engine + 0x72
        || upload != engine + 0x93f
        || wrapper[0x3d] > 1
        || bytes[usize::from(engine_end - 0x8000) + 16] != 0xea
    {
        return Ok(None);
    }
    for offset in [
        0x3d, 0x4e, 0x52, 0x5c, 0x60, 0x64, 0x65, 0x6d, 0x6e, 0x71, 0x72, 0x74, 0x75,
    ] {
        wrapper[offset] = 0;
    }
    budget.charge()?;
    if zeff_firmware::sha256_hex(&wrapper) != WRAPPER_SHA {
        return Ok(None);
    }
    let start = usize::from(engine - 0x8000) + 16;
    let Some(engine_bytes) = bytes.get(start..start + usize::from(ENGINE_LEN)) else {
        return Ok(None);
    };
    let mut normalized = engine_bytes.to_vec();
    for &(operand, target) in WORDS {
        budget.charge()?;
        if word(&normalized, operand, operand + 1) != engine.checked_add(target) {
            return Ok(None);
        }
        normalized[operand..operand + 2].copy_from_slice(&target.to_le_bytes());
    }
    for &(low, high, target) in SPLITS {
        budget.charge()?;
        if word(&normalized, low, high) != engine.checked_add(target) {
            return Ok(None);
        }
        normalized[low] = target as u8;
        normalized[high] = (target >> 8) as u8;
    }
    for _ in normalized.chunks(256) {
        budget.charge()?;
    }
    Ok(
        (zeff_firmware::sha256_hex(&normalized) == ENGINE_SHA).then_some(Layout {
            reset: 0xc000,
            engine,
            initialize: 0xc049,
            select: 0xc066,
            tick: 0xc070,
            root,
            instruments,
        }),
    )
}
