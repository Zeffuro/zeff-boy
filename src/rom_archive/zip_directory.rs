use anyhow::{Context, Result, ensure};

pub(crate) fn preflight_bounded_zip_directory(bytes: &[u8]) -> Result<()> {
    let mut zip64_counts = std::collections::BTreeMap::new();
    for (offset, signature) in bytes.windows(4).enumerate() {
        if signature != b"PK\x06\x06" {
            continue;
        }
        let Ok(size) = u64_at(bytes, offset + 4) else {
            continue;
        };
        let Some(end) = usize::try_from(size)
            .ok()
            .filter(|&size| size >= 40)
            .and_then(|size| offset.checked_add(12)?.checked_add(size))
        else {
            continue;
        };
        if bytes.get(end..end.saturating_add(4)) != Some(b"PK\x06\x07") {
            continue;
        }
        let bounded = u64_at(bytes, offset + 24)? <= 4096 && u64_at(bytes, offset + 32)? <= 4096;
        zip64_counts
            .entry(end)
            .and_modify(|value| *value &= bounded)
            .or_insert(bounded);
        ensure!(
            zip64_counts.len() <= 4096,
            "ZIP contains too many end records"
        );
    }
    let mut footers = 0;
    // The ZIP reader can retry earlier end records after a malformed directory.
    for footer in bytes
        .windows(4)
        .enumerate()
        .filter_map(|(offset, signature)| (signature == b"PK\x05\x06").then_some(offset))
    {
        let Some(record) = bytes.get(footer..footer + 22) else {
            continue;
        };
        let comment_len = usize::from(u16::from_le_bytes([record[20], record[21]]));
        if footer + 22 + comment_len > bytes.len() {
            continue;
        }
        footers += 1;
        ensure!(footers <= 4096, "ZIP contains too many end records");
        preflight_footer(bytes, footer, &zip64_counts)?;
    }
    ensure!(footers != 0, "ZIP end record is missing");
    Ok(())
}

fn preflight_footer(
    bytes: &[u8],
    footer: usize,
    zip64_counts: &std::collections::BTreeMap<usize, bool>,
) -> Result<()> {
    let count = u16::from_le_bytes(bytes[footer + 10..footer + 12].try_into()?);
    let may_be_zip64 = count == u16::MAX
        || bytes[footer + 12..footer + 16] == [0xff; 4]
        || bytes[footer + 16..footer + 20] == [0xff; 4];
    let locator = footer
        .checked_sub(20)
        .filter(|&offset| may_be_zip64 && bytes.get(offset..offset + 4) == Some(b"PK\x06\x07"));
    if let Some(locator) = locator {
        let bounded = zip64_counts
            .get(&locator)
            .context("ZIP64 end record is missing")?;
        ensure!(*bounded, "ZIP contains too many entries");
    } else {
        ensure!(count <= 4096, "ZIP contains too many entries");
        let count_on_disk = u16::from_le_bytes(bytes[footer + 8..footer + 10].try_into()?);
        ensure!(count_on_disk <= 4096, "ZIP contains too many entries");
    }
    Ok(())
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(
        bytes
            .get(offset..offset + 8)
            .context("truncated ZIP64 record")?
            .try_into()?,
    ))
}

pub(crate) fn validate_bounded_zip_directory(
    bytes: &[u8],
    directory_start: u64,
    unique_entries: usize,
) -> Result<()> {
    let mut offset = usize::try_from(directory_start)?;
    let mut count = 0;
    while bytes.get(offset..offset + 4) == Some(b"PK\x01\x02") {
        count += 1;
        ensure!(count <= 4096, "ZIP contains too many entries");
        let header = bytes
            .get(offset..offset + 46)
            .context("truncated ZIP directory entry")?;
        let name_len = usize::from(u16::from_le_bytes([header[28], header[29]]));
        let extra_len = usize::from(u16::from_le_bytes([header[30], header[31]]));
        let comment_len = usize::from(u16::from_le_bytes([header[32], header[33]]));
        ensure!(name_len <= 4096, "ZIP member name is too long");
        offset = offset
            .checked_add(46 + name_len + extra_len + comment_len)
            .context("invalid ZIP directory entry size")?;
        ensure!(offset <= bytes.len(), "truncated ZIP directory");
    }
    // ZipArchive collapses duplicate raw names before exposing its entry list.
    ensure!(
        count == unique_entries,
        "ZIP contains duplicate member names"
    );
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
