pub fn xm_fixture() -> Vec<u8> {
    let mut bytes = vec![0; 336];
    bytes[..17].copy_from_slice(b"Extended Module: ");
    bytes[17..24].copy_from_slice(b"Fixture");
    bytes[37] = 0x1a;
    bytes[38..44].copy_from_slice(b"TRITON");
    bytes[58..60].copy_from_slice(&0x0104u16.to_le_bytes());
    bytes[60..64].copy_from_slice(&276u32.to_le_bytes());
    for (offset, value) in [
        (64, 1u16),
        (68, 2),
        (70, 1),
        (72, 1),
        (74, 1),
        (76, 6),
        (78, 125),
    ] {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[9, 0, 0, 0, 0, 2, 0, 8, 0]);
    bytes.extend_from_slice(&[0x87, 49, 1, 0x50, 0x80, 0x81, 97, 0x80]);
    let instrument = bytes.len();
    bytes.resize(instrument + 263 + 40, 0);
    bytes[instrument..instrument + 4].copy_from_slice(&263u32.to_le_bytes());
    bytes[instrument + 4..instrument + 8].copy_from_slice(b"Tone");
    bytes[instrument + 27..instrument + 29].copy_from_slice(&1u16.to_le_bytes());
    bytes[instrument + 29..instrument + 33].copy_from_slice(&40u32.to_le_bytes());
    let sample = instrument + 263;
    bytes[sample..sample + 4].copy_from_slice(&6u32.to_le_bytes());
    bytes[sample + 12] = 64;
    bytes[sample + 15] = 128;
    bytes.extend_from_slice(&[0, 127, 1, 127, 1, 128]);
    bytes
}

pub fn mod_fixture() -> Vec<u8> {
    let mut bytes = vec![0; 1084 + 64 * 4 * 4 + 8];
    bytes[..7].copy_from_slice(b"Fixture");
    bytes[42..44].copy_from_slice(&4u16.to_be_bytes());
    bytes[45] = 64;
    bytes[48..50].copy_from_slice(&1u16.to_be_bytes());
    bytes[950] = 1;
    bytes[951] = 0x7f;
    bytes[1080..1084].copy_from_slice(b"M.K.");
    bytes[1084..1088].copy_from_slice(&[0x01, 0xac, 0x10, 0]);
    let end = bytes.len();
    bytes[end - 8..].copy_from_slice(&[0, 127, 0, 128, 0, 127, 0, 128]);
    bytes
}

pub fn s3m_fixture() -> Vec<u8> {
    let mut bytes = vec![0; 400];
    bytes[..7].copy_from_slice(b"Fixture");
    bytes[28] = 0x1a;
    bytes[29] = 0x10;
    for (offset, value) in [(32, 1u16), (34, 1), (36, 1), (42, 1)] {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    bytes[44..48].copy_from_slice(b"SCRM");
    bytes[48] = 64;
    bytes[49] = 6;
    bytes[50] = 125;
    bytes[64..96].fill(0xff);
    bytes[64] = 0;
    bytes[96] = 0;
    bytes[97..99].copy_from_slice(&8u16.to_le_bytes());
    bytes[99..101].copy_from_slice(&16u16.to_le_bytes());
    let instrument = 8 * 16;
    bytes[instrument] = 1;
    bytes[instrument + 14..instrument + 16].copy_from_slice(&24u16.to_le_bytes());
    bytes[instrument + 16..instrument + 20].copy_from_slice(&4u32.to_le_bytes());
    bytes[instrument + 28] = 64;
    bytes[instrument + 31] = 6;
    bytes[instrument + 32..instrument + 34].copy_from_slice(&8363u16.to_le_bytes());
    bytes[instrument + 76..instrument + 80].copy_from_slice(b"SCRS");
    let pattern = 16 * 16;
    let mut packed = vec![0x20, 0x40, 1, 0];
    packed.resize(67, 0);
    bytes[pattern..pattern + 2].copy_from_slice(&((packed.len() + 2) as u16).to_le_bytes());
    bytes[pattern + 2..pattern + 2 + packed.len()].copy_from_slice(&packed);
    bytes[24 * 16..].copy_from_slice(&[
        0, 0, 0xff, 0x7f, 0, 0, 0x80, 0xff, 0, 0, 0x40, 0, 0, 0, 0xc0, 0xff,
    ]);
    bytes
}

pub fn it_fixture() -> Vec<u8> {
    let mut bytes = vec![0; 1316];
    bytes[..4].copy_from_slice(b"IMPM");
    bytes[4..11].copy_from_slice(b"Fixture");
    for (offset, value) in [
        (32, 1u16),
        (34, 1),
        (36, 1),
        (38, 1),
        (40, 0x0214),
        (42, 0x0214),
    ] {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    bytes[44..46].copy_from_slice(&4u16.to_le_bytes());
    bytes[48] = 128;
    bytes[50] = 6;
    bytes[51] = 125;
    bytes[64..128].fill(32);
    bytes[128..192].fill(64);
    bytes[192] = 0;
    bytes[193..197].copy_from_slice(&512u32.to_le_bytes());
    bytes[197..201].copy_from_slice(&1200u32.to_le_bytes());
    bytes[201..205].copy_from_slice(&1088u32.to_le_bytes());
    bytes[512..516].copy_from_slice(b"IMPI");
    bytes[512 + 24] = 128;
    bytes[512 + 25] = 32;
    bytes[512 + 64 + 48 * 2] = 48;
    bytes[512 + 64 + 48 * 2 + 1] = 1;
    bytes[1088..1090].copy_from_slice(&5u16.to_le_bytes());
    bytes[1090..1092].copy_from_slice(&1u16.to_le_bytes());
    bytes[1096..1101].copy_from_slice(&[0x81, 3, 48, 1, 0]);
    bytes[1200..1204].copy_from_slice(b"IMPS");
    bytes[1217] = 64;
    bytes[1218] = 7;
    bytes[1219] = 64;
    bytes[1246] = 1;
    bytes[1247] = 32;
    bytes[1248..1252].copy_from_slice(&4u32.to_le_bytes());
    bytes[1260..1264].copy_from_slice(&8363u32.to_le_bytes());
    bytes[1272..1276].copy_from_slice(&1300u32.to_le_bytes());
    bytes[1300..].copy_from_slice(&[
        0, 0, 0xff, 0x7f, 0, 0, 0x80, 0xff, 0, 0, 0x40, 0, 0, 0, 0xc0, 0xff,
    ]);
    bytes
}
