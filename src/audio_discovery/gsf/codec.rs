use std::collections::BTreeSet;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use flate2::{Compression, write::ZlibEncoder};

const ROM_BASE: u32 = 0x0800_0000;
const ROM_LIMIT: u32 = 0x0A00_0000;
const MAX_TAG_BYTES: usize = 1024 * 1024;

pub(super) fn encode(
    entry_address: u32,
    load_address: u32,
    payload: &[u8],
    tags: &[(&str, String)],
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    ensure!(
        (ROM_BASE..ROM_LIMIT).contains(&(entry_address & !1))
            && (entry_address & 1 != 0 || entry_address.is_multiple_of(4)),
        "GSF entry point must be a valid cartridge code address"
    );
    ensure!(
        (ROM_BASE..ROM_LIMIT).contains(&load_address)
            && !payload.is_empty()
            && payload.len() <= (ROM_LIMIT - load_address) as usize,
        "GSF executable exceeds cartridge address space"
    );
    let tags = encode_tags(tags)?;
    check_cancel(cancel)?;
    let mut encoder = ZlibEncoder::new(vec![0; 16], Compression::default());
    for value in [entry_address, load_address, payload.len() as u32] {
        encoder.write_all(&value.to_le_bytes())?;
    }
    for block in payload.chunks(64 * 1024) {
        check_cancel(cancel)?;
        encoder.write_all(block)?;
    }
    let mut data = encoder.finish()?;
    check_cancel(cancel)?;
    let compressed_len = (data.len() - 16) as u32;
    let crc = crc32fast::hash(&data[16..]);
    data[..4].copy_from_slice(b"PSF\x22");
    data[8..12].copy_from_slice(&compressed_len.to_le_bytes());
    data[12..16].copy_from_slice(&crc.to_le_bytes());
    data.extend_from_slice(&tags);
    ensure!(
        data.len() <= super::super::MAX_ROM_BYTES + 2 * MAX_TAG_BYTES,
        "GSF file exceeds its size limit"
    );
    check_cancel(cancel)?;
    Ok(data)
}

fn encode_tags(tags: &[(&str, String)]) -> Result<Vec<u8>> {
    let mut output = b"[TAG]".to_vec();
    let mut keys = BTreeSet::new();
    for (key, value) in tags {
        ensure!(
            !key.is_empty()
                && key
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_')
                && keys.insert(key.to_ascii_lowercase())
                && !value.chars().any(char::is_control),
            "invalid or duplicate GSF tag"
        );
        ensure!(
            output.len() + key.len() + value.len() + 2 <= MAX_TAG_BYTES,
            "GSF tags exceed their size limit"
        );
        output.extend_from_slice(key.as_bytes());
        output.push(b'=');
        output.extend_from_slice(value.as_bytes());
        output.push(b'\n');
    }
    Ok(output)
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "GSF export cancelled");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn executable(bytes: &[u8]) -> Result<Vec<u8>> {
        assert_eq!(&bytes[..4], b"PSF\x22");
        assert_eq!(&bytes[4..8], &[0; 4]);
        let compressed_len = u32::from_le_bytes(bytes[8..12].try_into()?) as usize;
        let compressed = &bytes[16..16 + compressed_len];
        assert_eq!(
            u32::from_le_bytes(bytes[12..16].try_into()?),
            crc32fast::hash(compressed)
        );
        let mut decoded = Vec::new();
        flate2::read::ZlibDecoder::new(compressed).read_to_end(&mut decoded)?;
        assert_eq!(
            &bytes[16 + compressed_len..16 + compressed_len + 5],
            b"[TAG]"
        );
        Ok(decoded)
    }

    #[test]
    fn full_program_and_mini_overlay_reconstruct_the_same_cartridge() -> Result<()> {
        let cancel = AtomicBool::new(false);
        let mut rom = vec![0xA5; 0x1000];
        rom[0x800..0x804].copy_from_slice(&257u32.to_le_bytes());
        let full = executable(&encode(ROM_BASE, ROM_BASE, &rom, &[], &cancel)?)?;
        rom[0x800..0x804].fill(0);
        let library = executable(&encode(ROM_BASE, ROM_BASE, &rom, &[], &cancel)?)?;
        let mini = executable(&encode(
            ROM_BASE,
            ROM_BASE + 0x800,
            &257u32.to_le_bytes(),
            &[("_lib", "audio.gsflib".to_owned())],
            &cancel,
        )?)?;
        assert_eq!(u32::from_le_bytes(full[..4].try_into()?), ROM_BASE);
        assert_eq!(u32::from_le_bytes(full[4..8].try_into()?), ROM_BASE);
        assert_eq!(u32::from_le_bytes(full[8..12].try_into()?), 0x1000);
        assert_eq!(u32::from_le_bytes(mini[8..12].try_into()?), 4);
        let offset = (u32::from_le_bytes(mini[4..8].try_into()?) & 0x1FF_FFFF) as usize;
        let mut restored = library[12..].to_vec();
        restored[offset..offset + 4].copy_from_slice(&mini[12..]);
        assert_eq!(restored, full[12..]);
        Ok(())
    }

    #[test]
    fn bounds_cancellation_and_tag_injection_fail_closed() {
        let cancel = AtomicBool::new(false);
        for (entry, address, bytes) in [
            (0, ROM_BASE, &[0][..]),
            (ROM_BASE + 2, ROM_BASE, &[0][..]),
            (ROM_BASE, ROM_LIMIT, &[0][..]),
            (ROM_BASE, ROM_LIMIT - 1, &[0, 0][..]),
            (ROM_BASE, ROM_BASE, &[][..]),
        ] {
            assert!(encode(entry, address, bytes, &[], &cancel).is_err());
        }
        assert!(encode(ROM_BASE, ROM_BASE, &[0], &[], &AtomicBool::new(true)).is_err());
        for tags in [
            vec![("title", "Song\n_lib=other.gsflib".to_owned())],
            vec![("title=bad", "Song".to_owned())],
            vec![("title", "Song".to_owned()), ("TITLE", "Other".to_owned())],
            vec![("comment", "x".repeat(MAX_TAG_BYTES))],
        ] {
            assert!(encode(ROM_BASE, ROM_BASE, &[0], &tags, &cancel).is_err());
        }
    }
}
