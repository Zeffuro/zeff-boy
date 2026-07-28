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
}

impl std::str::FromStr for Core {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "gb" => Ok(Self::Gb),
            "gba" => Ok(Self::Gba),
            "nes" => Ok(Self::Nes),
            _ => bail!("invalid core '{value}', expected gb|gba|nes"),
        }
    }
}

impl fmt::Display for Core {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Gb => "gb",
            Self::Gba => "gba",
            Self::Nes => "nes",
        })
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
    GbaScreenshot,
    ScreenshotExact,
    ScreenshotPerceptual,
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
            Self::GbaScreenshot => "gba_screenshot",
            Self::ScreenshotExact => "screenshot_exact",
            Self::ScreenshotPerceptual => "screenshot_perceptual",
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
