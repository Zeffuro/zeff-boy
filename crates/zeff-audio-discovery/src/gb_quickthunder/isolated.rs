use crate::{Budget, ScanStop};

pub(super) fn source_bank(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<u16>, ScanStop> {
    let (bank, hash) = match (bytes.len(), bytes.get(0x143), bytes.get(0x147..0x14a)) {
        (0x40000, Some(0xc0), Some([0x97, 3, 0])) => (
            15,
            "7e7a7848fee2640e47e476f03ab8ccf16442a0391b639f30bd7ec7698225bde2",
        ),
        (0x80000, Some(0x80), Some([0x99, 4, 2])) => (
            31,
            "33417ba532f65061fb01f30645b4d3752ee463c09631e0c0a116ba8409936d13",
        ),
        _ => return Ok(None),
    };
    for _ in bytes.chunks(4096) {
        budget.charge()?;
    }
    let digest = zeff_firmware::sha256_hex(bytes);
    if digest == hash {
        return Ok(Some(bank));
    }
    #[cfg(any(test, feature = "test-support"))]
    if digest == zeff_firmware::sha256_hex(&super::tests::synthetic_rom_rocket(bytes[0x147])) {
        return Ok(Some(bank));
    }
    Ok(None)
}

pub fn supports_prepared_cartridge(bytes: &[u8]) -> bool {
    super::supports_cartridge(bytes)
        || bytes.len() == 0x8000 && bytes[0x143] == 0xc0 && bytes[0x147..0x14a] == [0, 0, 0]
}

pub(super) fn project(bytes: &[u8], song: &super::GbQuickThunderSong) -> anyhow::Result<Vec<u8>> {
    let start = usize::from(song.bank) * 0x4000;
    let end = start + 0x4000;
    anyhow::ensure!(
        end <= bytes.len()
            && song.mapped_spans.iter().all(|span| {
                let offset = span.effective_offset as usize;
                offset >= start && offset + span.byte_len as usize <= end
            }),
        "isolated QuickThunder source escapes its qualified ROM bank"
    );
    let mut output = vec![0; 0x8000];
    output[0x143] = 0xc0;
    output[0x4000..].copy_from_slice(&bytes[start..end]);
    output[0x14d] = output[0x134..=0x14c]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_sub(byte).wrapping_sub(1));
    Ok(output)
}
