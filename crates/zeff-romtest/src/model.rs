use std::fmt;
use std::path::PathBuf;

use anyhow::bail;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Core {
    Gb,
    Gba,
    Nes,
    Ws,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CoreSpec {
    pub(crate) core: Core,
    pub(crate) code: &'static str,
    pub(crate) rom_extensions: &'static [&'static str],
}

const GB_ROM_EXTENSIONS: [&str; 2] = ["gb", "gbc"];
const GBA_ROM_EXTENSIONS: [&str; 1] = ["gba"];
const NES_ROM_EXTENSIONS: [&str; 1] = ["nes"];
const WS_ROM_EXTENSIONS: [&str; 2] = ["ws", "wsc"];
const SUPPORTED_CORE_VALUES: &str = "gb|gba|nes|ws";

const CORE_SPECS: &[CoreSpec] = &[
    CoreSpec {
        core: Core::Gb,
        code: "gb",
        rom_extensions: &GB_ROM_EXTENSIONS,
    },
    CoreSpec {
        core: Core::Gba,
        code: "gba",
        rom_extensions: &GBA_ROM_EXTENSIONS,
    },
    CoreSpec {
        core: Core::Nes,
        code: "nes",
        rom_extensions: &NES_ROM_EXTENSIONS,
    },
    CoreSpec {
        core: Core::Ws,
        code: "ws",
        rom_extensions: &WS_ROM_EXTENSIONS,
    },
];

impl Core {
    pub(crate) fn specs() -> &'static [CoreSpec] {
        CORE_SPECS
    }

    pub(crate) fn code(self) -> &'static str {
        self.spec().code
    }

    pub(crate) fn supported_values() -> &'static str {
        SUPPORTED_CORE_VALUES
    }

    pub(crate) fn from_extension(ext: &str) -> Option<Self> {
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        Self::specs()
            .iter()
            .find(|spec| spec.rom_extensions.contains(&ext.as_str()))
            .map(|spec| spec.core)
    }

    fn from_code(value: &str) -> Option<Self> {
        Self::specs()
            .iter()
            .find(|spec| spec.code == value)
            .map(|spec| spec.core)
    }

    fn spec(self) -> &'static CoreSpec {
        Self::specs()
            .iter()
            .find(|spec| spec.core == self)
            .expect("Core must have a CoreSpec")
    }
}

impl std::str::FromStr for Core {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_code(value).ok_or_else(|| {
            anyhow::anyhow!(
                "invalid core '{value}', expected {}",
                Self::supported_values()
            )
        })
    }
}

impl fmt::Display for Core {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Tier {
    Smoke,
    Accuracy,
    Visual,
    Local,
    Compat,
}

impl std::str::FromStr for Tier {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "smoke" => Ok(Self::Smoke),
            "accuracy" => Ok(Self::Accuracy),
            "visual" => Ok(Self::Visual),
            "local" => Ok(Self::Local),
            "compat" => Ok(Self::Compat),
            _ => bail!("invalid tier '{value}', expected smoke|accuracy|visual|local|compat"),
        }
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Smoke => "smoke",
            Self::Accuracy => "accuracy",
            Self::Visual => "visual",
            Self::Local => "local",
            Self::Compat => "compat",
        })
    }
}

fn default_tier() -> Tier {
    Tier::Accuracy
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ArtifactKind {
    TestRom,
    GameRom,
}

impl fmt::Display for ArtifactKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TestRom => "test_rom",
            Self::GameRom => "game_rom",
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LicenseConfidence {
    Verified,
    CollectionVerified,
    Unknown,
    UserOwned,
}

impl fmt::Display for LicenseConfidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Verified => "verified",
            Self::CollectionVerified => "collection_verified",
            Self::Unknown => "unknown",
            Self::UserOwned => "user_owned",
        })
    }
}

pub(crate) fn default_license_confidence() -> LicenseConfidence {
    LicenseConfidence::Unknown
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PassKind {
    GbFibonacciLdBB,
    GbSerialContains,
    GbScreenText,
    GbMemoryStatus,
    #[serde(rename = "nes_6000_status")]
    Nes6000Status,
    GbaScreenText,
    GbaMgbaSuiteSram,
    GbaScreenshot,
    WsScreenText,
    WsPassFailTiles,
    ScreenshotExact,
    ScreenshotPerceptual,
    HeadlessExit,
    Manual,
}

impl fmt::Display for PassKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::GbFibonacciLdBB => "gb_fibonacci_ld_b_b",
            Self::GbSerialContains => "gb_serial_contains",
            Self::GbScreenText => "gb_screen_text",
            Self::GbMemoryStatus => "gb_memory_status",
            Self::Nes6000Status => "nes_6000_status",
            Self::GbaScreenText => "gba_screen_text",
            Self::GbaMgbaSuiteSram => "gba_mgba_suite_sram",
            Self::GbaScreenshot => "gba_screenshot",
            Self::WsScreenText => "ws_screen_text",
            Self::WsPassFailTiles => "ws_pass_fail_tiles",
            Self::ScreenshotExact => "screenshot_exact",
            Self::ScreenshotPerceptual => "screenshot_perceptual",
            Self::HeadlessExit => "headless_exit",
            Self::Manual => "manual",
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExpectationKind {
    Pass,
    KnownFail,
    Skip,
}

impl fmt::Display for ExpectationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Pass => "pass",
            Self::KnownFail => "known_fail",
            Self::Skip => "skip",
        })
    }
}

fn default_expectation_kind() -> ExpectationKind {
    ExpectationKind::Pass
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Manifest {
    pub(crate) manifest_version: u32,
    pub(crate) suite: Suite,
    #[serde(default)]
    pub(crate) tests: Vec<TestCase>,
    #[serde(default)]
    pub(crate) test_groups: Vec<TestGroup>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Suite {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) core: Option<Core>,
    pub(crate) upstream_url: Option<String>,
    pub(crate) license: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct TestCase {
    pub(crate) id: String,
    pub(crate) core: Core,
    #[serde(default = "default_tier")]
    pub(crate) tier: Tier,
    pub(crate) model: Option<String>,
    #[serde(default = "default_max_frames")]
    pub(crate) max_frames: u64,
    #[serde(default)]
    pub(crate) no_apu: bool,
    #[serde(default)]
    pub(crate) input: Vec<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    pub(crate) notes: Option<String>,
    pub(crate) artifact: Artifact,
    pub(crate) rom: RomSpec,
    pub(crate) pass: PassSpec,
    #[serde(default)]
    pub(crate) expectation: Expectation,
}

fn default_max_frames() -> u64 {
    600
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct TestGroup {
    pub(crate) id_prefix: String,
    pub(crate) cache_prefix: PathBuf,
    pub(crate) core: Core,
    #[serde(default = "default_tier")]
    pub(crate) tier: Tier,
    pub(crate) model: Option<String>,
    #[serde(default = "default_max_frames")]
    pub(crate) max_frames: u64,
    #[serde(default)]
    pub(crate) no_apu: bool,
    #[serde(default)]
    pub(crate) input: Vec<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    pub(crate) notes: Option<String>,
    pub(crate) artifact: Artifact,
    pub(crate) pass: PassSpec,
    #[serde(default)]
    pub(crate) expectation: Expectation,
    #[serde(default)]
    pub(crate) roms: Vec<TestGroupRom>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct TestGroupRom {
    pub(crate) id: String,
    pub(crate) archive_path: String,
    pub(crate) sha256: Option<String>,
    pub(crate) path: Option<PathBuf>,
    #[serde(default)]
    pub(crate) legacy_paths: Vec<PathBuf>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) input: Vec<String>,
    pub(crate) notes: Option<String>,
    pub(crate) pass: Option<PassSpec>,
    pub(crate) expectation: Option<Expectation>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Artifact {
    pub(crate) kind: ArtifactKind,
    pub(crate) license: String,
    #[serde(default = "default_license_confidence")]
    pub(crate) license_confidence: LicenseConfidence,
    #[serde(default)]
    pub(crate) redistributable: bool,
    pub(crate) source_url: Option<String>,
    pub(crate) source_version: Option<String>,
    pub(crate) source_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct RomSpec {
    pub(crate) path: PathBuf,
    pub(crate) sha256: Option<String>,
    pub(crate) archive_path: Option<String>,
    #[serde(default)]
    pub(crate) legacy_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PassSpec {
    pub(crate) kind: PassKind,
    pub(crate) contains: Option<String>,
    pub(crate) screenshot_frame: Option<u64>,
    pub(crate) screenshot_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Expectation {
    #[serde(default = "default_expectation_kind")]
    pub(crate) kind: ExpectationKind,
    pub(crate) reason: Option<String>,
}

impl Default for Expectation {
    fn default() -> Self {
        Self {
            kind: ExpectationKind::Pass,
            reason: None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LoadedTest {
    pub(crate) manifest_path: PathBuf,
    pub(crate) suite: Suite,
    pub(crate) test: TestCase,
}
