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
        let visible = sprite.filter(|sprite| {
            sprite.priority || background.selected.is_transparent() || !background.selected.priority
        });
        if !background.shadow_highlight {
            return self.pixel_color(registers, visible.unwrap_or(background.selected));
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
        let color = self.pixel_color(registers, selected)?;
        Ok(match intensity {
            Intensity::Shadow => (color >> 1) & SHADOW_COLOR_MASK,
            Intensity::Normal => color,
            Intensity::Highlight => ((color >> 1) & SHADOW_COLOR_MASK) + HIGHLIGHT_COLOR_OFFSET,
        })
    }

    fn pixel_color(
        &self,
        registers: &VdpRegisterPipeline,
        selected: VdpPixel,
    ) -> Result<u16, VdpRenderError> {
        let index = if selected.is_transparent() {
            usize::from(published_register(registers, BACKDROP_REGISTER)? & BACKDROP_COLOR_MASK)
        } else {
            usize::from(selected.palette) * PALETTE_COLORS + usize::from(selected.color)
        };
        Ok(self.cram()[index])
    }
}
