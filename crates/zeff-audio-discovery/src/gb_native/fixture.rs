use super::profiles::{CUES, Cue, Profile};

pub(super) const PROFILE: Profile = Profile {
    id: "gb-native-synthetic",
    sources: &["77117c737c249f1a793cc7acb194dd69d2f25e310ccb7e0166b5264946da51c8"],
    cues: CUES.split_at(11).0,
    playback_clocks: &[
        480_402_756,
        515_795_652,
        129_001_860,
        986_015_556,
        118_046_916,
        68_328_324,
        142_555_092,
        9_550_892,
        770_568_324,
        768_812_724,
        639_249_444,
    ],
};

pub(super) const EXPANDED_PROFILE: Profile = Profile {
    id: "gb-native-synthetic-expanded",
    sources: &["01c16f4a99f1b46259492453a921ff15db0def8c8fcba761975b10ab03511d5a"],
    cues: &expanded_cues(),
    playback_clocks: &expanded_clocks(),
};

const fn expanded_cues() -> [Cue; CUES.len()] {
    let mut cues = [CUES[0]; CUES.len()];
    let mut index = 0;
    while index < cues.len() {
        cues[index] = CUES[index];
        if index >= 11 {
            cues[index].playback_frames = 257;
            cues[index].loop_start_frame = Some(1);
        }
        index += 1;
    }
    cues
}

const fn expanded_clocks() -> [u64; CUES.len()] {
    let mut clocks = [18_047_940; CUES.len()];
    let mut index = 0;
    while index < PROFILE.playback_clocks.len() {
        clocks[index] = PROFILE.playback_clocks[index];
        index += 1;
    }
    clocks
}

pub fn fixture_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x10_0000];
    bytes[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    bytes[0x134..0x13d].copy_from_slice(b"NATIVE GB");
    bytes[0x147..0x14a].copy_from_slice(&[0x13, 5, 3]);
    let startup = [
        0xf3, 0x31, 0xff, 0xdf, 0xaf, 0xe0, 0xfb, 0xe0, 0xfc, 0x3e, 0x91, 0xe0, 0x40, 0xcd, 0x0e,
        0x20, 0xc3, 0xd3, 0x1f,
    ];
    bytes[0x150..0x150 + startup.len()].copy_from_slice(&startup);
    bytes[0x1fd3..0x1fd6].copy_from_slice(&[0xfb, 0x3e, 0x40]);
    let init = [
        0x3e, 2, 0xea, 0, 0x20, 0xe0, 0xb8, 0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x77, 0xe0, 0x24, 0xc9,
    ];
    bytes[0x200e..0x200e + init.len()].copy_from_slice(&init);
    let select = [
        0xea, 0, 0xc0, 0xaf, 0xea, 1, 0xc0, 0x3e, 0x11, 0xe0, 0x25, 0x3e, 0x80, 0xe0, 0x11, 0x3e,
        0xf0, 0xe0, 0x12, 0xfa, 0, 0xc0, 0xe0, 0x13, 0x3e, 0x82, 0xe0, 0x14, 0xc9,
    ];
    bytes[0x23a1..0x23a1 + select.len()].copy_from_slice(&select);
    bytes[0x28cb] = 0xc9;
    let tick = [
        0xfa, 0, 0xc0, 0xfe, 0xe8, 0x20, 6, 0xfa, 1, 0xc0, 0xfe, 0x88, 0xd0, 0xfa, 1, 0xc0, 0x3c,
        0xea, 1, 0xc0, 0x47, 0xfa, 0, 0xc0, 0xfe, 0xe8, 0x20, 0x0b, 0x78, 0xfe, 0x88, 0x38, 6,
        0xaf, 0xe0, 0x25, 0xe0, 0x12, 0xc9, 0x78, 0xe0, 0x13, 0xc9,
    ];
    bytes[0x9103..0x9103 + tick.len()].copy_from_slice(&tick);
    write_groups(&mut bytes, PROFILE.cues);
    bytes[0x14d] = bytes[0x134..0x14d]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_sub(*byte).wrapping_sub(1));
    bytes
}

pub fn expanded_fixture_rom() -> Vec<u8> {
    let mut bytes = fixture_rom();
    write_groups(&mut bytes, &CUES[11..]);
    bytes
}

fn write_groups(bytes: &mut [u8], cues: &[Cue]) {
    for cue in cues {
        for (channel, &(start, _)) in cue.streams.iter().enumerate() {
            let entry = 0x8000 + (usize::from(cue.raw) + channel) * 3;
            bytes[entry] = channel as u8
                | if channel == 0 {
                    (cue.streams.len() as u8 - 1) << 6
                } else {
                    0
                };
            bytes[entry + 1..entry + 3].copy_from_slice(&start.to_le_bytes());
            bytes[0x4000 + usize::from(start)] = 0xff;
        }
    }
}
