use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum VsyncMode {
    Off,
    #[default]
    On,
    Adaptive,
}

impl crate::debug::ui_helpers::EnumLabel for VsyncMode {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "Off (tearing allowed)",
            Self::On => "On (VSync)",
            Self::Adaptive => "Adaptive",
        }
    }

    fn all_variants() -> &'static [Self] {
        &[Self::On, Self::Adaptive, Self::Off]
    }
}

impl VsyncMode {

    pub(crate) fn to_present_mode(
        self,
        capabilities: &[wgpu::PresentMode],
    ) -> wgpu::PresentMode {
        match self {
            Self::Off => {
                if capabilities.contains(&wgpu::PresentMode::Immediate) {
                    wgpu::PresentMode::Immediate
                } else if capabilities.contains(&wgpu::PresentMode::Mailbox) {
                    wgpu::PresentMode::Mailbox
                } else {
                    wgpu::PresentMode::Fifo
                }
            }
            Self::On => wgpu::PresentMode::Fifo,
            Self::Adaptive => {
                if capabilities.contains(&wgpu::PresentMode::AutoVsync) {
                    wgpu::PresentMode::AutoVsync
                } else {
                    wgpu::PresentMode::Fifo
                }
            }
        }
    }
}

