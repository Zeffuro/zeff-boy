use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ColorCorrection {
    #[default]
    None,
    GbcLcd,
    Custom,
}

impl ColorCorrection {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None (raw RGB)",
            Self::GbcLcd => "GBC LCD panel",
            Self::Custom => "Custom matrix",
        }
    }
}

pub fn default_color_correction_matrix() -> [f32; 9] {
    [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
}

pub fn gbc_lcd_matrix() -> [f32; 9] {
    [
        0.8125, 0.125, 0.0625, 0.0, 0.75, 0.25, 0.1875, 0.125, 0.6875,
    ]
}
