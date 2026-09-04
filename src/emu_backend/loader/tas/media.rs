use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, ensure};

pub(super) fn read_bounded_direct_rom(
    path: &Path,
    expected_len: usize,
    changed_error: &str,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(expected_len);
    std::fs::File::open(path)
        .with_context(|| format!("failed to open TAS source media {}", path.display()))?
        .take(expected_len as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read TAS source media {}", path.display()))?;
    ensure!(bytes.len() == expected_len, "{changed_error}");
    Ok(bytes)
}

pub(super) fn reject_embedded_zip_sram(
    source_path: &Path,
    max_archive_bytes: u64,
    max_member_bytes: u64,
    error: &str,
) -> Result<()> {
    let inspection = crate::rom_archive::inspect_bounded_zip_members(
        source_path,
        "sav",
        max_archive_bytes,
        max_member_bytes,
    )?;
    ensure!(inspection.entries.is_empty(), "{error}");
    Ok(())
}
