use crate::constants::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Region {
    #[default]
    NtscUs,
    NtscJapan,
    Pal,
}

impl Region {
    pub const fn master_hz(self) -> u64 {
        match self {
            Self::Pal => PAL_MASTER_HZ,
            _ => NTSC_MASTER_HZ,
        }
    }

    pub const fn frame_lines(self) -> u64 {
        match self {
            Self::Pal => PAL_FRAME_LINES,
            _ => NTSC_FRAME_LINES,
        }
    }

    pub const fn frame_ticks(self) -> u64 {
        self.frame_lines() * VDP_LINE_TICKS
    }

    pub(crate) const fn io_version(self) -> u8 {
        match self {
            Self::NtscUs => IO_VERSION_NTSC_US,
            Self::NtscJapan => IO_VERSION_NTSC_JAPAN,
            Self::Pal => IO_VERSION_PAL,
        }
    }

    pub(crate) const fn pad_timeout_ticks(self) -> u64 {
        match self {
            Self::Pal => (PAL_MASTER_HZ * PAD_SIX_TIMEOUT_MICROS).div_ceil(1_000_000),
            _ => PAD_SIX_TIMEOUT_TICKS,
        }
    }
}
