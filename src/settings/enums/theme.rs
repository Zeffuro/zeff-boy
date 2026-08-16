use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UiThemePreset {
    #[default]
    DefaultDark,
    HighContrastDark,
    Light,
    Retro,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UiDensity {
    #[default]
    Compact,
    Comfortable,
}

impl crate::debug::ui_helpers::EnumLabel for UiDensity {
    fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Comfortable => "Comfortable",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[Self::Compact, Self::Comfortable]
    }
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
