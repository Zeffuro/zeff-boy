use super::*;
use std::io::{Cursor, Write as _};

fn archive() -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("game.nes", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(&[0; 4]).unwrap();
    writer.finish().unwrap().into_inner()
}

fn zip64(bytes: Vec<u8>) -> Vec<u8> {
    let footer = bytes.len() - 22;
    let mut end = bytes[footer..].to_vec();
    let mut record = [0u8; 56];
    record[..4].copy_from_slice(b"PK\x06\x06");
    record[4..12].copy_from_slice(&44u64.to_le_bytes());
    record[12..16].copy_from_slice(&[45, 0, 45, 0]);
    record[24..32].copy_from_slice(&1u64.to_le_bytes());
    record[32..40].copy_from_slice(&1u64.to_le_bytes());
    record[40..44].copy_from_slice(&end[12..16]);
    record[48..52].copy_from_slice(&end[16..20]);
    let mut locator = [0u8; 20];
    locator[..4].copy_from_slice(b"PK\x06\x07");
    locator[8..16].copy_from_slice(&(footer as u64).to_le_bytes());
    locator[16..20].copy_from_slice(&1u32.to_le_bytes());
    end[8..20].fill(0xff);
    [
        bytes[..footer].to_vec(),
        record.to_vec(),
        locator.to_vec(),
        end,
    ]
    .concat()
}

#[test]
fn bounded_directory_accepts_small_zip64_and_prepended_zip() {
    for bytes in [
        archive(),
        zip64(archive()),
        [b"prefix".to_vec(), archive()].concat(),
    ] {
        preflight_bounded_zip_directory(&bytes).unwrap();
        let archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        validate_bounded_zip_directory(&bytes, archive.central_directory_start(), archive.len())
            .unwrap();
    }
}

#[test]
fn metadata_entry_bombs_fail_before_zip_archive_allocation() {
    let mut bytes = archive();
    let footer = bytes.len() - 22;
    bytes[footer + 8..footer + 12].copy_from_slice(&[0x88, 0x13, 0x88, 0x13]);
    assert!(
        preflight_bounded_zip_directory(&bytes)
            .unwrap_err()
            .to_string()
            .contains("too many entries")
    );
    let mut bytes = zip64(archive());
    let record = bytes.len() - 22 - 20 - 56;
    bytes[record + 24..record + 40].fill(0xff);
    assert!(
        preflight_bounded_zip_directory(&bytes)
            .unwrap_err()
            .to_string()
            .contains("too many entries")
    );
}

#[test]
fn all_possible_footer_and_zip64_record_counts_are_bounded() {
    let mut earlier = archive();
    let footer = earlier.len() - 22;
    earlier[footer + 8..footer + 12].copy_from_slice(&[0x88, 0x13, 0x88, 0x13]);
    earlier.extend_from_slice(&archive());
    assert!(preflight_bounded_zip_directory(&earlier).is_err());

    let mut nested = zip64(archive());
    let record = nested.len() - 22 - 20 - 56;
    let nested_record = nested[record..record + 56].to_vec();
    nested[record + 4..record + 12].copy_from_slice(&100u64.to_le_bytes());
    nested[record + 24..record + 40].fill(0xff);
    nested.splice(record + 56..record + 56, nested_record);
    assert!(preflight_bounded_zip_directory(&nested).is_err());

    let mut misleading_locator = zip64(archive());
    let footer = misleading_locator.len() - 22;
    misleading_locator[footer + 8..footer + 12].copy_from_slice(&[0x88, 0x13, 0x88, 0x13]);
    misleading_locator[footer + 12..footer + 20].fill(0);
    assert!(preflight_bounded_zip_directory(&misleading_locator).is_err());
}
