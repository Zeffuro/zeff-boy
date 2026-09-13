pub(super) struct Cue {
    pub raw: u8,
    pub streams: &'static [(u16, u16)],
    pub extra: &'static [(u16, u16)],
}

pub(super) struct Profile {
    pub id: &'static str,
    pub sources: &'static [(&'static str, usize)],
    pub cues: &'static [Cue],
    pub bootstrap: u16,
    pub tables_end: u16,
}

pub(super) const CUES: &[Cue] = &[
    Cue {
        raw: 0x90,
        streams: &[(0xfe16, 0xfe24), (0xfe24, 0xfe30), (0xfe30, 0xfe3d)],
        extra: &[],
    },
    Cue {
        raw: 0x93,
        streams: &[(0xf3c4, 0xf3df), (0xf3df, 0xf454), (0xf454, 0xf4c9)],
        extra: &[],
    },
    Cue {
        raw: 0x96,
        streams: &[(0xfb35, 0xfb8c), (0xfb8c, 0xfc06), (0xfc06, 0xfc94)],
        extra: &[(0xfb00, 0xfb01)],
    },
];

const PROFILE: Profile = Profile {
    id: "nes-native-konami-cnrom-01",
    sources: &[
        (
            "b0634c4779b3a289c758c0ae792bce8ae228d58eac3c927b1424feb3ddf20896",
            0x10010,
        ),
        (
            "be8da127fc5bafffffe26493ac7af957c9c5c8f70addb664c595d1f562ac8518",
            0x12010,
        ),
    ],
    cues: CUES,
    bootstrap: 0x847f,
    tables_end: 0xf046,
};

pub(super) fn all_profiles() -> impl Iterator<Item = &'static Profile> {
    let profiles = std::iter::once(&PROFILE);
    #[cfg(any(test, feature = "test-support"))]
    let profiles = profiles.chain(std::iter::once(&super::fixture::PROFILE));
    profiles
}
