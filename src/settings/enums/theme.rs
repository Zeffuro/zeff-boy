use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum UiThemePreset {
    #[default]
    DefaultDark,
    HighContrastDark,
    Light,
    Retro,
}

impl crate::debug::ui_helpers::EnumLabel for UiThemePreset {
    fn label(self) -> &'static str {
        match self {
            Self::DefaultDark => "Default Dark",
            Self::HighContrastDark => "High Contrast Dark",
            Self::Light => "Light",
            Self::Retro => "Retro",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[
            Self::DefaultDark,
            Self::HighContrastDark,
            Self::Light,
            Self::Retro,
        ]
    }
}

