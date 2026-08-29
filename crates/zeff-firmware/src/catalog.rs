#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PceSystemCardRegion {
    Japan,
    Usa,
}

impl PceSystemCardRegion {
    pub const fn catalog_name(self) -> &'static str {
        match self {
            Self::Japan => "japan",
            Self::Usa => "usa",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PceSystemCardTier {
    Version1,
    Version2,
    Version3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PceSystemCardBoard {
    OriginalCdRom2,
    SuperCdRom2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PceSystemCardFirmware {
    variant_id: &'static str,
    region: PceSystemCardRegion,
    tier: PceSystemCardTier,
    board: PceSystemCardBoard,
}

impl PceSystemCardFirmware {
    pub const fn variant_id(self) -> &'static str {
        self.variant_id
    }

    pub const fn region(self) -> PceSystemCardRegion {
        self.region
    }

    pub const fn tier(self) -> PceSystemCardTier {
        self.tier
    }

    pub const fn board(self) -> PceSystemCardBoard {
        self.board
    }
}

pub const PCE_SYSTEM_CARD_V1_JAPAN_SHA256: [u8; 32] =
    decode_sha256(b"afe9f27f91ac918348555b86298b4f984643eafa2773196f2c5441ea84f0c3bb");
pub const PCE_SYSTEM_CARD_V2_JAPAN_SHA256: [u8; 32] =
    decode_sha256(b"0deb13845c7e44ea78a25bbbe324afd60a0ec29ea5a4cf5780349f1598d24cd3");
pub const PCE_SYSTEM_CARD_V2_USA_SHA256: [u8; 32] =
    decode_sha256(b"edba5be43803b180e1d64ca678c3f8bdbf07180c9e2a65a5db69ad635951e6cc");
pub const PCE_SYSTEM_CARD_V3_JAPAN_SHA256: [u8; 32] =
    decode_sha256(b"e11527b3b96ce112a037138988ca72fd117a6b0779c2480d9e03eaebece3d9ce");
pub const PCE_SYSTEM_CARD_V3_USA_SHA256: [u8; 32] =
    decode_sha256(b"cadac2725711b3c442bcf237b02f5a5210c96f17625c35fa58f009e0ed39e4db");
pub const PCE_SYSTEM_CARD_ADPCM_FIXTURE_SHA256: [u8; 32] =
    decode_sha256(b"4f85f6151a41a5b0244caa7fbb43cac8c67ceb596bcd6d6763028918d09cc81d");

pub fn classify_pce_system_card_sha256(sha256: [u8; 32]) -> Option<PceSystemCardFirmware> {
    let (variant_id, region, tier, board) = match sha256 {
        PCE_SYSTEM_CARD_V1_JAPAN_SHA256 => (
            "nec.pce.cd.system_card.v1",
            PceSystemCardRegion::Japan,
            PceSystemCardTier::Version1,
            PceSystemCardBoard::OriginalCdRom2,
        ),
        PCE_SYSTEM_CARD_V2_JAPAN_SHA256 => (
            "nec.pce.cd.system_card.v2",
            PceSystemCardRegion::Japan,
            PceSystemCardTier::Version2,
            PceSystemCardBoard::OriginalCdRom2,
        ),
        PCE_SYSTEM_CARD_V2_USA_SHA256 => (
            "nec.pce.cd.system_card.v2u",
            PceSystemCardRegion::Usa,
            PceSystemCardTier::Version2,
            PceSystemCardBoard::OriginalCdRom2,
        ),
        PCE_SYSTEM_CARD_V3_JAPAN_SHA256 => (
            "nec.pce.cd.system_card.v3",
            PceSystemCardRegion::Japan,
            PceSystemCardTier::Version3,
            PceSystemCardBoard::SuperCdRom2,
        ),
        PCE_SYSTEM_CARD_V3_USA_SHA256 => (
            "nec.pce.cd.system_card.v3u",
            PceSystemCardRegion::Usa,
            PceSystemCardTier::Version3,
            PceSystemCardBoard::SuperCdRom2,
        ),
        PCE_SYSTEM_CARD_ADPCM_FIXTURE_SHA256 => (
            "zeff.pce.cd.adpcm_fixture.v1",
            PceSystemCardRegion::Japan,
            PceSystemCardTier::Version3,
            PceSystemCardBoard::SuperCdRom2,
        ),
        _ => return None,
    };
    Some(PceSystemCardFirmware {
        variant_id,
        region,
        tier,
        board,
    })
}

const fn decode_sha256(hex: &[u8; 64]) -> [u8; 32] {
    let mut output = [0; 32];
    let mut index = 0;
    while index < output.len() {
        output[index] =
            (decode_hex_digit(hex[index * 2]) << 4) | decode_hex_digit(hex[index * 2 + 1]);
        index += 1;
    }
    output
}

const fn decode_hex_digit(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => panic!("invalid hexadecimal digest"),
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FirmwareId(pub String);

impl FirmwareId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl From<&str> for FirmwareId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl AsRef<str> for FirmwareId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FirmwareVariantId(pub String);

impl FirmwareVariantId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl From<&str> for FirmwareVariantId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl AsRef<str> for FirmwareVariantId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementLevel {
    Required,
    Recommended,
    Optional,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareDependency {
    BootOnly,
    RuntimeMapped,
    RuntimeCallable,
    Coprocessor,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallbackKind {
    None,
    SkipBoot {
        compatibility_version: u32,
    },
    Hle {
        implementation: String,
        compatibility_version: u32,
    },
    BuiltinOpenSource {
        implementation: String,
        compatibility_version: u32,
        sha256: [u8; 32],
    },
}

impl FallbackKind {
    pub fn is_available(&self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeRule {
    Exact(u64),
    OneOf(&'static [u64]),
    Range { min: u64, max: u64 },
}

impl SizeRule {
    pub fn matches(self, len: u64) -> bool {
        match self {
            Self::Exact(expected) => len == expected,
            Self::OneOf(values) => values.contains(&len),
            Self::Range { min, max } => (min..=max).contains(&len),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownHashes {
    pub md5: Option<&'static str>,
    pub sha1: Option<&'static str>,
    pub sha256: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareVariantSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub region: &'static str,
    pub model: Option<&'static str>,
    pub filenames: &'static [&'static str],
    pub size: SizeRule,
    pub hashes: KnownHashes,
}

impl FirmwareVariantSpec {
    pub fn filename_matches(&self, filename: &str) -> bool {
        self.filenames
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(filename))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub system: &'static str,
    pub purpose: &'static str,
    pub variants: &'static [FirmwareVariantSpec],
}

impl FirmwareSpec {
    pub fn variant(&self, id: &str) -> Option<&FirmwareVariantSpec> {
        self.variants.iter().find(|variant| variant.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareRequest {
    pub id: FirmwareId,
    pub requirement: RequirementLevel,
    pub fallback: FallbackKind,
    pub dependency: FirmwareDependency,
    pub region: Option<String>,
    pub model: Option<String>,
    pub preferred_variant: Option<FirmwareVariantId>,
}

impl FirmwareRequest {
    pub fn new(
        id: impl Into<FirmwareId>,
        requirement: RequirementLevel,
        fallback: FallbackKind,
        dependency: FirmwareDependency,
    ) -> Self {
        Self {
            id: id.into(),
            requirement,
            fallback,
            dependency,
            region: None,
            model: None,
            preferred_variant: None,
        }
    }

    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_preferred_variant(mut self, variant: impl Into<FirmwareVariantId>) -> Self {
        self.preferred_variant = Some(variant.into());
        self
    }
}

pub fn firmware_plan_for_existing_core(system: ExistingCoreSystem) -> Vec<FirmwareRequest> {
    match system {
        ExistingCoreSystem::GameBoy => vec![
            FirmwareRequest::new(
                "nintendo.gb.boot.dmg",
                RequirementLevel::Optional,
                FallbackKind::SkipBoot {
                    compatibility_version: 1,
                },
                FirmwareDependency::BootOnly,
            ),
            FirmwareRequest::new(
                "nintendo.gb.boot.cgb",
                RequirementLevel::Optional,
                FallbackKind::SkipBoot {
                    compatibility_version: 1,
                },
                FirmwareDependency::BootOnly,
            ),
        ],
        ExistingCoreSystem::GameBoyAdvance => vec![FirmwareRequest::new(
            "nintendo.gba.bios",
            RequirementLevel::Recommended,
            FallbackKind::Hle {
                implementation: "zeff-gba-hle".to_owned(),
                compatibility_version: 1,
            },
            FirmwareDependency::RuntimeCallable,
        )],
        ExistingCoreSystem::MasterSystem => vec![FirmwareRequest::new(
            "sega.sms.boot",
            RequirementLevel::Optional,
            FallbackKind::SkipBoot {
                compatibility_version: 1,
            },
            FirmwareDependency::BootOnly,
        )],
        ExistingCoreSystem::GameGear => vec![FirmwareRequest::new(
            "sega.gg.boot",
            RequirementLevel::Optional,
            FallbackKind::SkipBoot {
                compatibility_version: 1,
            },
            FirmwareDependency::BootOnly,
        )],
        ExistingCoreSystem::ColecoVision => vec![FirmwareRequest::new(
            "coleco.vision.bios",
            RequirementLevel::Required,
            FallbackKind::None,
            FirmwareDependency::RuntimeMapped,
        )],
        ExistingCoreSystem::Nes | ExistingCoreSystem::WonderSwan => Vec::new(),
    }
}

pub fn firmware_plan_for_famicom_disk_system() -> Vec<FirmwareRequest> {
    vec![
        FirmwareRequest::new(
            "nintendo.fds.bios",
            RequirementLevel::Required,
            FallbackKind::None,
            FirmwareDependency::RuntimeMapped,
        )
        .with_region("japan"),
    ]
}

pub fn firmware_plan_for_pce_cdrom2() -> Vec<FirmwareRequest> {
    firmware_plan_for_pce_cdrom2_region("usa")
}

pub fn firmware_plan_for_pce_cdrom2_region(region: &'static str) -> Vec<FirmwareRequest> {
    let preferred = if region == "japan" {
        "nec.pce.cd.system_card.v3"
    } else {
        "nec.pce.cd.system_card.v3u"
    };
    vec![
        FirmwareRequest::new(
            "nec.pce.cd.system_card",
            RequirementLevel::Required,
            FallbackKind::None,
            FirmwareDependency::RuntimeMapped,
        )
        .with_region(region)
        .with_preferred_variant(preferred),
    ]
}

pub fn firmware_plan_for_pce_cdrom2_fixture() -> Vec<FirmwareRequest> {
    vec![
        FirmwareRequest::new(
            "zeff.pce.cd.adpcm_fixture_card",
            RequirementLevel::Required,
            FallbackKind::None,
            FirmwareDependency::RuntimeMapped,
        )
        .with_preferred_variant("zeff.pce.cd.adpcm_fixture.v1"),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingCoreSystem {
    GameBoy,
    GameBoyAdvance,
    Nes,
    WonderSwan,
    MasterSystem,
    GameGear,
    ColecoVision,
}

pub fn catalog_specs() -> &'static [FirmwareSpec] {
    CATALOG_SPECS
}

const CATALOG_SPECS: &[FirmwareSpec] = &[
    FirmwareSpec {
        id: "nintendo.gb.boot.dmg",
        display_name: "Game Boy DMG boot ROM",
        system: "Game Boy",
        purpose: "DMG startup boot ROM",
        variants: &[FirmwareVariantSpec {
            id: "nintendo.gb.boot.dmg.retail",
            display_name: "Game Boy DMG retail boot ROM",
            region: "any",
            model: Some("dmg"),
            filenames: &["dmg_boot.bin", "gb_bios.bin"],
            size: SizeRule::Exact(256),
            hashes: KnownHashes {
                md5: Some("32fbbd84168d3482956eb3c5051637f5"),
                sha1: Some("4ed31ec6b0b175bb109c0eb5fd3d193da823339f"),
                sha256: None,
            },
        }],
    },
    FirmwareSpec {
        id: "nintendo.gb.boot.cgb",
        display_name: "Game Boy Color boot ROM",
        system: "Game Boy Color",
        purpose: "CGB startup boot ROM",
        variants: &[FirmwareVariantSpec {
            id: "nintendo.gb.boot.cgb.retail",
            display_name: "Game Boy Color retail boot ROM",
            region: "any",
            model: Some("cgb"),
            filenames: &["cgb_boot.bin", "gbc_bios.bin"],
            size: SizeRule::Exact(2304),
            hashes: KnownHashes {
                md5: Some("dbfce9db9deaa2567f6a84fde55f9680"),
                sha1: None,
                sha256: None,
            },
        }],
    },
    FirmwareSpec {
        id: "nintendo.gba.bios",
        display_name: "Game Boy Advance BIOS",
        system: "Game Boy Advance",
        purpose: "GBA BIOS code and system calls",
        variants: &[FirmwareVariantSpec {
            id: "nintendo.gba.bios.agb",
            display_name: "Game Boy Advance BIOS",
            region: "any",
            model: Some("agb"),
            filenames: &["gba_bios.bin"],
            size: SizeRule::Exact(16_384),
            hashes: KnownHashes {
                md5: Some("a860e8c0b6d573d191e4ec7db1b1e4f6"),
                sha1: None,
                sha256: None,
            },
        }],
    },
    FirmwareSpec {
        id: "nintendo.fds.bios",
        display_name: "Famicom Disk System BIOS",
        system: "NES / Famicom Disk System",
        purpose: "FDS disk runtime BIOS",
        variants: &[FirmwareVariantSpec {
            id: "nintendo.fds.bios.retail",
            display_name: "Famicom Disk System BIOS",
            region: "japan",
            model: None,
            filenames: &["disksys.rom"],
            size: SizeRule::Exact(8192),
            hashes: KnownHashes {
                md5: Some("ca30b50f880eb660a320674ed365ef7a"),
                sha1: None,
                sha256: None,
            },
        }],
    },
    FirmwareSpec {
        id: "sega.sms.boot",
        display_name: "Master System boot ROM",
        system: "Master System",
        purpose: "SMS startup boot ROM",
        variants: &[
            FirmwareVariantSpec {
                id: "sega.sms.boot.export",
                display_name: "Master System export boot ROM",
                region: "export",
                model: None,
                filenames: &["bios_E.sms", "bios_U.sms", "bios.sms"],
                size: SizeRule::Exact(8192),
                hashes: KnownHashes {
                    md5: Some("840481177270d5642a14ca71ee72844c"),
                    sha1: None,
                    sha256: None,
                },
            },
            FirmwareVariantSpec {
                id: "sega.sms.boot.japan",
                display_name: "Master System Japan boot ROM",
                region: "japan",
                model: None,
                filenames: &["bios_J.sms"],
                size: SizeRule::Exact(8192),
                hashes: KnownHashes {
                    md5: Some("24a519c53f67b00640d0048ef7089105"),
                    sha1: None,
                    sha256: None,
                },
            },
        ],
    },
    FirmwareSpec {
        id: "sega.gg.boot",
        display_name: "Game Gear boot ROM",
        system: "Game Gear",
        purpose: "Game Gear startup boot ROM",
        variants: &[FirmwareVariantSpec {
            id: "sega.gg.boot.retail",
            display_name: "Game Gear boot ROM",
            region: "any",
            model: None,
            filenames: &["bios.gg"],
            size: SizeRule::Exact(1024),
            hashes: KnownHashes {
                md5: Some("672e104c3be3a238301aceffc3b23fd6"),
                sha1: None,
                sha256: None,
            },
        }],
    },
    FirmwareSpec {
        id: "coleco.vision.bios",
        display_name: "ColecoVision BIOS",
        system: "ColecoVision",
        purpose: "ColecoVision startup and system services",
        variants: &[FirmwareVariantSpec {
            id: "coleco.vision.bios.retail",
            display_name: "ColecoVision retail BIOS",
            region: "ntsc",
            model: None,
            filenames: &[
                "coleco.rom",
                "colecovision.rom",
                "BIOS.col",
                "313 10031-4005 73108a.u2",
            ],
            size: SizeRule::Exact(8192),
            hashes: KnownHashes {
                md5: Some("2c66f5911e5b42b8ebe113403548eee7"),
                sha1: None,
                sha256: None,
            },
        }],
    },
    FirmwareSpec {
        id: "sega.megacd.bios.usa",
        display_name: "Sega CD BIOS (USA)",
        system: "Sega CD / Mega-CD",
        purpose: "US CD boot/runtime BIOS",
        variants: &[FirmwareVariantSpec {
            id: "sega.megacd.bios.usa.common",
            display_name: "Sega CD BIOS (USA)",
            region: "usa",
            model: None,
            filenames: &["bios_CD_U.bin"],
            size: SizeRule::Exact(131_072),
            hashes: KnownHashes {
                md5: Some("854b9150240a198070150e4566ae1290"),
                sha1: None,
                sha256: None,
            },
        }],
    },
    FirmwareSpec {
        id: "sega.megacd.bios.europe",
        display_name: "Mega-CD BIOS (Europe)",
        system: "Sega CD / Mega-CD",
        purpose: "European CD boot/runtime BIOS",
        variants: &[FirmwareVariantSpec {
            id: "sega.megacd.bios.europe.common",
            display_name: "Mega-CD BIOS (Europe)",
            region: "europe",
            model: None,
            filenames: &["bios_CD_E.bin"],
            size: SizeRule::Exact(131_072),
            hashes: KnownHashes {
                md5: Some("e66fa1dc5820d254611fdcdba0662372"),
                sha1: None,
                sha256: None,
            },
        }],
    },
    FirmwareSpec {
        id: "sega.megacd.bios.japan",
        display_name: "Mega-CD BIOS (Japan)",
        system: "Sega CD / Mega-CD",
        purpose: "Japanese CD boot/runtime BIOS",
        variants: &[FirmwareVariantSpec {
            id: "sega.megacd.bios.japan.common",
            display_name: "Mega-CD BIOS (Japan)",
            region: "japan",
            model: None,
            filenames: &["bios_CD_J.bin"],
            size: SizeRule::Exact(131_072),
            hashes: KnownHashes {
                md5: Some("278a9397d192149e84e820ac621a8edd"),
                sha1: None,
                sha256: None,
            },
        }],
    },
    FirmwareSpec {
        id: "nec.pce.cd.system_card",
        display_name: "PC Engine CD System Card",
        system: "PC Engine / TurboGrafx-CD",
        purpose: "CD-ROM System Card",
        variants: &[
            FirmwareVariantSpec {
                id: "nec.pce.cd.system_card.v1",
                display_name: "PC Engine CD System Card v1",
                region: "japan",
                model: Some("v1"),
                filenames: &["syscard1.pce"],
                size: SizeRule::Exact(262_144),
                hashes: KnownHashes {
                    md5: Some("2b7ccb3d86baa18f6402c176f3065082"),
                    sha1: None,
                    sha256: Some(
                        "afe9f27f91ac918348555b86298b4f984643eafa2773196f2c5441ea84f0c3bb",
                    ),
                },
            },
            FirmwareVariantSpec {
                id: "nec.pce.cd.system_card.v2",
                display_name: "PC Engine CD System Card v2",
                region: "japan",
                model: Some("v2"),
                filenames: &["syscard2.pce"],
                size: SizeRule::Exact(262_144),
                hashes: KnownHashes {
                    md5: Some("3cdd6614a918616bfc41c862e889dd79"),
                    sha1: None,
                    sha256: Some(
                        "0deb13845c7e44ea78a25bbbe324afd60a0ec29ea5a4cf5780349f1598d24cd3",
                    ),
                },
            },
            FirmwareVariantSpec {
                id: "nec.pce.cd.system_card.v2u",
                display_name: "TurboGrafx-CD System Card v2",
                region: "usa",
                model: Some("v2"),
                filenames: &["syscard2u.pce"],
                size: SizeRule::Exact(262_144),
                hashes: KnownHashes {
                    md5: Some("94279f315e8b52904f65ab3108542afe"),
                    sha1: None,
                    sha256: Some(
                        "edba5be43803b180e1d64ca678c3f8bdbf07180c9e2a65a5db69ad635951e6cc",
                    ),
                },
            },
            FirmwareVariantSpec {
                id: "nec.pce.cd.system_card.v3",
                display_name: "PC Engine CD System Card v3",
                region: "japan",
                model: Some("v3"),
                filenames: &["syscard3.pce"],
                size: SizeRule::Exact(262_144),
                hashes: KnownHashes {
                    md5: Some("38179df8f4ac870017db21ebcbf53114"),
                    sha1: None,
                    sha256: Some(
                        "e11527b3b96ce112a037138988ca72fd117a6b0779c2480d9e03eaebece3d9ce",
                    ),
                },
            },
            FirmwareVariantSpec {
                id: "nec.pce.cd.system_card.v3u",
                display_name: "TurboGrafx-CD Super System Card v3",
                region: "usa",
                model: Some("v3"),
                filenames: &["syscard3u.pce"],
                size: SizeRule::Exact(262_144),
                hashes: KnownHashes {
                    md5: Some("0754f903b52e3b3342202bdafb13efa5"),
                    sha1: None,
                    sha256: Some(
                        "cadac2725711b3c442bcf237b02f5a5210c96f17625c35fa58f009e0ed39e4db",
                    ),
                },
            },
        ],
    },
    FirmwareSpec {
        id: "zeff.pce.cd.adpcm_fixture_card",
        display_name: "Zeff PC Engine CD ADPCM Fixture Card",
        system: "PC Engine CD-ROM2",
        purpose: "Open end-to-end ADPCM fixture boot card",
        variants: &[FirmwareVariantSpec {
            id: "zeff.pce.cd.adpcm_fixture.v1",
            display_name: "Zeff PC Engine CD ADPCM Fixture Card",
            region: "japan",
            model: Some("fixture-v1"),
            filenames: &["syscard3.pce"],
            size: SizeRule::Exact(262_144),
            hashes: KnownHashes {
                md5: None,
                sha1: None,
                sha256: Some("4f85f6151a41a5b0244caa7fbb43cac8c67ceb596bcd6d6763028918d09cc81d"),
            },
        }],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_ws_core_has_empty_firmware_plan() {
        assert!(firmware_plan_for_existing_core(ExistingCoreSystem::WonderSwan).is_empty());
    }

    #[test]
    fn existing_gba_plan_uses_hle_fallback() {
        let plan = firmware_plan_for_existing_core(ExistingCoreSystem::GameBoyAdvance);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].id.as_ref(), "nintendo.gba.bios");
        assert_eq!(plan[0].requirement, RequirementLevel::Recommended);
        assert!(matches!(plan[0].fallback, FallbackKind::Hle { .. }));
    }

    #[test]
    fn famicom_disk_system_plan_requires_runtime_bios() {
        let plan = firmware_plan_for_famicom_disk_system();

        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].id.as_ref(), "nintendo.fds.bios");
        assert_eq!(plan[0].requirement, RequirementLevel::Required);
        assert_eq!(plan[0].fallback, FallbackKind::None);
        assert_eq!(plan[0].dependency, FirmwareDependency::RuntimeMapped);
        assert_eq!(plan[0].region.as_deref(), Some("japan"));
    }

    #[test]
    fn pce_cdrom2_plan_prefers_exact_super_system_card_v3u() {
        let plan = firmware_plan_for_pce_cdrom2();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].id.as_ref(), "nec.pce.cd.system_card");
        assert_eq!(plan[0].requirement, RequirementLevel::Required);
        assert_eq!(plan[0].fallback, FallbackKind::None);
        assert_eq!(plan[0].region.as_deref(), Some("usa"));
        assert_eq!(plan[0].model, None);
        assert_eq!(
            plan[0]
                .preferred_variant
                .as_ref()
                .map(|variant| variant.0.as_str()),
            Some("nec.pce.cd.system_card.v3u")
        );
        let variant = catalog_specs()
            .iter()
            .find(|spec| spec.id == "nec.pce.cd.system_card")
            .unwrap()
            .variants
            .iter()
            .find(|variant| variant.id == "nec.pce.cd.system_card.v3u")
            .unwrap();
        assert_eq!(variant.size, SizeRule::Exact(262_144));
        assert_eq!(variant.hashes.md5, Some("0754f903b52e3b3342202bdafb13efa5"));
        assert_eq!(
            variant.hashes.sha256,
            Some("cadac2725711b3c442bcf237b02f5a5210c96f17625c35fa58f009e0ed39e4db")
        );
    }

    #[test]
    fn pce_cdrom2_fixture_plan_only_matches_the_open_fixture_card() {
        let plan = firmware_plan_for_pce_cdrom2_fixture();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].id.as_ref(), "zeff.pce.cd.adpcm_fixture_card");
        assert_eq!(
            plan[0].preferred_variant.as_ref().map(AsRef::as_ref),
            Some("zeff.pce.cd.adpcm_fixture.v1")
        );
    }

    #[test]
    fn exact_system_card_hashes_classify_region_tier_and_board() {
        for (sha256, variant_id, region, tier, board) in [
            (
                PCE_SYSTEM_CARD_V1_JAPAN_SHA256,
                "nec.pce.cd.system_card.v1",
                PceSystemCardRegion::Japan,
                PceSystemCardTier::Version1,
                PceSystemCardBoard::OriginalCdRom2,
            ),
            (
                PCE_SYSTEM_CARD_V2_JAPAN_SHA256,
                "nec.pce.cd.system_card.v2",
                PceSystemCardRegion::Japan,
                PceSystemCardTier::Version2,
                PceSystemCardBoard::OriginalCdRom2,
            ),
            (
                PCE_SYSTEM_CARD_V2_USA_SHA256,
                "nec.pce.cd.system_card.v2u",
                PceSystemCardRegion::Usa,
                PceSystemCardTier::Version2,
                PceSystemCardBoard::OriginalCdRom2,
            ),
            (
                PCE_SYSTEM_CARD_V3_JAPAN_SHA256,
                "nec.pce.cd.system_card.v3",
                PceSystemCardRegion::Japan,
                PceSystemCardTier::Version3,
                PceSystemCardBoard::SuperCdRom2,
            ),
            (
                PCE_SYSTEM_CARD_V3_USA_SHA256,
                "nec.pce.cd.system_card.v3u",
                PceSystemCardRegion::Usa,
                PceSystemCardTier::Version3,
                PceSystemCardBoard::SuperCdRom2,
            ),
            (
                PCE_SYSTEM_CARD_ADPCM_FIXTURE_SHA256,
                "zeff.pce.cd.adpcm_fixture.v1",
                PceSystemCardRegion::Japan,
                PceSystemCardTier::Version3,
                PceSystemCardBoard::SuperCdRom2,
            ),
        ] {
            let firmware = classify_pce_system_card_sha256(sha256).unwrap();
            assert_eq!(firmware.variant_id(), variant_id);
            assert_eq!(firmware.region(), region);
            assert_eq!(firmware.tier(), tier);
            assert_eq!(firmware.board(), board);
        }
        assert_eq!(classify_pce_system_card_sha256([0; 32]), None);
    }

    #[test]
    fn catalog_uses_stable_semantic_ids_not_filenames() {
        let gba = catalog_specs()
            .iter()
            .find(|spec| spec.id == "nintendo.gba.bios")
            .unwrap();
        assert_eq!(gba.variants[0].filenames, &["gba_bios.bin"]);
        assert_ne!(gba.id, "gba_bios.bin");
    }
}
