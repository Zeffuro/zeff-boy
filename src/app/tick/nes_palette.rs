use super::super::App;
use crate::settings::NesPaletteMode;

pub(super) struct NesPaletteSource {
    key: String,
    label: String,
    kind: NesPaletteSourceKind,
}

enum NesPaletteSourceKind {
    #[cfg(not(target_arch = "wasm32"))]
    Path(String),
    #[cfg(target_arch = "wasm32")]
    Bytes(Vec<u8>),
}

pub(super) fn load_nes_palette_source(
    source: &NesPaletteSource,
) -> Result<zeff_nes_core::hardware::ppu::NesPalette, String> {
    let bytes = match source.kind {
        #[cfg(not(target_arch = "wasm32"))]
        NesPaletteSourceKind::Path(ref path) => {
            std::fs::read(path).map_err(|err| err.to_string())?
        }
        #[cfg(target_arch = "wasm32")]
        NesPaletteSourceKind::Bytes(ref bytes) => bytes.clone(),
    };
    zeff_nes_core::hardware::ppu::parse_nes_palette_bytes(&bytes).map_err(|err| err.to_string())
}

impl App {
    pub(super) fn nes_custom_palette_for_render(
        &mut self,
    ) -> Option<zeff_nes_core::hardware::ppu::NesPalette> {
        if self.settings.video.nes_palette_mode != NesPaletteMode::Custom {
            return None;
        }

        let Some(source) = self.nes_custom_palette_source() else {
            self.nes_palette_cache.path.clear();
            self.nes_palette_cache.palette = None;
            self.nes_palette_cache.error = None;
            return None;
        };

        if self.nes_palette_cache.path != source.key {
            self.nes_palette_cache.path = source.key.clone();
            match load_nes_palette_source(&source) {
                Ok(palette) => {
                    log::info!("Loaded NES palette from {}", source.label);
                    self.nes_palette_cache.palette = Some(palette);
                    self.nes_palette_cache.error = None;
                }
                Err(err) => {
                    log::warn!("Failed to load NES palette from {}: {err}", source.label);
                    self.nes_palette_cache.palette = None;
                    self.nes_palette_cache.error = Some(err);
                }
            }
        }

        self.nes_palette_cache.palette.clone()
    }

    fn nes_custom_palette_source(&self) -> Option<NesPaletteSource> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = self.settings.video.nes_custom_palette_path.trim();
            if path.is_empty() {
                None
            } else {
                Some(NesPaletteSource {
                    key: path.to_string(),
                    label: path.to_string(),
                    kind: NesPaletteSourceKind::Path(path.to_string()),
                })
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            if self.settings.video.nes_custom_palette_bytes.is_empty() {
                return None;
            }
            let name = if self.settings.video.nes_custom_palette_name.is_empty() {
                "uploaded .pal"
            } else {
                &self.settings.video.nes_custom_palette_name
            };
            return Some(NesPaletteSource {
                key: format!(
                    "embedded:{name}:{}",
                    self.settings.video.nes_custom_palette_bytes.len()
                ),
                label: name.to_string(),
                kind: NesPaletteSourceKind::Bytes(
                    self.settings.video.nes_custom_palette_bytes.clone(),
                ),
            });
        }
    }
}
