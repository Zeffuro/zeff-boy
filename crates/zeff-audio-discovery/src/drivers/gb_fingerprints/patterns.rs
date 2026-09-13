// Pinned upstream signature data is independently transcribed; matching stays in the parent module.

#[repr(usize)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PatternId {
    GhxAudio,
    GhxSound,
    DevsoundClassic,
    DevsoundLite,
    DevsoundX,
    DevsoundX2,
    GbMod,
    VisualImpact,
    MusyxAudioTools,
    MusyxAudioToolsUpper,
    MusyxSoundtool,
    FreaqAuthor,
    FreaqDescription,
    LsdjCom,
    LsdjName,
    LsdpackTitle,
    GbsoundModern,
    GbsoundClassic,
    GbsoundName,
    GbsoundAuthor,
    HugeVolSlideV1,
    HugeVolSlideV2,
    HugeFortissimoVolSlide,
    HugeGetNotePoly,
    HugeCoffeeBatShift,
    Trackerboy,
    BlackBoxOne,
    BlackBoxTwo,
    CarillonName,
    CarillonEditor,
    CarillonChannelTwo,
    CarillonAudterm,
    MakrillonAudterm,
    LemonWave,
    GbtWave,
    MplayTwo,
    MplayOne,
    MmlgbOne,
    MmlgbTwo,
    MmlgbRetroHax,
    GbmcExecModv,
    QuickThunderChannelTwo,
    Imedgboy,
    Cosmigo,
    Deflemask,
    Tonicfur,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PatternKind {
    Text,
    Code,
    Data,
    Header,
}

pub(super) struct Pattern {
    pub id: PatternId,
    pub name: &'static str,
    pub bytes: &'static [u8],
    pub at: Option<usize>,
    pub kind: PatternKind,
}

pub(super) struct Rule {
    pub family: &'static str,
    pub variant: &'static str,
    pub required: &'static [PatternId],
}

pub(super) struct FamilyRules {
    pub rules: &'static [Rule],
}

use PatternId::*;
use PatternKind::*;

#[rustfmt::skip]
pub(super) static PATTERNS: &[Pattern] = &[
    Pattern { id: GhxAudio, name: "ghx_audio", bytes: b"GHX Audio Engine", at: None, kind: Text },
    Pattern { id: GhxSound, name: "ghx_sound", bytes: b"GHX Sound Engine", at: None, kind: Text },
    Pattern { id: DevsoundClassic, name: "devsound_classic", bytes: b"DevSound GB music player", at: None, kind: Text },
    Pattern { id: DevsoundLite, name: "devsound_lite", bytes: b"DevSound Lite", at: None, kind: Text },
    Pattern { id: DevsoundX, name: "devsound_x", bytes: b"DevSound X sound driver by DevEd", at: None, kind: Text },
    Pattern { id: DevsoundX2, name: "devsound_x2", bytes: b"DevSound X2 sound driver", at: None, kind: Text },
    Pattern { id: GbMod, name: "dev_gbmod", bytes: b"GBMod music engine by DevEd", at: None, kind: Text },
    Pattern { id: VisualImpact, name: "gbmusicplayer_audio", bytes: b"GB Music Player Copyright VISUAL IMPACT BVBA", at: None, kind: Text },
    Pattern { id: MusyxAudioTools, name: "musyx_1", bytes: b"MusyX Audio Tools", at: None, kind: Text },
    Pattern { id: MusyxAudioToolsUpper, name: "musyx_2", bytes: b"MUSYX AUDIO TOOLS", at: None, kind: Text },
    Pattern { id: MusyxSoundtool, name: "musyx_3", bytes: b"MusyX Soundtool", at: None, kind: Text },
    Pattern { id: FreaqAuthor, name: "freaq_1", bytes: b"Stilianos Doussis", at: None, kind: Text },
    Pattern { id: FreaqDescription, name: "freaq_2", bytes: b"Gameboy Audio-System coded and Music composed by Stilianos Doussis", at: None, kind: Text },
    Pattern { id: LsdjCom, name: "lsdj_1", bytes: b"LITTLESOUNDDJ.COM", at: None, kind: Text },
    Pattern { id: LsdjName, name: "lsdj_2", bytes: b"LITTLE SOUND DJ", at: None, kind: Text },
    Pattern { id: LsdpackTitle, name: "lsdpack_header_title", bytes: b"LSDPACK", at: Some(0x134), kind: Header },
    Pattern { id: GbsoundModern, name: "gbsoundsystem_modern_ssfp_multi_sfx", bytes: &[ 0x57, 0x78, 0x06, 0x00, 0x87, 0xcb, 0x10, 0x87, 0xcb, 0x10, 0x87, 0xcb, 0x10, 0x83, 0x6f, 0x3e, 0x00, 0x8a, 0x80, 0x67, ], at: None, kind: Code },
    Pattern { id: GbsoundClassic, name: "gbsoundsystem_multi_sfx_loop", bytes: &[ 0x2a, 0x4e, 0x06, 0x00, 0x87, 0xcb, 0x10, 0x87, 0xcb, 0x10, 0x87, 0xcb, 0x10, ], at: None, kind: Code },
    Pattern { id: GbsoundName, name: "gbsoundsystem_1", bytes: b"SoundSystem", at: None, kind: Text },
    Pattern { id: GbsoundAuthor, name: "gbsoundsystem_2", bytes: b"Hockenhull", at: None, kind: Text },
    Pattern { id: HugeVolSlideV1, name: "hugetracker_fx_vol_slide_base_v1", bytes: &[ 0x79, 0xe6, 0x0f, 0x57, 0x79, 0xe6, 0xf0, 0x5f, 0xcb, 0x33, 0x7e, 0xe6, 0xf0, 0xcb, 0x37, 0x92, ], at: None, kind: Code },
    Pattern { id: HugeVolSlideV2, name: "hugetracker_fx_vol_slide_base_v2", bytes: &[ 0x79, 0xe6, 0x0f, 0x57, 0x79, 0xe6, 0xf0, 0x5f, 0xcb, 0x33, 0x78, 0x87, 0x87, 0x80, 0xc6, 0x12, 0x4f, 0xf2, 0xe6, 0xf0, 0xcb, 0x37, 0x92, ], at: None, kind: Code },
    Pattern { id: HugeFortissimoVolSlide, name: "hugetracker_fortissimo_fx_vol_slide", bytes: &[ 0xa1, 0xc8, 0xee, 0x02, 0x3c, 0xee, 0x04, 0xc6, 0x12, 0x4f, 0x78, 0x06, 0xf0, 0xa0, 0x67, 0x78, 0xcb, 0x37, 0xa0, 0x6f, 0xf2, 0xa0, 0x95, ], at: None, kind: Code },
    Pattern { id: HugeGetNotePoly, name: "hugetracker_fx_get_note_poly", bytes: &[ 0xc6, 0xc0, 0x2f, 0xfe, 0x07, 0xd8, 0x67, 0xd6, 0x04, 0xcb, 0x3f, 0xcb, 0x3f, 0x6f, 0x7c, 0xe6, 0x03, 0xc6, 0x04, 0xcb, 0x35, 0xb5, 0xc9, ], at: None, kind: Code },
    Pattern { id: HugeCoffeeBatShift, name: "hugetracker_coffeebat_get_shift_ch3", bytes: &[0xcb, 0x37, 0xcb, 0x3f, 0x4f, 0xaf, 0x91, 0xe6, 0x03, 0x4f], at: None, kind: Code },
    Pattern { id: Trackerboy, name: "tbengine_noisetable", bytes: b"tbengine - sound driver by stoneface", at: None, kind: Text },
    Pattern { id: BlackBoxOne, name: "blackboxplayer_1", bytes: &[ 0x09, 0x11, 0x30, 0xff, 0x06, 0x10, 0x2a, 0x12, 0x13, 0x05, 0x20, 0xfa, 0x3e, 0x80, 0xe0, 0x1a, 0xc9, 0xaf, 0xe0, 0x1a, 0xc9, ], at: None, kind: Code },
    Pattern { id: BlackBoxTwo, name: "blackboxplayer_2", bytes: &[ 0xcb, 0x27, 0x4f, 0x06, 0x00, 0x09, 0x2a, 0xfe, 0x00, 0x28, 0x08, 0x47, 0x7e, 0x3d, 0x05, 0xcd, ], at: None, kind: Code },
    Pattern { id: CarillonName, name: "carillon_player_1", bytes: b"CARILLON PLAYER", at: None, kind: Text },
    Pattern { id: CarillonEditor, name: "carillon_player_2", bytes: b"CARILLON EDITOR", at: None, kind: Text },
    Pattern { id: CarillonChannelTwo, name: "carillon_player_3", bytes: &[ 0x47, 0xe6, 0xf0, 0xf6, 0x08, 0xe0, 0x12, 0x78, 0xe6, 0x0f, 0x1d, 0x12, 0x24, 0x2a, 0xcb, 0x2f, 0xcb, 0x18, 0xcb, 0x2f, 0xcb, 0x18, 0x87, 0x67, 0x78, 0xe6, 0xc0, ], at: None, kind: Code },
    Pattern { id: CarillonAudterm, name: "carillon_player_4", bytes: &[ 0x7d, 0xcb, 0x37, 0xe6, 0x0f, 0xc6, 0xc0, 0x4f, 0x06, 0x47, 0x0a, 0x47, 0xf0, 0x25, 0xe6, 0xee, 0xb0, 0xe0, 0x25, ], at: None, kind: Code },
    Pattern { id: MakrillonAudterm, name: "makrillon_1", bytes: &[ 0x7d, 0xcb, 0x37, 0xe6, 0x0f, 0xc6, 0xc0, 0x4f, 0x06, 0xd7, 0x0a, 0x47, 0xf0, 0x25, 0xe6, 0xee, 0xb0, 0xe0, 0x25, ], at: None, kind: Code },
    Pattern { id: LemonWave, name: "lemon_wave_default", bytes: &[ 0xff, 0xff, 0xff, 0xff, 0xff, 0xfd, 0xcb, 0xa8, 0x75, 0x43, 0x21, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xb7, 0x42, 0x23, 0x46, 0x9b, 0xcd, 0xdb, 0x84, 0x00, 0x00, 0x00, 0x00, 0xff, 0x89, 0xcb, 0x88, 0xba, 0x78, 0x99, 0x77, 0x88, 0x66, 0x78, 0x54, 0x77, 0x43, 0x67, 0x00, ], at: None, kind: Data },
    Pattern { id: GbtWave, name: "gbtplayer_gbt_wave", bytes: &[ 0xa5, 0xd7, 0xc9, 0xe1, 0xbc, 0x9a, 0x76, 0x31, 0x0c, 0xba, 0xde, 0x60, 0x1b, 0xca, 0x03, 0x93, 0xf0, 0xe1, 0xd2, 0xc3, 0xb4, 0xa5, 0x96, 0x87, 0x78, 0x69, 0x5a, 0x4b, 0x3c, 0x2d, 0x1e, 0x0f, 0xfd, 0xec, 0xdb, 0xca, 0xb9, 0xa8, 0x97, 0x86, 0x79, 0x68, 0x57, 0x46, 0x35, 0x24, 0x13, 0x02, ], at: None, kind: Data },
    Pattern { id: MplayTwo, name: "mplay2", bytes: &[ 0x07, 0xd9, 0x07, 0xdb, 0x07, 0xdd, 0x07, 0xdf, 0x07, 0x4d, 0x50, 0x6c, 0x61, 0x79, 0x32, ], at: None, kind: Data },
    Pattern { id: MplayOne, name: "mplay1", bytes: &[ 0x07, 0xd9, 0x07, 0xdb, 0x07, 0xdd, 0x07, 0xdf, 0x07, 0xcc, 0xcc, 0xcc, 0xcc, 0x00, 0x00, 0x00, 0x00, 0xcc, 0xcc, 0xcc, 0xcc, 0x00, 0x00, 0x00, 0x00, ], at: None, kind: Data },
    Pattern { id: MmlgbOne, name: "mmlgb1", bytes: &[ 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x07, 0x07, 0x07, 0x07, 0x06, 0x06, 0x06, 0x05, 0x05, 0x04, 0x04, 0x04, 0x03, 0x03, 0x02, 0x02, 0x02, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x02, 0x02, 0x02, 0x03, 0x03, 0x04, ], at: None, kind: Data },
    Pattern { id: MmlgbTwo, name: "mmlgb2", bytes: &[ 0x10, 0x10, 0x10, 0x10, 0x0f, 0x0f, 0x0f, 0x0e, 0x0e, 0x0d, 0x0c, 0x0c, 0x0b, 0x0a, 0x0a, 0x09, 0x08, 0x07, 0x06, 0x06, 0x05, 0x04, 0x04, 0x03, 0x02, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x02, 0x02, 0x03, 0x04, 0x04, 0x05, 0x06, 0x06, 0x07, ], at: None, kind: Data },
    Pattern { id: MmlgbRetroHax, name: "mmlgb_v2", bytes: &[ 55, 54, 53, 52, 39, 38, 37, 36, 23, 22, 21, 20, 0, 0, 0, 0, 7, 6, 5, 4, 3, 2, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, ], at: None, kind: Data },
    Pattern { id: GbmcExecModv, name: "gbmc_snd_exec_modv", bytes: &[ 0x7e, 0xcb, 0x37, 0x0f, 0x4f, 0x3e, 0x04, 0x91, 0xe6, 0x03, 0x80, 0xfe, 0x04, 0x38, 0x06, 0x07, 0x3e, 0x03, 0x30, 0x01, 0xaf, 0x47, 0x3e, 0x04, 0x90, 0xe6, 0x03, 0x87, 0xcb, 0x37, ], at: None, kind: Code },
    Pattern { id: QuickThunderChannelTwo, name: "quickthunder_audio_arts_ch2", bytes: &[ 0xe0, 0x17, 0x3e, 0x80, 0xe0, 0x19, 0x2c, 0x2a, 0xe0, 0x16, 0x5d, 0x54, 0x2c, 0x2a, 0x66, 0x6f, 0x1a, 0x3d, 0x20, 0x0b, 0x23, 0x23, 0x2a, 0xfe, 0x00, ], at: None, kind: Code },
    Pattern { id: Imedgboy, name: "imedgboy", bytes: &[0x49, 0x4d, 0x45, 0x44, 0x47, 0x42, 0x6f, 0x79, 0x14, 0x00], at: None, kind: Data },
    Pattern { id: Cosmigo, name: "cosmigo3", bytes: &[ 0x78, 0xe6, 0x60, 0xcb, 0x27, 0xe0, 0x11, 0x7b, 0xfe, 0x00, 0x28, 0x07, 0xe0, 0x12, 0xe6, 0xf0, ], at: None, kind: Code },
    Pattern { id: Deflemask, name: "deflemask_romstart", bytes: b"DMGBVGM", at: Some(1), kind: Header },
    Pattern { id: Tonicfur, name: "tonicfur", bytes: b"TonicFur Audio Engine", at: None, kind: Text },
];

pub(super) static FAMILIES: &[FamilyRules] = &[
    FamilyRules {
        rules: &[
            Rule {
                family: "GHX",
                variant: "",
                required: &[GhxAudio],
            },
            Rule {
                family: "GHX",
                variant: "",
                required: &[GhxSound],
            },
        ],
    },
    FamilyRules {
        rules: &[
            Rule {
                family: "DevSound",
                variant: "Classic",
                required: &[DevsoundClassic],
            },
            Rule {
                family: "DevSound",
                variant: "Lite",
                required: &[DevsoundLite],
            },
            Rule {
                family: "DevSound",
                variant: "X",
                required: &[DevsoundX],
            },
            Rule {
                family: "DevSound",
                variant: "X2",
                required: &[DevsoundX2],
            },
            Rule {
                family: "GBMod",
                variant: "",
                required: &[GbMod],
            },
        ],
    },
    FamilyRules {
        rules: &[Rule {
            family: "Visual Impact",
            variant: "",
            required: &[VisualImpact],
        }],
    },
    FamilyRules {
        rules: &[
            Rule {
                family: "MusyX",
                variant: "",
                required: &[MusyxAudioTools],
            },
            Rule {
                family: "MusyX",
                variant: "",
                required: &[MusyxAudioToolsUpper],
            },
            Rule {
                family: "MusyX",
                variant: "",
                required: &[MusyxSoundtool],
            },
        ],
    },
    FamilyRules {
        rules: &[
            Rule {
                family: "Freaq",
                variant: "",
                required: &[FreaqAuthor],
            },
            Rule {
                family: "Freaq",
                variant: "",
                required: &[FreaqDescription],
            },
        ],
    },
    FamilyRules {
        rules: &[
            Rule {
                family: "LSDJ",
                variant: "",
                required: &[LsdjCom],
            },
            Rule {
                family: "LSDJ",
                variant: "",
                required: &[LsdjName],
            },
            Rule {
                family: "LSDJ",
                variant: "",
                required: &[LsdpackTitle],
            },
        ],
    },
    FamilyRules {
        rules: &[
            Rule {
                family: "hUGETracker",
                variant: "SuperDisk",
                required: &[HugeVolSlideV1],
            },
            Rule {
                family: "hUGETracker",
                variant: "SuperDisk",
                required: &[HugeVolSlideV2],
            },
            Rule {
                family: "hUGETracker",
                variant: "fortISSimO",
                required: &[HugeFortissimoVolSlide],
            },
            Rule {
                family: "hUGETracker",
                variant: "Coffee Bat",
                required: &[HugeGetNotePoly, HugeCoffeeBatShift],
            },
            Rule {
                family: "hUGETracker",
                variant: "SuperDisk",
                required: &[HugeGetNotePoly],
            },
        ],
    },
    FamilyRules {
        rules: &[Rule {
            family: "Trackerboy engine",
            variant: "",
            required: &[Trackerboy],
        }],
    },
    FamilyRules {
        rules: &[Rule {
            family: "Black Box Music Box",
            variant: "",
            required: &[BlackBoxOne, BlackBoxTwo],
        }],
    },
    FamilyRules {
        rules: &[Rule {
            family: "Lemon",
            variant: "",
            required: &[LemonWave],
        }],
    },
    FamilyRules {
        rules: &[Rule {
            family: "GBT Player",
            variant: "",
            required: &[GbtWave],
        }],
    },
    FamilyRules {
        rules: &[
            Rule {
                family: "Carillon Player",
                variant: "Standard",
                required: &[CarillonName],
            },
            Rule {
                family: "Carillon Player",
                variant: "Standard",
                required: &[CarillonEditor],
            },
            Rule {
                family: "Carillon Player",
                variant: "Makrillon",
                required: &[CarillonChannelTwo, MakrillonAudterm],
            },
            Rule {
                family: "Carillon Player",
                variant: "Standard",
                required: &[CarillonChannelTwo],
            },
        ],
    },
    FamilyRules {
        rules: &[
            Rule {
                family: "MPlay",
                variant: "2",
                required: &[MplayTwo],
            },
            Rule {
                family: "MPlay",
                variant: "1",
                required: &[MplayOne],
            },
        ],
    },
    FamilyRules {
        rules: &[
            Rule {
                family: "GBSoundSystem",
                variant: "Modern",
                required: &[GbsoundModern],
            },
            Rule {
                family: "GBSoundSystem",
                variant: "Classic",
                required: &[GbsoundName, GbsoundAuthor],
            },
            Rule {
                family: "GBSoundSystem",
                variant: "Classic",
                required: &[GbsoundClassic],
            },
        ],
    },
    FamilyRules {
        rules: &[
            Rule {
                family: "MMLGB",
                variant: "Retro-Hax",
                required: &[MmlgbOne, MmlgbRetroHax],
            },
            Rule {
                family: "MMLGB",
                variant: "Retro-Hax",
                required: &[MmlgbTwo, MmlgbRetroHax],
            },
            Rule {
                family: "MMLGB",
                variant: "",
                required: &[MmlgbOne],
            },
            Rule {
                family: "MMLGB",
                variant: "",
                required: &[MmlgbTwo],
            },
        ],
    },
    FamilyRules {
        rules: &[Rule {
            family: "GBMC",
            variant: "",
            required: &[GbmcExecModv],
        }],
    },
    FamilyRules {
        rules: &[Rule {
            family: "QuickThunder",
            variant: "",
            required: &[QuickThunderChannelTwo],
        }],
    },
    FamilyRules {
        rules: &[Rule {
            family: "IMEDGBoy",
            variant: "",
            required: &[Imedgboy],
        }],
    },
    FamilyRules {
        rules: &[Rule {
            family: "Cosmigo",
            variant: "",
            required: &[Cosmigo],
        }],
    },
    FamilyRules {
        rules: &[Rule {
            family: "DefleMask",
            variant: "",
            required: &[Deflemask],
        }],
    },
    FamilyRules {
        rules: &[Rule {
            family: "TonicFur Audio Engine",
            variant: "",
            required: &[Tonicfur],
        }],
    },
];
