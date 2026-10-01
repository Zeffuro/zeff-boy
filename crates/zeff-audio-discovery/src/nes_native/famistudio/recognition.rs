use crate::{Budget, ScanStop};

pub(super) const ENGINE_LEN: u16 = 0x57b;
pub(super) const BOOTSTRAP: u16 = 0xf800;
const ENGINE_SHA: &str = "898165a13fa8d9b846cd2f4a8c044b8ebee65690a1970dcc40df72ef1871084d";
const WRAPPER_SHA: &str = "57ebb534059bb597846f43c1a5fd4738eb8ea4e2bc54e35df40268c87cedce82";

pub(super) const WORDS: &[(usize, u16)] = &[
    (0x010, 0x058),
    (0x056, 0x058),
    (0x0ae, 0x49c),
    (0x0d1, 0x058),
    (0x115, 0x49c),
    (0x141, 0x4ab),
    (0x148, 0x50c),
    (0x157, 0x226),
    (0x16e, 0x38e),
    (0x1a2, 0x18e),
    (0x1b4, 0x1b6),
    (0x1e8, 0x1ec),
    (0x1f1, 0x21d),
    (0x20c, 0x21d),
    (0x215, 0x1d3),
    (0x22c, 0x24d),
    (0x236, 0x135),
    (0x251, 0x577),
    (0x25c, 0x27d),
    (0x266, 0x135),
    (0x281, 0x577),
    (0x28c, 0x2a5),
    (0x296, 0x135),
    (0x2b0, 0x2cd),
    (0x2c0, 0x577),
    (0x2d1, 0x577),
    (0x2f2, 0x56d),
    (0x319, 0x572),
    (0x35d, 0x361),
    (0x364, 0x572),
    (0x37b, 0x56d),
    (0x38c, 0x331),
    (0x3b8, 0x472),
    (0x3bd, 0x487),
    (0x3ce, 0x376),
    (0x3d5, 0x3a9),
    (0x3ef, 0x2f1),
    (0x41b, 0x3e2),
    (0x443, 0x3a9),
    (0x455, 0x3a9),
    (0x45e, 0x3a9),
    (0x466, 0x3a9),
    (0x46f, 0x3a9),
];
pub(super) const SPLITS: &[(usize, usize, u16)] = &[
    (0x073, 0x078, 0x4a2),
    (0x08f, 0x094, 0x4a6),
    (0x472, 0x487, 0x417),
    (0x473, 0x488, 0x41d),
    (0x474, 0x489, 0x445),
    (0x475, 0x48a, 0x457),
    (0x476, 0x48b, 0x460),
    (0x477, 0x48c, 0x471),
    (0x478, 0x48d, 0x468),
    (0x479, 0x48e, 0x471),
    (0x47a, 0x48f, 0x471),
    (0x47b, 0x490, 0x471),
    (0x47c, 0x491, 0x471),
    (0x47d, 0x492, 0x471),
    (0x47e, 0x493, 0x471),
    (0x47f, 0x494, 0x471),
    (0x480, 0x495, 0x471),
    (0x481, 0x496, 0x471),
    (0x482, 0x497, 0x471),
    (0x483, 0x498, 0x471),
    (0x484, 0x499, 0x471),
    (0x485, 0x49a, 0x471),
    (0x486, 0x49b, 0x471),
];

pub(super) struct Layout {
    pub reset: u16,
    pub init: u16,
    pub header: u16,
}

fn word(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

pub(super) fn inspect(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<Layout>, ScanStop> {
    budget.charge()?;
    if bytes.len() != 0xa010
        || bytes[..16] != [b'N', b'E', b'S', 0x1a, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    {
        return Ok(None);
    }
    let Some(reset) = word(bytes, 0x800c) else {
        return Ok(None);
    };
    let Some(wrapper_end) = reset.checked_add(0x56) else {
        return Ok(None);
    };
    if !(0x8000..=BOOTSTRAP - 0x56).contains(&reset)
        || word(bytes, 0x800a) != reset.checked_add(0x51)
        || word(bytes, 0x800e) != reset.checked_add(0x55)
        || bytes[0x7810..0x7910].iter().any(|&value| value != 0xea)
    {
        return Ok(None);
    }
    let wrapper_offset = usize::from(reset - 0x8000) + 16;
    let Some(wrapper_bytes) =
        bytes.get(wrapper_offset..wrapper_offset + usize::from(wrapper_end - reset))
    else {
        return Ok(None);
    };
    let mut wrapper = wrapper_bytes.to_vec();
    let (Some(init), Some(play), Some(nmi), Some(update)) = (
        word(&wrapper, 0x42),
        word(&wrapper, 0x47),
        word(&wrapper, 0x4f),
        word(&wrapper, 0x52),
    ) else {
        return Ok(None);
    };
    let header = u16::from_le_bytes([wrapper[0x3e], wrapper[0x40]]);
    let Some(engine_end) = init.checked_add(ENGINE_LEN) else {
        return Ok(None);
    };
    if init < wrapper_end
        || engine_end > BOOTSTRAP
        || header < engine_end
        || header > BOOTSTRAP - 33
        || wrapper[0x45] > 1
        || play != init + 0xb0
        || nmi != reset + 0x4e
        || update != init + 0x14f
    {
        return Ok(None);
    }
    for offset in [0x3e, 0x40, 0x45] {
        wrapper[offset] = 0;
    }
    for offset in [0x42, 0x47, 0x4f, 0x52] {
        wrapper[offset..offset + 2].fill(0);
    }
    budget.charge()?;
    if zeff_firmware::sha256_hex(&wrapper) != WRAPPER_SHA {
        return Ok(None);
    }
    let engine_offset = usize::from(init - 0x8000) + 16;
    let Some(engine_bytes) = bytes.get(engine_offset..engine_offset + usize::from(ENGINE_LEN))
    else {
        return Ok(None);
    };
    let mut engine = engine_bytes.to_vec();
    for &(operand, target) in WORDS {
        budget.charge()?;
        if word(&engine, operand) != init.checked_add(target) {
            return Ok(None);
        }
        engine[operand..operand + 2].copy_from_slice(&target.to_le_bytes());
    }
    for &(low, high, target) in SPLITS {
        budget.charge()?;
        let Some(expected) = init.checked_add(target) else {
            return Ok(None);
        };
        if u16::from_le_bytes([engine[low], engine[high]]) != expected {
            return Ok(None);
        }
        engine[low] = target as u8;
        engine[high] = (target >> 8) as u8;
    }
    for _ in engine.chunks(256) {
        budget.charge()?;
    }
    Ok(
        (zeff_firmware::sha256_hex(&engine) == ENGINE_SHA).then_some(Layout {
            reset,
            init,
            header,
        }),
    )
}
