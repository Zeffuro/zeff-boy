use super::*;

#[derive(Clone, Copy)]
enum Intensity {
    Shadow,
    Normal,
    Highlight,
}

impl VdpMemory {
    pub(super) fn composite_color(
        &self,
        registers: &VdpRegisterPipeline,
        background: BackgroundPixel,
        sprite: Option<VdpPixel>,
    ) -> Result<u16, VdpRenderError> {
        self.composite_color_checked::<true>(registers, background, sprite)
    }

    pub(super) fn composite_color_checked<const CHECK: bool>(
        &self,
        registers: &VdpRegisterPipeline,
        background: BackgroundPixel,
        sprite: Option<VdpPixel>,
    ) -> Result<u16, VdpRenderError> {
        let visible = sprite.filter(|sprite| {
            sprite.priority || background.selected.is_transparent() || !background.selected.priority
        });
        if !background.shadow_highlight {
            let selected = visible.unwrap_or(background.selected);
            return self.pixel_color::<CHECK>(registers, selected);
        }
        let mut selected = background.selected;
        let mut intensity = if background.high_priority {
            Intensity::Normal
        } else {
            Intensity::Shadow
        };
        if let Some(sprite) = visible {
            if sprite.palette == INTENSITY_OPERATOR_PALETTE && sprite.color == HIGHLIGHT_OPERATOR {
                intensity = if background.high_priority {
                    Intensity::Highlight
                } else {
                    Intensity::Normal
                };
            } else if sprite.palette == INTENSITY_OPERATOR_PALETTE
                && sprite.color == SHADOW_OPERATOR
            {
                intensity = Intensity::Shadow;
            } else {
                selected = sprite;
                if sprite.priority || sprite.color == HIGHLIGHT_OPERATOR {
                    intensity = Intensity::Normal;
                }
            }
        }
        let color = self.pixel_color::<CHECK>(registers, selected)?;
        Ok(match intensity {
            Intensity::Shadow => (color >> 1) & SHADOW_COLOR_MASK,
            Intensity::Normal => color,
            Intensity::Highlight => ((color >> 1) & SHADOW_COLOR_MASK) + HIGHLIGHT_COLOR_OFFSET,
        })
    }

    fn pixel_color<const CHECK: bool>(
        &self,
        registers: &VdpRegisterPipeline,
        selected: VdpPixel,
    ) -> Result<u16, VdpRenderError> {
        let index = if selected.is_transparent() {
            usize::from(published_register(registers, BACKDROP_REGISTER)? & BACKDROP_COLOR_MASK)
        } else {
            usize::from(selected.palette) * PALETTE_COLORS + usize::from(selected.color)
        };
        self.render_cram::<CHECK>(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::vdp::ControlCommand;

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn composite_color_preserves_backdrop_and_sprite_intensity() {
        let mut memory = VdpMemory::default();
        for (index, color) in [(1, 0x0246), (2, 0x068a), (14, 0x0ace)] {
            memory
                .write_word(
                    ControlCommand {
                        address: index * 2,
                        code: CRAM_WRITE_CODE,
                    },
                    color,
                )
                .unwrap();
        }
        let registers = VdpRegisterPipeline::bootstrap_mode5();
        for (shadow, background, high, sprite_color, palette, sprite_high, expected) in [
            (false, 0, false, 0, 0, false, 0),
            (false, 1, false, 0, 0, false, 0x0246),
            (false, 0, false, 1, 0, false, 0x0246),
            (false, 1, true, 2, 0, false, 0x0246),
            (true, 0, false, 14, 3, true, 0),
            (true, 1, false, 14, 3, true, 0x0246),
            (true, 0, true, 14, 3, true, 0x0777),
            (true, 0, false, 15, 3, true, 0),
            (true, 1, false, 15, 3, true, 0x0123),
            (true, 0, false, 14, 0, true, 0x0ace),
        ] {
            let background = BackgroundPixel {
                selected: VdpPixel {
                    color: background,
                    palette: 0,
                    priority: high,
                },
                high_priority: high,
                shadow_highlight: shadow,
            };
            let sprite = (sprite_color != 0).then_some(VdpPixel {
                color: sprite_color,
                palette,
                priority: sprite_high,
            });
            assert_eq!(
                memory.composite_color(&registers, background, sprite),
                Ok(expected),
            );
        }
    }
}
