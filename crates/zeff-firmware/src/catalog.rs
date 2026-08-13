#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingCoreSystem {
    GameBoy,
    GameBoyAdvance,
    Nes,
    WonderSwan,
    MasterSystem,
    GameGear,
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
        variants: &[FirmwareVariantSpec {
            id: "nec.pce.cd.system_card.v3",
            display_name: "PC Engine CD System Card v3",
            region: "any",
            model: Some("v3"),
            filenames: &["syscard3.pce"],
            size: SizeRule::Exact(262_144),
            hashes: KnownHashes {
                md5: Some("38179df8f4ac870017db21ebcbf53114"),
                sha1: None,
                sha256: None,
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
    fn catalog_uses_stable_semantic_ids_not_filenames() {
        let gba = catalog_specs()
            .iter()
            .find(|spec| spec.id == "nintendo.gba.bios")
            .unwrap();
        assert_eq!(gba.variants[0].filenames, &["gba_bios.bin"]);
        assert_ne!(gba.id, "gba_bios.bin");
    }
}
