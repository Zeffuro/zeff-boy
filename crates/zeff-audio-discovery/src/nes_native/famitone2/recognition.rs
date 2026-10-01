use crate::{Budget, ScanStop};

pub(super) const ENGINE_LEN: u16 = 0x40c;
pub(super) const BOOTSTRAP: u16 = 0xf800;
const ENGINE_SHA: &str = "7eb0d036534974093a267f48bb86ef1fb95d01e93218c9e494c393983c36e6d0";
const WRAPPER_SHA: &str = "57ebb534059bb597846f43c1a5fd4738eb8ea4e2bc54e35df40268c87cedce82";
pub(super) const WORDS: &[(usize, u16)] = &[
    (0x010, 0x055),
    (0x0b6, 0x055),
    (0x12d, 0x1d2),
    (0x148, 0x196),
    (0x154, 0x2db),
    (0x15e, 0x288),
    (0x166, 0x2db),
    (0x170, 0x288),
    (0x178, 0x2db),
    (0x182, 0x288),
    (0x187, 0x2db),
    (0x191, 0x288),
    (0x1c3, 0x1af),
    (0x1e1, 0x38c),
    (0x1ee, 0x3cc),
    (0x213, 0x38c),
    (0x220, 0x3cc),
    (0x245, 0x38c),
    (0x252, 0x3cc),
    (0x344, 0x2f1),
    (0x362, 0x2f1),
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
    let reset = word(bytes, 0x800c).unwrap();
    if !(0x8000..=BOOTSTRAP - 0x56).contains(&reset)
        || word(bytes, 0x800a) != Some(reset + 0x51)
        || word(bytes, 0x800e) != Some(reset + 0x55)
        || bytes[0x7810..0x7910].iter().any(|&value| value != 0xea)
    {
        return Ok(None);
    }
    let offset = usize::from(reset - 0x8000) + 16;
    let mut wrapper = bytes[offset..offset + 0x56].to_vec();
    let init = word(&wrapper, 0x42).unwrap();
    let header = u16::from_le_bytes([wrapper[0x3e], wrapper[0x40]]);
    if init < reset + 0x56
        || init > BOOTSTRAP - ENGINE_LEN
        || header < init + ENGINE_LEN
        || header > BOOTSTRAP - 33
        || wrapper[0x45] > 1
        || word(&wrapper, 0x47) != Some(init + 0x91)
        || word(&wrapper, 0x52) != Some(init + 0x11f)
        || word(&wrapper, 0x4f) != Some(reset + 0x4e)
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
    let offset = usize::from(init - 0x8000) + 16;
    let mut engine = bytes[offset..offset + usize::from(ENGINE_LEN)].to_vec();
    for &(operand, target) in WORDS {
        budget.charge()?;
        if word(&engine, operand) != Some(init + target) {
            return Ok(None);
        }
        engine[operand..operand + 2].copy_from_slice(&target.to_le_bytes());
    }
    for (operand, shift) in [(0x7a, 0), (0x7f, 8)] {
        if engine[operand] != ((init + 0x389) >> shift) as u8 {
            return Ok(None);
        }
        engine[operand] = (0x389_u16 >> shift) as u8;
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
