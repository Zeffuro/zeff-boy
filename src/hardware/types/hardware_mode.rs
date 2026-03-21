use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum HardwareMode {
    DMG,
    SGB1,
    SGB2,
    CGBNormal,
    CGBDouble,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum HardwareModePreference {
    Auto,
    ForceDmg,
    ForceCgb,
}

impl HardwareModePreference {
    pub(crate) fn resolve(
        self,
        is_cgb_compatible: bool,
        is_sgb_supported: bool,
        old_licensee_code: u8,
    ) -> HardwareMode {
        match self {
            HardwareModePreference::Auto => {
                if is_cgb_compatible {
                    HardwareMode::CGBNormal
                } else if is_sgb_supported && old_licensee_code == 0x33 {
                    HardwareMode::SGB1
                } else {
                    HardwareMode::DMG
                }
            }
            HardwareModePreference::ForceDmg => HardwareMode::DMG,
            HardwareModePreference::ForceCgb => {
                if is_cgb_compatible {
                    HardwareMode::CGBNormal
                } else {
                    HardwareMode::DMG
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HardwareMode, HardwareModePreference};

    #[test]
    fn auto_prefers_cgb_for_cgb_roms() {
        let mode = HardwareModePreference::Auto.resolve(true, true, 0x33);
        assert_eq!(mode, HardwareMode::CGBNormal);
    }

    #[test]
    fn auto_uses_sgb_when_header_matches() {
        let mode = HardwareModePreference::Auto.resolve(false, true, 0x33);
        assert_eq!(mode, HardwareMode::SGB1);
    }

    #[test]
    fn auto_falls_back_to_dmg_when_not_sgb() {
        let mode = HardwareModePreference::Auto.resolve(false, true, 0x01);
        assert_eq!(mode, HardwareMode::DMG);
    }
}

