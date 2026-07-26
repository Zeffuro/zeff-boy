use super::read_le16;

#[derive(Clone, Copy)]
pub(super) struct ColorEffects {
    control: u16,
    eva: u8,
    evb: u8,
    evy: u8,
}

impl ColorEffects {
    pub(super) fn from_io(io: &[u8]) -> Self {
        let alpha = read_le16(io, 0x52);
        Self {
            control: read_le16(io, 0x50),
            eva: (alpha & 0x1F).min(16) as u8,
            evb: ((alpha >> 8) & 0x1F).min(16) as u8,
            evy: (read_le16(io, 0x54) & 0x1F).min(16) as u8,
        }
    }

    pub(super) fn apply_pixel(
        self,
        color: u16,
        layer: Layer,
        lower: Option<(u16, Layer)>,
        force_alpha: bool,
        effects_enabled: bool,
    ) -> u16 {
        if !effects_enabled {
            return color;
        }

        if let Some((lower_color, lower_layer)) = lower
            && let Some(color) =
                self.alpha_blend_pixel(color, layer, lower_color, lower_layer, force_alpha)
        {
            return color;
        }

        if !self.is_first_target(layer) {
            return color;
        }

        match self.blend_mode() {
            2 => brightness_increase(color, self.evy),
            3 => brightness_decrease(color, self.evy),
            _ => color,
        }
    }

    pub(super) fn alpha_blend_pixel(
        self,
        top_color: u16,
        top_layer: Layer,
        lower_color: u16,
        lower_layer: Layer,
        force_alpha: bool,
    ) -> Option<u16> {
        if (self.blend_mode() == 1 || force_alpha)
            && (force_alpha || self.is_first_target(top_layer))
            && self.is_second_target(lower_layer)
        {
            Some(alpha_blend(top_color, lower_color, self.eva, self.evb))
        } else {
            None
        }
    }

    fn blend_mode(self) -> u16 {
        (self.control >> 6) & 0x3
    }

    fn is_first_target(self, layer: Layer) -> bool {
        self.control & (1 << layer.target_bit()) != 0
    }

    fn is_second_target(self, layer: Layer) -> bool {
        self.control & (1 << (8 + layer.target_bit())) != 0
    }
}

#[derive(Clone, Copy)]
pub(super) enum Layer {
    Bg(usize),
    Obj,
    Backdrop,
}

impl Layer {
    fn target_bit(self) -> u16 {
        match self {
            Self::Bg(bg) => bg as u16,
            Self::Obj => 4,
            Self::Backdrop => 5,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Mosaic {
    bg_width: usize,
    bg_height: usize,
    obj_width: usize,
    obj_height: usize,
}

impl Mosaic {
    pub(super) fn from_io(io: &[u8]) -> Self {
        let raw = read_le16(io, 0x4C);
        Self {
            bg_width: usize::from(raw & 0xF) + 1,
            bg_height: usize::from((raw >> 4) & 0xF) + 1,
            obj_width: usize::from((raw >> 8) & 0xF) + 1,
            obj_height: usize::from((raw >> 12) & 0xF) + 1,
        }
    }

    pub(super) fn bg_sample(self, x: usize, y: usize, enabled: bool) -> (usize, usize) {
        if enabled {
            (x - (x % self.bg_width), y - (y % self.bg_height))
        } else {
            (x, y)
        }
    }

    pub(super) fn obj_sample(self, x: usize, y: usize, enabled: bool) -> (usize, usize) {
        if enabled {
            (x - (x % self.obj_width), y - (y % self.obj_height))
        } else {
            (x, y)
        }
    }
}

fn alpha_blend(top: u16, bottom: u16, eva: u8, evb: u8) -> u16 {
    let eva = u16::from(eva.min(16));
    let evb = u16::from(evb.min(16));
    let r = (((top & 0x1F) * eva + (bottom & 0x1F) * evb) >> 4).min(31);
    let g = ((((top >> 5) & 0x1F) * eva + ((bottom >> 5) & 0x1F) * evb) >> 4).min(31);
    let b = ((((top >> 10) & 0x1F) * eva + ((bottom >> 10) & 0x1F) * evb) >> 4).min(31);
    r | (g << 5) | (b << 10)
}

fn brightness_increase(color: u16, evy: u8) -> u16 {
    adjust_brightness(color, evy, |component, evy| {
        component + (((31 - component) * evy) >> 4)
    })
}

fn brightness_decrease(color: u16, evy: u8) -> u16 {
    adjust_brightness(color, evy, |component, evy| {
        component - ((component * evy) >> 4)
    })
}

fn adjust_brightness(color: u16, evy: u8, adjust: impl Fn(u16, u16) -> u16) -> u16 {
    let evy = u16::from(evy.min(16));
    let r = adjust(color & 0x1F, evy).min(31);
    let g = adjust((color >> 5) & 0x1F, evy).min(31);
    let b = adjust((color >> 10) & 0x1F, evy).min(31);
    r | (g << 5) | (b << 10)
}
