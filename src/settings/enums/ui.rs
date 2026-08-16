use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DebugPresentation {
    GameAndDebugger,
    Floating,
    Ide,
}

#[allow(clippy::derivable_impls)]
impl Default for DebugPresentation {
    fn default() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        return Self::GameAndDebugger;

        #[cfg(target_arch = "wasm32")]
        Self::Floating
    }
}

impl crate::debug::ui_helpers::EnumLabel for DebugPresentation {
    fn label(self) -> &'static str {
        match self {
            Self::GameAndDebugger => "Game + Debugger",
            Self::Floating => "Floating",
            Self::Ide => "IDE",
        }
    }

    fn all_variants() -> &'static [Self] {
        #[cfg(not(target_arch = "wasm32"))]
        return &[Self::GameAndDebugger, Self::Floating, Self::Ide];

        #[cfg(target_arch = "wasm32")]
        &[Self::Floating, Self::Ide]
    }
}
