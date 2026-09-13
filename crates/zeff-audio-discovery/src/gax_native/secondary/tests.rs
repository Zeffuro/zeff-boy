use super::*;
use crate::gax_native::{GaxNativeEntry, GaxNativeProfile, RomSpan};

fn halves(bytes: &mut [u8], at: usize, values: &[u16]) {
    for (index, value) in values.iter().enumerate() {
        bytes[at + index * 2..at + index * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
}

fn put_word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn load(bytes: &mut [u8], at: usize, register: u16, slot: usize, value: u32) {
    let relative = slot - ((at + 4) & !3);
    assert!(relative.is_multiple_of(4) && relative <= 1020);
    halves(bytes, at, &[0x4800 | register << 8 | (relative / 4) as u16]);
    put_word(bytes, slot, value);
}

fn fixture() -> (Vec<u8>, GaxNativeSong) {
    let mut bytes = vec![0; 0x4000];
    bytes[0x400..0x414].copy_from_slice(INIT_SIGNATURES[2].bytes);
    halves(
        &mut bytes,
        0x43e,
        &[
            0x8938, 0x490d, 0x4288, 0xd104, 0x6b38, 0x6880, 0x6980, 0x8b00, 0x8138, 0x8978, 0x4288,
            0xd117, 0x6b38, 0x6880, 0x6981, 0x8b08, 0x2801, 0xd910, 0x7ec9, 0x0148, 0x1a40, 0x0080,
            0x1840, 0x00c0, 0xe009,
        ],
    );
    halves(&mut bytes, 0x47c, &[0x1c08, 0xe01d, 0x1c08, 0xe02a, 0x8178]);
    halves(
        &mut bytes,
        0x4a2,
        &[
            0x893b, 0x2100, 0x4a18, 0x4690, 0x897c, 0x6810, 0x4298, 0xd2e4, 0x3208, 0x3101, 0x290c,
            0xd9f8, 0x200c, 0x00c0, 0x4440, 0x6800, 0x8138, 0x1c23, 0x2100, 0x4642, 0x6810, 0x4298,
            0xd2d7, 0x3208, 0x3101, 0x290c, 0xd9f8, 0x200c, 0x00c0, 0x4440, 0x6800, 0x8178,
        ],
    );
    halves(
        &mut bytes,
        0x5ec,
        &[
            0x68b0, 0x6980, 0x69c0, 0x2800, 0xd03e, 0x8a38, 0x2802, 0xd83b, 0x314c, 0x2001, 0x7008,
        ],
    );
    halves(
        &mut bytes,
        0x67e,
        &[
            0x8978, 0x893c, 0x42a0, 0xd108, 0x4e09, 0x6830, 0x304c, 0x7800, 0x2800, 0xd00e, 0x8a38,
            0x2800, 0xd00b, 0x4804, 0x6801, 0x9803, 0x6188, 0x3008, 0x9003, 0x9804, 0x3808, 0x9004,
        ],
    );
    halves(&mut bytes, 0x6b0, &[0x4a48, 0x6811, 0x2000, 0x6188]);
    load(&mut bytes, 0x440, 1, 0x590, 65535);
    load(&mut bytes, 0x422, 0, 0x594, 0x03000000);
    load(&mut bytes, 0x4a6, 2, 0x598, 0x08003000);
    for (at, register) in [(0x686, 6), (0x698, 0), (0x6b0, 2)] {
        load(&mut bytes, at, register, 0x710, 0x03000000);
    }
    load(&mut bytes, 0x2004, 0, 0x2140, 0x03000000);
    halves(
        &mut bytes,
        0x215a,
        &[
            0x6918, 0x0080, 0x1c19, 0x3108, 0x1808, 0x6800, 0x6800, 0x6182, 0x1c18, 0x304c, 0x7800,
            0x2800, 0xd00d,
        ],
    );
    let relative = 0x2400u32 - 0x218a - 4;
    halves(
        &mut bytes,
        0x218a,
        &[
            0xf000 | ((relative >> 12) & 0x7ff) as u16,
            0xf800 | ((relative >> 1) & 0x7ff) as u16,
        ],
    );
    halves(
        &mut bytes,
        0x2400,
        &[0xb530, 0x1c04, 0x1c0d, 0x68e2, 0x69a0, 0x2801, 0xd103],
    );
    halves(&mut bytes, 0x2424, &[0x2000, 0x0600, 0x2800, 0xd10a]);
    for (index, rate) in [
        5735, 9079, 10513, 11469, 13380, 15769, 18158, 21025, 26760, 31537, 36316, 40138, 42049,
    ]
    .into_iter()
    .enumerate()
    {
        put_word(&mut bytes, 0x3000 + index * 8, rate);
    }
    put_word(&mut bytes, 0x3208, 0x08003240);
    put_word(&mut bytes, 0x3258, 0x08003280);
    halves(&mut bytes, 0x3298, &[21025]);
    bytes[0x329b] = 21;
    put_word(&mut bytes, 0x329c, 0x08003300);
    let entry = |at| GaxNativeEntry {
        source: RomSpan::new(at, 20),
        cpu_address: 0x08000001 + at as u32,
    };
    let song = GaxNativeSong {
        header: RomSpan::new(0x3200, 32),
        index: 0,
        title: "synthetic".into(),
        channels: 4,
        native: GaxNativeProfile {
            version: "GAX 2".into(),
            layout: GaxNativeLayout::V2Current,
            new: Some(entry(0x100)),
            init: entry(0x400),
            mix: entry(0x1f00),
            play: entry(0x2000),
            work_ram: 0x03000004,
            sample_rate: u16::MAX,
            ram_copies: Vec::new(),
        },
        mapped_spans: vec![RomSpan::new(0x3200, 32)],
        warnings: Vec::new(),
    };
    (bytes, song)
}

#[test]
fn equal_optimized_rates_select_a_witnessed_lower_rate() {
    let (bytes, song) = fixture();
    assert_eq!(lower_rate(&bytes, &song).unwrap(), Some(5735));
    let mut different = bytes.clone();
    different[0x329b] = 9;
    assert_eq!(lower_rate(&different, &song).unwrap(), None);
    let mut minimum = bytes.clone();
    halves(&mut minimum, 0x3298, &[5735]);
    minimum[0x329b] = 5;
    assert!(lower_rate(&minimum, &song).is_err());
    let mut unoptimized = bytes;
    put_word(&mut unoptimized, 0x329c, 0);
    assert_eq!(lower_rate(&unoptimized, &song).unwrap(), None);
}

#[test]
fn changed_code_state_literals_and_unbounded_data_do_not_override_rates() {
    let (bytes, song) = fixture();
    for at in [
        0x400, 0x43e, 0x47c, 0x4a2, 0x5ec, 0x67e, 0x6b2, 0x710, 0x215a, 0x218a, 0x2400, 0x2424,
        0x3000, 0x3208, 0x3258, 0x329c,
    ] {
        let mut changed = bytes.clone();
        put_word(&mut changed, at, 0);
        assert_eq!(
            lower_rate(&changed, &song).unwrap(),
            None,
            "accepted {at:x}"
        );
    }
    let mut changed = bytes.clone();
    put_word(&mut changed, 0x598, 0x08003ffc);
    assert_eq!(lower_rate(&changed, &song).unwrap(), None);
    let mut changed = bytes;
    put_word(&mut changed, 0x3008, 5735);
    assert_eq!(lower_rate(&changed, &song).unwrap(), None);
}

#[test]
fn ordinary_bootstrap_bytes_stay_exact_and_override_preserves_source() {
    let (mut bytes, song) = fixture();
    let changed = super::super::build(&bytes, &song).unwrap();
    put_word(&mut bytes, 0x329c, 0);
    let absent = super::super::build(&bytes, &song).unwrap();
    put_word(&mut bytes, 0x329c, 0x08003300);
    bytes[0x329b] = 9;
    let unequal = super::super::build(&bytes, &song).unwrap();
    assert_eq!(&absent[bytes.len()..], &unequal[bytes.len()..]);
    assert_eq!(changed.len(), unequal.len() + 12);
    assert_eq!(&unequal[4..bytes.len()], &bytes[4..]);
    assert!(
        changed[bytes.len()..]
            .windows(4)
            .any(|word| word == 0xe1c400bau32.to_le_bytes())
    );
}
