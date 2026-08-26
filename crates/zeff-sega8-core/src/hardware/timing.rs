use std::path::Path;

use super::constants::{
    SEGA8_NTSC_FRAME_RATE_APPROX, SEGA8_PAL_FRAME_RATE_APPROX, SMS_NTSC_TOTAL_SCANLINES,
    SMS_PAL_TOTAL_SCANLINES, SMS_SCANLINE_Z80_CYCLES,
};

const NTSC_192_LINEAR_VCOUNTER_END_SCANLINE: u16 = 0xDA;
const NTSC_192_POST_VISIBLE_VCOUNTER_START: u16 = 0xD5;
const NTSC_224_LINEAR_VCOUNTER_END_SCANLINE: u16 = 0xEA;
const NTSC_224_POST_VISIBLE_VCOUNTER_START: u16 = 0xE5;
const PAL_192_LINEAR_VCOUNTER_END_SCANLINE: u16 = 0xF2;
const PAL_192_POST_VISIBLE_VCOUNTER_START: u16 = 0xBA;
const PAL_224_LINEAR_VCOUNTER_END_SCANLINE: u16 = 0xFF;
const PAL_224_RESTART_VCOUNTER_END_SCANLINE: u16 = 0x102;
const PAL_224_POST_VISIBLE_VCOUNTER_START: u16 = 0xCA;
const PAL_240_LINEAR_VCOUNTER_END_SCANLINE: u16 = 0xFF;
const PAL_240_RESTART_VCOUNTER_END_SCANLINE: u16 = 0x10A;
const PAL_240_POST_VISIBLE_VCOUNTER_START: u16 = 0xD2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sega8DisplayHeight {
    Lines192,
    Lines224,
    Lines240,
}

impl Sega8DisplayHeight {
    pub const fn lines(self) -> u16 {
        match self {
            Self::Lines192 => 192,
            Self::Lines224 => 224,
            Self::Lines240 => 240,
        }
    }

    pub const fn frame_interrupt_scanline(self) -> u16 {
        self.lines() + 1
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sega8VideoStandard {
    #[default]
    Ntsc,
    Pal,
}

impl Sega8VideoStandard {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ntsc => "ntsc",
            Self::Pal => "pal",
        }
    }

    pub fn display_label(self) -> &'static str {
        match self {
            Self::Ntsc => "NTSC",
            Self::Pal => "PAL",
        }
    }

    pub fn total_scanlines(self) -> u16 {
        match self {
            Self::Ntsc => SMS_NTSC_TOTAL_SCANLINES,
            Self::Pal => SMS_PAL_TOTAL_SCANLINES,
        }
    }

    pub fn frame_rate_approx(self) -> u32 {
        match self {
            Self::Ntsc => SEGA8_NTSC_FRAME_RATE_APPROX,
            Self::Pal => SEGA8_PAL_FRAME_RATE_APPROX,
        }
    }

    pub fn cycles_per_frame(self) -> u32 {
        SMS_SCANLINE_Z80_CYCLES * u32::from(self.total_scanlines())
    }

    pub fn clock_hz_approx(self) -> u32 {
        self.cycles_per_frame() * self.frame_rate_approx()
    }

    pub fn nominal_frame_duration_ns(self) -> u64 {
        1_000_000_000u64.div_ceil(u64::from(self.frame_rate_approx()))
    }

    pub fn v_counter_for_scanline(self, display_height: Sega8DisplayHeight, scanline: u16) -> u8 {
        let scanline = scanline % self.total_scanlines();
        match (self, display_height) {
            (Self::Ntsc, Sega8DisplayHeight::Lines192) => v_counter_with_post_visible_sequence(
                scanline,
                NTSC_192_LINEAR_VCOUNTER_END_SCANLINE,
                NTSC_192_POST_VISIBLE_VCOUNTER_START,
            ),
            (Self::Ntsc, Sega8DisplayHeight::Lines224) => v_counter_with_post_visible_sequence(
                scanline,
                NTSC_224_LINEAR_VCOUNTER_END_SCANLINE,
                NTSC_224_POST_VISIBLE_VCOUNTER_START,
            ),
            (Self::Ntsc, Sega8DisplayHeight::Lines240) => scanline as u8,
            (Self::Pal, Sega8DisplayHeight::Lines192) => v_counter_with_post_visible_sequence(
                scanline,
                PAL_192_LINEAR_VCOUNTER_END_SCANLINE,
                PAL_192_POST_VISIBLE_VCOUNTER_START,
            ),
            (Self::Pal, Sega8DisplayHeight::Lines224) => {
                v_counter_with_restart_and_post_visible_sequence(
                    scanline,
                    PAL_224_LINEAR_VCOUNTER_END_SCANLINE,
                    PAL_224_RESTART_VCOUNTER_END_SCANLINE,
                    PAL_224_POST_VISIBLE_VCOUNTER_START,
                )
            }
            (Self::Pal, Sega8DisplayHeight::Lines240) => {
                v_counter_with_restart_and_post_visible_sequence(
                    scanline,
                    PAL_240_LINEAR_VCOUNTER_END_SCANLINE,
                    PAL_240_RESTART_VCOUNTER_END_SCANLINE,
                    PAL_240_POST_VISIBLE_VCOUNTER_START,
                )
            }
        }
    }

    pub fn v_counter_for_192_line_scanline(self, scanline: u16) -> u8 {
        self.v_counter_for_scanline(Sega8DisplayHeight::Lines192, scanline)
    }

    pub fn parse(value: &str) -> Option<Self> {
        let normalized = normalize_region_tag(value);
        match normalized.as_str() {
            "ntsc" | "60hz" | "usa" | "us" | "japan" | "jp" | "jpn" => Some(Self::Ntsc),
            "pal" | "50hz" | "europe" | "eu" | "eur" | "australia" | "aus" => Some(Self::Pal),
            _ => None,
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let text = path.to_string_lossy().to_ascii_lowercase();
        Self::from_region_tag_text(&text)
    }

    fn from_region_tag_text(text: &str) -> Option<Self> {
        if ["[e]", "(e)"].iter().any(|tag| text.contains(tag)) {
            return Some(Self::Pal);
        }
        if ["[u]", "(u)", "[j]", "(j)"]
            .iter()
            .any(|tag| text.contains(tag))
        {
            return Some(Self::Ntsc);
        }

        let tokens = text
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();

        let pal_tokens = ["europe", "eu", "eur", "pal", "australia", "aus"];
        if tokens.iter().any(|token| pal_tokens.contains(token)) {
            return Some(Self::Pal);
        }

        let ntsc_tokens = ["usa", "us", "japan", "jp", "jpn", "ntsc"];
        if tokens.iter().any(|token| ntsc_tokens.contains(token)) {
            return Some(Self::Ntsc);
        }

        None
    }
}

fn v_counter_with_post_visible_sequence(
    scanline: u16,
    linear_end: u16,
    post_visible_start: u16,
) -> u8 {
    if scanline <= linear_end {
        scanline as u8
    } else {
        (post_visible_start + scanline - linear_end - 1) as u8
    }
}

fn v_counter_with_restart_and_post_visible_sequence(
    scanline: u16,
    linear_end: u16,
    restart_end: u16,
    post_visible_start: u16,
) -> u8 {
    if scanline <= linear_end {
        scanline as u8
    } else if scanline <= restart_end {
        (scanline - linear_end - 1) as u8
    } else {
        (post_visible_start + scanline - restart_end - 1) as u8
    }
}

fn normalize_region_tag(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_video_standard_values() {
        assert_eq!(
            Sega8VideoStandard::parse("pal"),
            Some(Sega8VideoStandard::Pal)
        );
        assert_eq!(
            Sega8VideoStandard::parse("60Hz"),
            Some(Sega8VideoStandard::Ntsc)
        );
        assert_eq!(Sega8VideoStandard::parse("bogus"), None);
    }

    #[test]
    fn detects_common_region_tags_from_paths() {
        assert_eq!(
            Sega8VideoStandard::from_path(Path::new("Game (Europe).sms")),
            Some(Sega8VideoStandard::Pal)
        );
        assert_eq!(
            Sega8VideoStandard::from_path(Path::new("Game [U].sms")),
            Some(Sega8VideoStandard::Ntsc)
        );
        assert_eq!(Sega8VideoStandard::from_path(Path::new("Game.sms")), None);
    }

    #[test]
    fn nominal_frame_duration_tracks_the_video_standard() {
        assert_eq!(
            Sega8VideoStandard::Ntsc.nominal_frame_duration_ns(),
            16_666_667
        );
        assert_eq!(
            Sega8VideoStandard::Pal.nominal_frame_duration_ns(),
            20_000_000
        );
    }

    #[test]
    fn ntsc_192_line_v_counter_uses_sms_post_visible_sequence() {
        let standard = Sega8VideoStandard::Ntsc;

        assert_eq!(standard.v_counter_for_192_line_scanline(0), 0x00);
        assert_eq!(
            standard.v_counter_for_192_line_scanline(NTSC_192_LINEAR_VCOUNTER_END_SCANLINE),
            0xDA
        );
        assert_eq!(
            standard.v_counter_for_192_line_scanline(NTSC_192_LINEAR_VCOUNTER_END_SCANLINE + 1),
            0xD5
        );
        assert_eq!(standard.v_counter_for_192_line_scanline(261), 0xFF);
        assert_eq!(standard.v_counter_for_192_line_scanline(262), 0x00);
    }

    #[test]
    fn pal_192_line_v_counter_uses_sms_post_visible_sequence() {
        let standard = Sega8VideoStandard::Pal;

        assert_eq!(standard.v_counter_for_192_line_scanline(0), 0x00);
        assert_eq!(
            standard.v_counter_for_192_line_scanline(PAL_192_LINEAR_VCOUNTER_END_SCANLINE),
            0xF2
        );
        assert_eq!(
            standard.v_counter_for_192_line_scanline(PAL_192_LINEAR_VCOUNTER_END_SCANLINE + 1),
            0xBA
        );
        assert_eq!(standard.v_counter_for_192_line_scanline(312), 0xFF);
        assert_eq!(standard.v_counter_for_192_line_scanline(313), 0x00);
    }

    #[test]
    fn ntsc_224_line_v_counter_uses_sms_tv_detection_sequence() {
        let standard = Sega8VideoStandard::Ntsc;
        let height = Sega8DisplayHeight::Lines224;

        assert_eq!(standard.v_counter_for_scanline(height, 0xEA), 0xEA);
        assert_eq!(standard.v_counter_for_scanline(height, 0xEB), 0xE5);
        assert_eq!(standard.v_counter_for_scanline(height, 261), 0xFF);
    }

    #[test]
    fn ntsc_240_line_v_counter_wraps_after_255() {
        let standard = Sega8VideoStandard::Ntsc;
        let height = Sega8DisplayHeight::Lines240;

        assert_eq!(standard.v_counter_for_scanline(height, 0xFF), 0xFF);
        assert_eq!(standard.v_counter_for_scanline(height, 0x100), 0x00);
        assert_eq!(standard.v_counter_for_scanline(height, 261), 0x05);
    }

    #[test]
    fn pal_224_line_v_counter_uses_restart_and_post_visible_sequence() {
        let standard = Sega8VideoStandard::Pal;
        let height = Sega8DisplayHeight::Lines224;

        assert_eq!(standard.v_counter_for_scanline(height, 0xFF), 0xFF);
        assert_eq!(standard.v_counter_for_scanline(height, 0x100), 0x00);
        assert_eq!(standard.v_counter_for_scanline(height, 0x102), 0x02);
        assert_eq!(standard.v_counter_for_scanline(height, 0x103), 0xCA);
        assert_eq!(standard.v_counter_for_scanline(height, 312), 0xFF);
    }

    #[test]
    fn pal_240_line_v_counter_uses_restart_and_post_visible_sequence() {
        let standard = Sega8VideoStandard::Pal;
        let height = Sega8DisplayHeight::Lines240;

        assert_eq!(standard.v_counter_for_scanline(height, 0xFF), 0xFF);
        assert_eq!(standard.v_counter_for_scanline(height, 0x100), 0x00);
        assert_eq!(standard.v_counter_for_scanline(height, 0x10A), 0x0A);
        assert_eq!(standard.v_counter_for_scanline(height, 0x10B), 0xD2);
        assert_eq!(standard.v_counter_for_scanline(height, 312), 0xFF);
    }
}
