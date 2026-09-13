#[derive(Clone, Copy)]
pub(super) struct Cue {
    pub raw: u8,
    pub streams: &'static [(u16, u16)],
    pub playback_frames: u32,
    pub loop_start_frame: Option<u32>,
}

pub(super) struct Profile {
    pub id: &'static str,
    pub sources: &'static [&'static str],
    pub cues: &'static [Cue],
    // Frame-aligned READY/ACK through the final interrupt return, including selector time.
    pub playback_clocks: &'static [u64],
}

pub(super) const CUES: &[Cue] = &[
    Cue {
        raw: 0xba,
        streams: &[(0x67c5, 0x685e), (0x685f, 0x68dd), (0x68de, 0x6912)],
        playback_frames: 6841,
        loop_start_frame: Some(1081),
    },
    Cue {
        raw: 0xc3,
        streams: &[
            (0x6a76, 0x6b92),
            (0x6b92, 0x6c32),
            (0x6c32, 0x6cef),
            (0x6cef, 0x6d77),
        ],
        playback_frames: 7345,
        loop_start_frame: Some(433),
    },
    Cue {
        raw: 0xca,
        streams: &[(0x76c7, 0x773a), (0x773a, 0x77b5), (0x77b5, 0x786d)],
        playback_frames: 1837,
        loop_start_frame: Some(109),
    },
    Cue {
        raw: 0xd0,
        streams: &[
            (0x79eb, 0x7a61),
            (0x7a61, 0x7aa6),
            (0x7aa6, 0x7b0d),
            (0x7b0d, 0x7b58),
        ],
        playback_frames: 14041,
        loop_start_frame: Some(1),
    },
    Cue {
        raw: 0xdb,
        streams: &[(0x6f59, 0x6fa9), (0x6fa9, 0x703d), (0x703d, 0x7119)],
        playback_frames: 1681,
        loop_start_frame: Some(1009),
    },
    Cue {
        raw: 0xe1,
        streams: &[
            (0x6dae, 0x6df1),
            (0x6df1, 0x6e6e),
            (0x6e6e, 0x6ed1),
            (0x6ed1, 0x6f59),
        ],
        playback_frames: 973,
        loop_start_frame: Some(589),
    },
    Cue {
        raw: 0xe5,
        streams: &[(0x7c2e, 0x7c6a), (0x7c6a, 0x7c94), (0x7c94, 0x7cbb)],
        playback_frames: 2030,
        loop_start_frame: Some(50),
    },
    Cue {
        raw: 0xe8,
        streams: &[(0x5ba3, 0x5bc4), (0x5bc4, 0x5bd2), (0x5bd2, 0x5bde)],
        playback_frames: 136,
        loop_start_frame: None,
    },
    Cue {
        raw: 0xeb,
        streams: &[
            (0x5bde, 0x5c50),
            (0x5c51, 0x5cd7),
            (0x5cd8, 0x5d23),
            (0x5d24, 0x5db8),
        ],
        playback_frames: 10973,
        loop_start_frame: Some(29),
    },
    Cue {
        raw: 0xf3,
        streams: &[
            (0x5fad, 0x6009),
            (0x6009, 0x607f),
            (0x607f, 0x6131),
            (0x6131, 0x626a),
        ],
        playback_frames: 10948,
        loop_start_frame: Some(292),
    },
    Cue {
        raw: 0xf7,
        streams: &[
            (0x626a, 0x6304),
            (0x6304, 0x63c4),
            (0x63c4, 0x649d),
            (0x649d, 0x65f0),
        ],
        playback_frames: 9103,
        loop_start_frame: Some(223),
    },
    Cue {
        raw: 0xbd,
        streams: &[(0x7e56, 0x7ef9), (0x7ef9, 0x7f70), (0x7f70, 0x7ff4)],
        playback_frames: 20845,
        loop_start_frame: Some(109),
    },
    Cue {
        raw: 0xc0,
        streams: &[(0x7cbb, 0x7d6b), (0x7d6b, 0x7dfa), (0x7dfa, 0x7e56)],
        playback_frames: 20080,
        loop_start_frame: Some(208),
    },
    Cue {
        raw: 0xc7,
        streams: &[(0x7504, 0x7569), (0x7569, 0x7640), (0x7640, 0x76c7)],
        playback_frames: 21535,
        loop_start_frame: Some(223),
    },
    Cue {
        raw: 0xcd,
        streams: &[(0x786d, 0x78d4), (0x78d4, 0x793d), (0x793d, 0x79eb)],
        playback_frames: 51868,
        loop_start_frame: Some(28),
    },
    Cue {
        raw: 0xd4,
        streams: &[
            (0x7b58, 0x7b9e),
            (0x7b9e, 0x7bae),
            (0x7bae, 0x7c21),
            (0x7c21, 0x7c2e),
        ],
        playback_frames: 328777,
        loop_start_frame: Some(457),
    },
    Cue {
        raw: 0xd8,
        streams: &[(0x73a7, 0x7419), (0x7419, 0x74cb), (0x74cb, 0x7504)],
        playback_frames: 19369,
        loop_start_frame: Some(169),
    },
    Cue {
        raw: 0xde,
        streams: &[(0x7120, 0x719b), (0x71bb, 0x721d), (0x7233, 0x72b5)],
        playback_frames: 58979,
        loop_start_frame: Some(179),
    },
    Cue {
        raw: 0xef,
        streams: &[
            (0x5db9, 0x5e4e),
            (0x5e4f, 0x5e9a),
            (0x5e9b, 0x5f07),
            (0x5f08, 0x5fac),
        ],
        playback_frames: 143641,
        loop_start_frame: Some(1),
    },
    Cue {
        raw: 0xfb,
        streams: &[
            (0x65f0, 0x6664),
            (0x6664, 0x66ba),
            (0x66ba, 0x670f),
            (0x670f, 0x67c5),
        ],
        playback_frames: 31879,
        loop_start_frame: Some(199),
    },
];

const PROFILE: Profile = Profile {
    id: "gb-native-banked-mbc3-bank2-01",
    sources: &[
        "5ca7ba01642a3b27b0cc0b5349b52792795b62d3ed977e98a09390659af96b7b",
        "2a951313c2640e8c2cb21f25d1db019ae6245d9c7121f754fa61afd7bee6452d",
    ],
    cues: CUES,
    playback_clocks: &[
        480_419_784,
        515_823_224,
        129_023_304,
        986_045_404,
        118_065_272,
        68_353_668,
        142_578_168,
        9_557_072,
        770_587_800,
        768_841_580,
        639_276_616,
        1_463_836_116,
        1_410_117_856,
        1_512_293_508,
        3_642_392_132,
        23_088_064_460,
        1_360_185_280,
        4_141_751_924,
        10_087_075_524,
        2_238_702_892,
    ],
};

pub(super) fn all_profiles() -> impl Iterator<Item = &'static Profile> {
    let profiles = std::iter::once(&PROFILE);
    #[cfg(any(test, feature = "test-support"))]
    let profiles = profiles.chain([&super::fixture::PROFILE, &super::fixture::EXPANDED_PROFILE]);
    profiles
}
