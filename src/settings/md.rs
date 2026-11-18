use serde::{Deserialize, Serialize};
use zeff_md_core::Region;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) enum MdRegionPreference {
    #[default]
    Auto,
    NtscUs,
    NtscJapan,
    Pal,
}

impl MdRegionPreference {
    pub(crate) const fn forced_region(self) -> Option<Region> {
        match self {
            Self::Auto => None,
            Self::NtscUs => Some(Region::NtscUs),
            Self::NtscJapan => Some(Region::NtscJapan),
            Self::Pal => Some(Region::Pal),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) enum MdTimingPreference {
    #[default]
    Standard,
    Precise,
}

impl MdTimingPreference {
    pub(crate) const fn core_model(self) -> zeff_md_core::TimingModel {
        match self {
            Self::Standard => zeff_md_core::TimingModel::Standard,
            Self::Precise => zeff_md_core::TimingModel::Precise,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) enum MdControllerPreference {
    ThreeButton,
    #[default]
    SixButton,
}

impl MdControllerPreference {
    pub(crate) const fn core_kind(self) -> zeff_md_core::ControllerKind {
        match self {
            Self::ThreeButton => zeff_md_core::ControllerKind::ThreeButton,
            Self::SixButton => zeff_md_core::ControllerKind::SixButton,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(try_from = "RawSvpFunctionalProfile", into = "RawSvpFunctionalProfile")]
pub(crate) struct MdSvpFunctionalProfile {
    raw: RawSvpFunctionalProfile,
    profile: zeff_md_core::svp::FunctionalProfile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RawSvpFunctionalProfile {
    identity: u64,
    numerator: u32,
    denominator: u32,
    costs: [u16; 6],
}

impl TryFrom<RawSvpFunctionalProfile> for MdSvpFunctionalProfile {
    type Error = zeff_md_core::svp::Error;

    fn try_from(raw: RawSvpFunctionalProfile) -> Result<Self, Self::Error> {
        let profile = zeff_md_core::svp::FunctionalProfile::new(
            raw.identity,
            raw.numerator,
            raw.denominator,
            raw.costs,
        )?;
        Ok(Self { raw, profile })
    }
}

impl From<MdSvpFunctionalProfile> for RawSvpFunctionalProfile {
    fn from(value: MdSvpFunctionalProfile) -> Self {
        value.raw
    }
}

impl MdSvpFunctionalProfile {
    pub(crate) fn core_profile(self) -> zeff_md_core::svp::FunctionalProfile {
        self.profile
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn md_preference_defaults_old_settings_and_roundtrips_each_region() {
        let legacy: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.emulation.md_region, MdRegionPreference::Auto);
        assert_eq!(legacy.emulation.md_timing, MdTimingPreference::Standard);
        assert_eq!(
            legacy.emulation.md_controller,
            MdControllerPreference::SixButton
        );
        assert!(!legacy.emulation.md_use_tmss);
        let mut settings = Settings::default();
        settings.emulation.md_use_tmss = true;
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(restored.emulation.md_use_tmss);
        for preference in [
            MdControllerPreference::ThreeButton,
            MdControllerPreference::SixButton,
        ] {
            settings.emulation.md_controller = preference;
            let restored: Settings =
                serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
            assert_eq!(restored.emulation.md_controller, preference);
        }
        for (preference, timing) in [
            (
                MdTimingPreference::Standard,
                zeff_md_core::TimingModel::Standard,
            ),
            (
                MdTimingPreference::Precise,
                zeff_md_core::TimingModel::Precise,
            ),
        ] {
            let mut settings = Settings::default();
            settings.emulation.md_timing = preference;
            let json = serde_json::to_string(&settings).unwrap();
            let restored: Settings = serde_json::from_str(&json).unwrap();
            assert_eq!(restored.emulation.md_timing, preference);
            assert_eq!(restored.emulation.md_timing.core_model(), timing);
        }
        for (preference, region) in [
            (MdRegionPreference::Auto, None),
            (MdRegionPreference::NtscUs, Some(Region::NtscUs)),
            (MdRegionPreference::NtscJapan, Some(Region::NtscJapan)),
            (MdRegionPreference::Pal, Some(Region::Pal)),
        ] {
            let mut settings = Settings::default();
            settings.emulation.md_region = preference;
            let json = serde_json::to_string(&settings).unwrap();
            let restored: Settings = serde_json::from_str(&json).unwrap();
            assert_eq!(restored.emulation.md_region, preference);
            assert_eq!(restored.emulation.md_region.forced_region(), region);
        }
    }
}
