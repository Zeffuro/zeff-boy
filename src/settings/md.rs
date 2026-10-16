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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn md_preference_defaults_old_settings_and_roundtrips_each_region() {
        let legacy: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.emulation.md_region, MdRegionPreference::Auto);
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
