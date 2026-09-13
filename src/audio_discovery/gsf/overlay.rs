use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde_json::{Value, json};

const ROM_BASE: u32 = 0x0800_0000;

fn patches(original: &[u8], prepared: &[u8], cancel: &AtomicBool) -> Result<Vec<(usize, usize)>> {
    ensure!(
        !original.is_empty()
            && prepared.len() >= original.len()
            && prepared.len() <= crate::audio_discovery::MAX_ROM_BYTES,
        "invalid native GSF image size"
    );
    let mut ranges = Vec::new();
    let mut position = 0;
    while position < prepared.len() {
        ensure!(!cancel.load(Ordering::Relaxed), "GSF export cancelled");
        if position < original.len() && original[position] == prepared[position] {
            position += 1;
            continue;
        }
        let start = position;
        let mut last_change = position;
        position += 1;
        while position < prepared.len() {
            if position.is_multiple_of(65536) {
                ensure!(!cancel.load(Ordering::Relaxed), "GSF export cancelled");
            }
            if position >= original.len() || original[position] != prepared[position] {
                last_change = position;
            } else if position - last_change > 32 {
                break;
            }
            position += 1;
        }
        ranges.push((start, last_change + 1));
        ensure!(
            ranges.len() <= super::MAX_NATIVE_PATCHES,
            "too many native GSF patches"
        );
    }
    ensure!(
        !ranges.is_empty(),
        "native GSF contains no playback patches"
    );
    Ok(ranges)
}

pub(super) fn pack(
    original: &[u8],
    prepared: &[u8],
    mini_name: &str,
    tags: &[(&str, String)],
    mut metadata: Value,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    let ranges = patches(original, prepared, cancel)?;
    let base = super::codec::encode(ROM_BASE, ROM_BASE, original, &[], cancel)?;
    let hash = zeff_firmware::sha256_hex(&base);
    let base_name = format!("source-{hash}.gsflib");
    let mut bundle = crate::audio_discovery::bundle::Bundle::new();
    bundle.add(&base_name, &base)?;
    let mut libraries =
        vec![json!({"path": base_name, "sha256": hash, "offset": 0, "byte_len": original.len()})];
    let mut keys = Vec::new();
    let mut names = Vec::new();
    for (index, &(start, end)) in ranges.iter().enumerate().skip(1) {
        let data = super::codec::encode(
            ROM_BASE,
            ROM_BASE + start as u32,
            &prepared[start..end],
            &[],
            cancel,
        )?;
        let hash = zeff_firmware::sha256_hex(&data);
        let name = format!("patch-{hash}.gsflib");
        bundle.add(&name, &data)?;
        libraries
            .push(json!({"path": name, "sha256": hash, "offset": start, "byte_len": end-start}));
        keys.push(format!("_lib{}", index + 1));
        names.push(name);
    }
    metadata["libraries"] = json!(libraries);
    metadata["patches"] = json!(
        ranges
            .iter()
            .map(|&(start, end)| json!({"offset":start,"byte_len":end-start}))
            .collect::<Vec<_>>()
    );
    metadata["minigsf"] = json!(mini_name);
    let mut tags = tags.to_vec();
    tags.push(("_lib", base_name));
    for (key, name) in keys.iter().zip(names) {
        tags.push((key, name));
    }
    tags.push(("comment", super::metadata_text(&metadata)?));
    let (start, end) = ranges[0];
    let mini = super::codec::encode(
        ROM_BASE,
        ROM_BASE + start as u32,
        &prepared[start..end],
        &tags,
        cancel,
    )?;
    bundle.add(mini_name, &mini)?;
    bundle.add("manifest.json", super::metadata_text(&metadata)?.as_bytes())?;
    bundle.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_patches_reconstruct_without_copying_the_gap() -> Result<()> {
        let original = vec![0xA5; 1024 * 1024];
        let mut prepared = original.clone();
        prepared[..4].copy_from_slice(&[1, 2, 3, 4]);
        prepared[5000] = 9;
        prepared.extend_from_slice(&[7; 128]);
        let ranges = patches(&original, &prepared, &AtomicBool::new(false))?;
        assert_eq!(
            ranges,
            [(0, 4), (5000, 5001), (original.len(), prepared.len())]
        );
        let mut restored = original.clone();
        restored.resize(prepared.len(), 0);
        for (start, end) in ranges {
            restored[start..end].copy_from_slice(&prepared[start..end]);
        }
        assert_eq!(restored, prepared);
        assert!(patches(&original, &prepared, &AtomicBool::new(true)).is_err());
        assert!(patches(&original, &original, &AtomicBool::new(false)).is_err());
        Ok(())
    }
}
