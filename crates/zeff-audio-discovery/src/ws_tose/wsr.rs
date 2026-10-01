use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};

use super::{WsToseHardware, WsToseSong};

const SOURCE_SIZE: usize = 0x10_0000;
const BOOTSTRAP_OFFSET: usize = 0x0f_e000;
const TRAILER_SIZE: usize = 32;
pub(super) const QUALIFIED_SOURCE_SHA: &str =
    "33d1e3380de5edac1af72dc4cb7d6cabcebc40eec78c6dd656b452f9889672ce";
#[cfg(test)]
const BOOTSTRAP_SHA: &str = "dbf56c6ee4e782a563e10242d7a2b04b975a204fbd3ee94d9df85b7c246b6878";
const BOOTSTRAP_STAGE: [u8; 47] = [
    0xfa, 0xfc, 0x89, 0xc2, 0x31, 0xc0, 0x8e, 0xd0, 0xbc, 0xf0, 0x3b, 0x8e, 0xd8, 0x8e, 0xc0, 0x31,
    0xff, 0xb9, 0, 0x20, 0xf3, 0xab, 0xe6, 0xb2, 0x52, 0x0e, 0x1f, 0xbe, 0x80, 0xe0, 0xbf, 0, 0x3c,
    0xb9, 0x50, 0, 0xf3, 0xa4, 0x31, 0xc0, 0x8e, 0xd8, 0xea, 0, 0x3c, 0, 0,
];
const BOOTSTRAP_RAM_CODE: [u8; 80] = [
    0xb0, 0, 0xe6, 0xc0, 0xb0, 0x0f, 0xe6, 0xc3, 0xe6, 0xc2, 0x9a, 0x22, 0, 0, 0xf0, 0x58, 0x85,
    0xc0, 0x75, 0x22, 0xb8, 0x20, 0, 0x9a, 0x96, 0, 0, 0xf0, 0xc7, 0x06, 0x38, 0, 0x3a, 0x3c, 0xc7,
    0x06, 0x3a, 0, 0, 0, 0xb0, 8, 0xe6, 0xb0, 0xb0, 0x40, 0xe6, 0xb6, 0xe6, 0xb2, 0xfb, 0xf4, 0xeb,
    0xfd, 0xfa, 0xf4, 0xeb, 0xfc, 0x60, 0x1e, 0x06, 0x31, 0xc0, 0x8e, 0xd8, 0x8e, 0xc0, 0xb0, 0x40,
    0xe6, 0xb6, 0x9a, 0xeb, 1, 0, 0xf0, 0x07, 0x1f, 0x61, 0xcf,
];

fn bootstrap() -> [u8; 512] {
    let mut bootstrap = [0xff; 512];
    bootstrap[..BOOTSTRAP_STAGE.len()].copy_from_slice(&BOOTSTRAP_STAGE);
    bootstrap[BOOTSTRAP_STAGE.len()..128].fill(0x90);
    bootstrap[128..128 + BOOTSTRAP_RAM_CODE.len()].copy_from_slice(&BOOTSTRAP_RAM_CODE);
    bootstrap
}

pub fn supported(song: &WsToseSong) -> bool {
    song.wsr_exportable
        && song.profile == "ws-tose-fixed-v5"
        && song.hardware == WsToseHardware::Mono
        && song.index == 32
}

pub fn encode(bytes: &[u8], song: &WsToseSong, cancel: &AtomicBool) -> Result<Vec<u8>> {
    check_cancel(cancel)?;
    ensure!(
        supported(song),
        "WonderSwan TOSE selection is not WSR-qualified"
    );
    ensure!(
        bytes.len() == SOURCE_SIZE,
        "WSR-qualified source has an unexpected size"
    );
    ensure!(
        zeff_firmware::sha256_hex(bytes) == QUALIFIED_SOURCE_SHA,
        "WSR-qualified source hash differs"
    );
    super::validate_song(bytes, song, cancel)?;
    let output = emit(bytes)?;
    check_cancel(cancel)?;
    Ok(output)
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "WonderSwan WSR export cancelled"
    );
    Ok(())
}

fn emit(bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        bytes.len() == SOURCE_SIZE,
        "WSR source must be exactly 1 MiB"
    );
    let mut output = bytes.to_vec();
    let bootstrap = bootstrap();
    output[BOOTSTRAP_OFFSET..BOOTSTRAP_OFFSET + bootstrap.len()].copy_from_slice(&bootstrap);
    let trailer = SOURCE_SIZE - TRAILER_SIZE;
    output[trailer..trailer + 16].copy_from_slice(b"WSRF\0\0\0\0\0\0\0\0\0\0\0\0");
    output[trailer + 16..trailer + 22].copy_from_slice(&[0xea, 0, 0xe0, 0, 0xf0, 0x90]);
    output[trailer + 23] = 0;
    let checksum = output[..SOURCE_SIZE - 2]
        .iter()
        .fold(0_u16, |sum, &byte| sum.wrapping_add(u16::from(byte)));
    output[SOURCE_SIZE - 2..].copy_from_slice(&checksum.to_le_bytes());
    Ok(output)
}

#[cfg(any(test, feature = "test-support"))]
fn synthetic_source() -> Vec<u8> {
    let mut source = vec![0xff; SOURCE_SIZE];
    source[0x0f_0000..0x0f_01f0].fill(0x90);
    source[0x0f_0022..0x0f_002f].copy_from_slice(&[
        0xc7, 0x06, 0, 1, 0x34, 0x12, 0xba, 0xad, 0xde, 0xb8, 0xcd, 0xab, 0xcb,
    ]);
    source[0x0f_0096..0x0f_00c0].copy_from_slice(&[
        0xa3, 2, 1, 0xbf, 0xc0, 0, 0xb9, 8, 0, 0x31, 0xc0, 0xf3, 0xaa, 0xb9, 8, 0, 0xb0, 0xff,
        0xf3, 0xaa, 0xb8, 0xc0, 7, 0xe7, 0x80, 0xb0, 0xff, 0xe6, 0x88, 0xb0, 3, 0xe6, 0x8f, 0xb0,
        1, 0xe6, 0x90, 0xb0, 6, 0xe6, 0x91, 0xcb,
    ]);
    source[0x0f_01eb..0x0f_01f0].copy_from_slice(&[0xff, 6, 4, 1, 0xcb]);
    source[SOURCE_SIZE - TRAILER_SIZE..].fill(0);
    source[SOURCE_SIZE - 6] = 3;
    source
}

#[cfg(feature = "test-support")]
pub fn synthetic_wsr() -> Vec<u8> {
    emit(&synthetic_source()).expect("synthetic WSR fixture fits the emitter")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use crate::RomSpan;

    use super::*;

    fn descriptor() -> WsToseSong {
        WsToseSong {
            profile: "ws-tose-fixed-v5",
            hardware: WsToseHardware::Mono,
            index: 32,
            title: "Audio selector 32".into(),
            table_entry: RomSpan {
                effective_offset: 0,
                byte_len: 6,
                canonical_cpu_address: 0,
            },
            tracks: Vec::new(),
            mapped_spans: Vec::new(),
            warnings: Vec::new(),
            wsr_exportable: true,
        }
    }

    #[test]
    fn synthetic_fixture_uses_the_qualified_emitter() {
        let source = synthetic_source();
        let output = emit(&source).unwrap();
        assert_eq!(
            zeff_firmware::sha256_hex(&output),
            "b5572772fff1f330e5f97cc70d8efb407e3b456d13bc0c4870fdc71d0ea86f52"
        );
        assert_eq!(zeff_firmware::sha256_hex(&bootstrap()), BOOTSTRAP_SHA);
        assert_eq!(&output[..BOOTSTRAP_OFFSET], &source[..BOOTSTRAP_OFFSET]);
        assert_eq!(
            &output[BOOTSTRAP_OFFSET + bootstrap().len()..SOURCE_SIZE - TRAILER_SIZE],
            &source[BOOTSTRAP_OFFSET + bootstrap().len()..SOURCE_SIZE - TRAILER_SIZE]
        );
        let trailer = SOURCE_SIZE - TRAILER_SIZE;
        assert_eq!(
            &output[trailer + 22..trailer + 23],
            &source[trailer + 22..trailer + 23]
        );
        assert_eq!(
            &output[trailer + 24..trailer + 30],
            &source[trailer + 24..trailer + 30]
        );
        assert_eq!(&output[trailer..trailer + 4], b"WSRF");
        assert_eq!(output[trailer + 23], 0);
    }

    #[test]
    fn emitter_requires_exact_source_bounds() {
        assert!(emit(&[]).is_err());
        assert!(emit(&vec![0; SOURCE_SIZE - 1]).is_err());
        assert!(emit(&vec![0; SOURCE_SIZE + 1]).is_err());
    }

    #[test]
    fn qualification_rejects_false_and_forged_descriptors() {
        let song = descriptor();
        assert!(supported(&song));
        let mut false_flag = song.clone();
        false_flag.wsr_exportable = false;
        assert!(!supported(&false_flag));
        let mut wrong_index = song.clone();
        wrong_index.index = 31;
        assert!(!supported(&wrong_index));
        let mut wrong_hardware = song.clone();
        wrong_hardware.hardware = WsToseHardware::Color;
        assert!(!supported(&wrong_hardware));
        let mut wrong_profile = song.clone();
        wrong_profile.profile = "ws-tose-fixed-v6";
        assert!(!supported(&wrong_profile));
    }

    #[test]
    fn encode_checks_cancellation_source_and_descriptor() {
        let source = synthetic_source();
        let song = descriptor();
        assert!(encode(&source, &song, &AtomicBool::new(false)).is_err());
        assert!(encode(&source, &song, &AtomicBool::new(true)).is_err());
        let mut stale = song.clone();
        stale.index = 31;
        assert!(encode(&source, &stale, &AtomicBool::new(false)).is_err());
    }
}
