use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HardwareMode {
    DMG,
    SGB1,
    SGB2,
    CGBNormal,
    CGBDouble,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HardwareModePreference {
    Auto,
    ForceDmg,
    ForceSgb,
    ForceCgb,
}

impl HardwareModePreference {
    pub fn resolve(
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
            HardwareModePreference::ForceSgb => {
                if is_sgb_supported && old_licensee_code == 0x33 {
                    HardwareMode::SGB1
                } else {
                    HardwareMode::DMG
                }
            }
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
mod tests;
