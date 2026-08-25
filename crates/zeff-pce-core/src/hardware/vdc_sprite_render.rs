use super::vdc::{HuC6270, VdcRegister};

const SPRITE_COUNT: usize = 64;
const SATB_WORDS_PER_SPRITE: usize = 4;
const SPRITES_PER_LINE: usize = 16;
const SPRITE_CELL_SIZE: usize = 16;
const SPRITE_CELL_WORDS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteColorMode {
    Full,
    PlanePair,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpriteRenderState {
    enabled: bool,
    color_mode: SpriteColorMode,
}

impl SpriteRenderState {
    pub(super) fn from_register_values(control: u16, memory_width: u16) -> Self {
        Self {
            enabled: control & 0x40 != 0,
            color_mode: if (memory_width >> 2) & 3 == 1 {
                SpriteColorMode::PlanePair
            } else {
                SpriteColorMode::Full
            },
        }
    }

    #[inline]
    pub const fn enabled(self) -> bool {
        self.enabled
    }

    #[inline]
    pub const fn color_mode(self) -> SpriteColorMode {
        self.color_mode
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteBackgroundPriority {
    Background,
    Sprite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpritePixel {
    palette_index: u16,
    background_priority: SpriteBackgroundPriority,
    sat_index: u8,
}

impl SpritePixel {
    #[inline]
    pub const fn palette_index(self) -> u16 {
        self.palette_index
    }

    #[inline]
    pub const fn background_priority(self) -> SpriteBackgroundPriority {
        self.background_priority
    }

    #[inline]
    pub const fn sat_index(self) -> u8 {
        self.sat_index
    }

    #[inline]
    pub(crate) const fn new(
        palette_index: u16,
        background_priority: SpriteBackgroundPriority,
        sat_index: u8,
    ) -> Self {
        Self {
            palette_index,
            background_priority,
            sat_index,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpriteScanlineEvents {
    collision_within_output: bool,
    overflow: bool,
}

impl SpriteScanlineEvents {
    #[inline]
    pub const fn collision_within_output(self) -> bool {
        self.collision_within_output
    }

    #[inline]
    pub const fn overflow(self) -> bool {
        self.overflow
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteScanlineStatus {
    Disabled,
    Rendered(SpriteScanlineEvents),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteRenderError {
    VramAddressUnavailable { word: u16 },
}

#[derive(Clone, Copy)]
struct SpriteLine {
    sat_index: u8,
    left: i32,
    source_y: usize,
    width: usize,
    height: usize,
    pattern_code: u16,
    attributes: u16,
}

impl HuC6270 {
    pub fn sprite_render_state(&self) -> SpriteRenderState {
        SpriteRenderState::from_register_values(
            self.register(VdcRegister::Control),
            self.register(VdcRegister::MemoryWidth),
        )
    }

    pub fn render_sprite_scanline(
        &self,
        state: &SpriteRenderState,
        display_line: usize,
        output: &mut [Option<SpritePixel>],
    ) -> Result<SpriteScanlineStatus, SpriteRenderError> {
        if !state.enabled {
            return Ok(SpriteScanlineStatus::Disabled);
        }

        let (sprites, sprite_count, overflow) = self.select_sprite_lines(display_line);
        output.fill(None);
        let mut collision = false;
        for sprite in sprites[..sprite_count].iter().flatten() {
            self.render_sprite_line(state, *sprite, output, &mut collision);
        }
        Ok(SpriteScanlineStatus::Rendered(SpriteScanlineEvents {
            collision_within_output: collision,
            overflow,
        }))
    }

    fn select_sprite_lines(
        &self,
        display_line: usize,
    ) -> ([Option<SpriteLine>; SPRITES_PER_LINE], usize, bool) {
        let display_line = i64::try_from(display_line).unwrap_or(i64::MAX);
        let mut sprites = [None; SPRITES_PER_LINE];
        let mut count = 0;
        let mut overflow = false;

        for sat_index in 0..SPRITE_COUNT {
            let base = sat_index * SATB_WORDS_PER_SPRITE;
            let attributes = self.satb()[base + 3];
            let height = match (attributes >> 12) & 3 {
                0 => 16,
                1 => 32,
                _ => 64,
            };
            let top = i64::from(self.satb()[base] & 0x03FF) - 64;
            let row = display_line.saturating_sub(top);
            if row < 0 || row >= height as i64 {
                continue;
            }
            if count == SPRITES_PER_LINE {
                overflow = true;
                break;
            }

            let source_y = if attributes & 0x8000 == 0 {
                row as usize
            } else {
                height - 1 - row as usize
            };
            sprites[count] = Some(SpriteLine {
                sat_index: sat_index as u8,
                left: i32::from(self.satb()[base + 1] & 0x03FF) - 32,
                source_y,
                width: if attributes & 0x0100 == 0 { 16 } else { 32 },
                height,
                pattern_code: self.satb()[base + 2] & 0x07FF,
                attributes,
            });
            count += 1;
        }
        (sprites, count, overflow)
    }

    fn render_sprite_line(
        &self,
        state: &SpriteRenderState,
        sprite: SpriteLine,
        output: &mut [Option<SpritePixel>],
        collision: &mut bool,
    ) {
        let Some(visible_x) = sprite_visible_local_x_range(sprite, output.len()) else {
            return;
        };

        let cell_y = sprite.source_y / SPRITE_CELL_SIZE;
        let row = sprite.source_y % SPRITE_CELL_SIZE;
        let flipped = sprite.attributes & 0x0800 != 0;
        let palette = 0x0100 | ((sprite.attributes & 0x000F) << 4);
        let priority = if sprite.attributes & 0x0080 == 0 {
            SpriteBackgroundPriority::Background
        } else {
            SpriteBackgroundPriority::Sprite
        };
        let mut local_x = visible_x.start;
        while local_x < visible_x.end {
            let source_x = if flipped {
                sprite.width - 1 - local_x
            } else {
                local_x
            };
            let cell_x = source_x / SPRITE_CELL_SIZE;
            let count = if flipped {
                (source_x % SPRITE_CELL_SIZE + 1).min(visible_x.end - local_x)
            } else {
                (SPRITE_CELL_SIZE - source_x % SPRITE_CELL_SIZE).min(visible_x.end - local_x)
            };
            let base = sprite_pattern_cell_base(sprite, cell_x, cell_y);
            let planes = match state.color_mode {
                SpriteColorMode::Full => [
                    self.sprite_pattern_word(base + row),
                    self.sprite_pattern_word(base + 16 + row),
                    self.sprite_pattern_word(base + 32 + row),
                    self.sprite_pattern_word(base + 48 + row),
                ],
                SpriteColorMode::PlanePair => {
                    let first = usize::from(sprite.pattern_code & 1) * 2;
                    [
                        self.sprite_pattern_word(base + first * 16 + row),
                        self.sprite_pattern_word(base + (first + 1) * 16 + row),
                        0,
                        0,
                    ]
                }
            };
            for offset in 0..count {
                let source_column = if flipped {
                    source_x - offset
                } else {
                    source_x + offset
                };
                let bit = 15 - source_column % 16;
                let color = ((planes[0] >> bit) & 1) as u8
                    | (((planes[1] >> bit) & 1) as u8) << 1
                    | (((planes[2] >> bit) & 1) as u8) << 2
                    | (((planes[3] >> bit) & 1) as u8) << 3;
                write_sprite_pixel(
                    &mut output[(i64::from(sprite.left) + (local_x + offset) as i64) as usize],
                    color,
                    palette,
                    priority,
                    sprite.sat_index,
                    collision,
                );
            }
            local_x += count;
        }
    }

    #[inline]
    fn sprite_pattern_word(&self, word: usize) -> u16 {
        self.read_logical_vram_word(word as u16)
    }
}

#[inline]
fn write_sprite_pixel(
    destination: &mut Option<SpritePixel>,
    color: u8,
    palette: u16,
    priority: SpriteBackgroundPriority,
    sat_index: u8,
    collision: &mut bool,
) {
    if color == 0 {
        return;
    }
    if let Some(existing) = destination {
        if existing.sat_index == 0 && sat_index != 0 {
            *collision = true;
        }
    } else {
        *destination = Some(SpritePixel::new(
            palette | u16::from(color),
            priority,
            sat_index,
        ));
    }
}

#[inline]
fn sprite_pattern_cell_base(sprite: SpriteLine, cell_x: usize, cell_y: usize) -> usize {
    let mut pattern_code = sprite.pattern_code & 0x07FE;
    if sprite.width == 32 {
        pattern_code &= !0x0002;
    }
    if sprite.height == 32 {
        pattern_code &= !0x0004;
    } else if sprite.height == 64 {
        pattern_code &= !0x000C;
    }
    (usize::from(pattern_code) << 5) + (cell_y * 2 + cell_x) * SPRITE_CELL_WORDS
}

#[inline]
fn sprite_visible_local_x_range(
    sprite: SpriteLine,
    output_len: usize,
) -> Option<std::ops::Range<usize>> {
    let output_end = i64::try_from(output_len).unwrap_or(i64::MAX);
    let start = i64::from(sprite.left).max(0);
    let end = (i64::from(sprite.left) + sprite.width as i64).min(output_end);
    (start < end)
        .then(|| (start - i64::from(sprite.left)) as usize..(end - i64::from(sprite.left)) as usize)
}
