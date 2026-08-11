use std::path::Path;

use super::constants::{
    SEGA8_NTSC_FRAME_RATE_APPROX, SEGA8_PAL_FRAME_RATE_APPROX, SMS_NTSC_TOTAL_SCANLINES,
    SMS_PAL_TOTAL_SCANLINES, SMS_SCANLINE_Z80_CYCLES,
};

const NTSC_192_LINEAR_VCOUNTER_END_SCANLINE: u16 = 0xDA;
const NTSC_192_POST_VISIBLE_VCOUNTER_START: u16 = 0xD5;
const PAL_192_LINEAR_VCOUNTER_END_SCANLINE: u16 = 0xF2;
const PAL_192_POST_VISIBLE_VCOUNTER_START: u16 = 0xBA;

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

    pub fn v_counter_for_192_line_scanline(self, scanline: u16) -> u8 {
        let scanline = scanline % self.total_scanlines();
        let (linear_end, post_visible_start) = match self {
            Self::Ntsc => (
                NTSC_192_LINEAR_VCOUNTER_END_SCANLINE,
                NTSC_192_POST_VISIBLE_VCOUNTER_START,
            ),
            Self::Pal => (
                PAL_192_LINEAR_VCOUNTER_END_SCANLINE,
                PAL_192_POST_VISIBLE_VCOUNTER_START,
            ),
        };

        if scanline <= linear_end {
            scanline as u8
        } else {
            (post_visible_start + scanline - linear_end - 1) as u8
        }
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
}
