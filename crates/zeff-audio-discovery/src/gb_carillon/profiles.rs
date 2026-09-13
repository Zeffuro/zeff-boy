use crate::{Budget, ScanStop};

const CODE_HASH: &str = "a9a5262449482a854f3951788c48dec01ec81b666f8cbbb5c24c3e00ca40601f";
pub(super) const COMMANDS: [u16; 16] = [
    0x4559, 0x454a, 0x4551, 0x44fc, 0x4509, 0x4514, 0x4523, 0x4535, 0x4542, 0x4559, 0x4559, 0x4559,
    0x4559, 0x4559, 0x4559, 0x4559,
];

pub(super) fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Vec<u16>, ScanStop> {
    let mut result = Vec::new();
    if !super::supports_cartridge(bytes) {
        return Ok(result);
    }
    for (bank, data) in bytes.as_chunks::<0x4000>().0.iter().enumerate().skip(1) {
        budget.charge()?;
        if data[0..15]
            != [
                0xc3, 0x56, 0x40, 0xc3, 0x88, 0x40, 0xc3, 0xe0, 0x40, 0xc3, 0, 0x41, 0xc3, 0xe8,
                0x40,
            ]
        {
            continue;
        }
        budget.charge()?;
        let code_matches = zeff_firmware::sha256_hex(&data[..0x600]) == CODE_HASH;
        #[cfg(any(test, feature = "test-support"))]
        let code_matches = code_matches || super::tests::recognized(data);
        if code_matches
            && COMMANDS
                .iter()
                .enumerate()
                .all(|(i, target)| data[0x6e0 + i * 2..0x6e2 + i * 2] == target.to_le_bytes())
        {
            result.push(bank as u16);
        }
    }
    Ok(result)
}
