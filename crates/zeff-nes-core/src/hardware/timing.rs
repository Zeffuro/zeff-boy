use crate::hardware::constants::{
    DENDY_MASTER_CLOCK_HZ_DENOMINATOR, DENDY_MASTER_CLOCK_HZ_NUMERATOR,
    DENDY_MASTER_TICKS_PER_CPU_CYCLE, DENDY_MASTER_TICKS_PER_PPU_DOT, DENDY_SCANLINES_PER_FRAME,
    NES_FRAME_DURATION_NS, NTSC_APU_FRAME_STEP_1_CPU_CYCLE, NTSC_APU_FRAME_STEP_2_CPU_CYCLE,
    NTSC_APU_FRAME_STEP_3_CPU_CYCLE, NTSC_APU_FRAME_STEP_4_CLOCK_CPU_CYCLE,
    NTSC_APU_FRAME_STEP_4_IRQ_START_CPU_CYCLE, NTSC_APU_FRAME_STEP_4_RESET_CPU_CYCLE,
    NTSC_APU_FRAME_STEP_5_CPU_CYCLE, NTSC_APU_FRAME_STEP_5_RESET_CPU_CYCLE,
    NTSC_MASTER_CLOCK_HZ_DENOMINATOR, NTSC_MASTER_CLOCK_HZ_NUMERATOR,
    NTSC_MASTER_TICKS_PER_CPU_CYCLE, NTSC_MASTER_TICKS_PER_PPU_DOT, NTSC_SCANLINES_PER_FRAME,
    PAL_APU_FRAME_STEP_1_CPU_CYCLE, PAL_APU_FRAME_STEP_2_CPU_CYCLE, PAL_APU_FRAME_STEP_3_CPU_CYCLE,
    PAL_APU_FRAME_STEP_4_CLOCK_CPU_CYCLE, PAL_APU_FRAME_STEP_4_IRQ_START_CPU_CYCLE,
    PAL_APU_FRAME_STEP_4_RESET_CPU_CYCLE, PAL_APU_FRAME_STEP_5_CPU_CYCLE,
    PAL_APU_FRAME_STEP_5_RESET_CPU_CYCLE, PAL_MASTER_CLOCK_HZ_DENOMINATOR,
    PAL_MASTER_CLOCK_HZ_NUMERATOR, PAL_MASTER_TICKS_PER_CPU_CYCLE, PAL_MASTER_TICKS_PER_PPU_DOT,
    PAL_SCANLINES_PER_FRAME, PPU_DOTS_PER_SCANLINE,
};
use anyhow::bail;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NesTiming {
    Ntsc,
    Pal,
    Dendy,
}

#[derive(Clone, Copy)]
pub(crate) struct ApuFrameCounterCycles {
    pub(crate) step_1: u64,
    pub(crate) step_2: u64,
    pub(crate) step_3: u64,
    pub(crate) step_4_irq_start: u64,
    pub(crate) step_4_clock: u64,
    pub(crate) step_4_reset: u64,
    pub(crate) step_5: u64,
    pub(crate) step_5_reset: u64,
}

const NTSC_APU_FRAME_COUNTER_CYCLES: ApuFrameCounterCycles = ApuFrameCounterCycles {
    step_1: NTSC_APU_FRAME_STEP_1_CPU_CYCLE,
    step_2: NTSC_APU_FRAME_STEP_2_CPU_CYCLE,
    step_3: NTSC_APU_FRAME_STEP_3_CPU_CYCLE,
    step_4_irq_start: NTSC_APU_FRAME_STEP_4_IRQ_START_CPU_CYCLE,
    step_4_clock: NTSC_APU_FRAME_STEP_4_CLOCK_CPU_CYCLE,
    step_4_reset: NTSC_APU_FRAME_STEP_4_RESET_CPU_CYCLE,
    step_5: NTSC_APU_FRAME_STEP_5_CPU_CYCLE,
    step_5_reset: NTSC_APU_FRAME_STEP_5_RESET_CPU_CYCLE,
};

const PAL_APU_FRAME_COUNTER_CYCLES: ApuFrameCounterCycles = ApuFrameCounterCycles {
    step_1: PAL_APU_FRAME_STEP_1_CPU_CYCLE,
    step_2: PAL_APU_FRAME_STEP_2_CPU_CYCLE,
    step_3: PAL_APU_FRAME_STEP_3_CPU_CYCLE,
    step_4_irq_start: PAL_APU_FRAME_STEP_4_IRQ_START_CPU_CYCLE,
    step_4_clock: PAL_APU_FRAME_STEP_4_CLOCK_CPU_CYCLE,
    step_4_reset: PAL_APU_FRAME_STEP_4_RESET_CPU_CYCLE,
    step_5: PAL_APU_FRAME_STEP_5_CPU_CYCLE,
    step_5_reset: PAL_APU_FRAME_STEP_5_RESET_CPU_CYCLE,
};

impl NesTiming {
    pub(crate) const fn cpu_master_divisor(self) -> u8 {
        match self {
            Self::Ntsc => NTSC_MASTER_TICKS_PER_CPU_CYCLE,
            Self::Pal => PAL_MASTER_TICKS_PER_CPU_CYCLE,
            Self::Dendy => DENDY_MASTER_TICKS_PER_CPU_CYCLE,
        }
    }

    pub(crate) const fn ppu_master_divisor(self) -> u8 {
        match self {
            Self::Ntsc => NTSC_MASTER_TICKS_PER_PPU_DOT,
            Self::Pal => PAL_MASTER_TICKS_PER_PPU_DOT,
            Self::Dendy => DENDY_MASTER_TICKS_PER_PPU_DOT,
        }
    }

    pub(crate) const fn master_clock_hz_ratio(self) -> (u64, u64) {
        match self {
            Self::Ntsc => (
                NTSC_MASTER_CLOCK_HZ_NUMERATOR,
                NTSC_MASTER_CLOCK_HZ_DENOMINATOR,
            ),
            Self::Pal => (
                PAL_MASTER_CLOCK_HZ_NUMERATOR,
                PAL_MASTER_CLOCK_HZ_DENOMINATOR,
            ),
            Self::Dendy => (
                DENDY_MASTER_CLOCK_HZ_NUMERATOR,
                DENDY_MASTER_CLOCK_HZ_DENOMINATOR,
            ),
        }
    }

    pub(crate) const fn cpu_clock_hz_ratio(self) -> (u64, u64) {
        let (numerator, denominator) = self.master_clock_hz_ratio();
        (numerator, denominator * self.cpu_master_divisor() as u64)
    }

    pub(crate) const fn scanlines_per_frame(self) -> u16 {
        match self {
            Self::Ntsc => NTSC_SCANLINES_PER_FRAME,
            Self::Pal => PAL_SCANLINES_PER_FRAME,
            Self::Dendy => DENDY_SCANLINES_PER_FRAME,
        }
    }

    pub(crate) const fn state_tag(self) -> u8 {
        match self {
            Self::Ntsc => 0,
            Self::Pal => 1,
            Self::Dendy => 2,
        }
    }

    pub(crate) fn from_state_tag(tag: u8) -> anyhow::Result<Self> {
        match tag {
            0 => Ok(Self::Ntsc),
            1 => Ok(Self::Pal),
            2 => Ok(Self::Dendy),
            _ => bail!("invalid NES timing tag in save-state: {tag}"),
        }
    }

    pub(crate) const fn pre_render_scanline(self) -> u16 {
        self.scanlines_per_frame() - 1
    }

    pub(crate) const fn vblank_start_scanline(self) -> u16 {
        match self {
            Self::Ntsc | Self::Pal => 241,
            Self::Dendy => 291,
        }
    }

    pub(crate) const fn forced_oam_refresh_start_scanline(self) -> Option<u16> {
        match self {
            Self::Pal => Some(self.vblank_start_scanline() + 24),
            Self::Ntsc | Self::Dendy => None,
        }
    }

    pub(crate) const fn odd_frame_dot_skip(self) -> bool {
        matches!(self, Self::Ntsc)
    }

    pub(crate) const fn apu_frame_counter_cycles(self) -> ApuFrameCounterCycles {
        match self {
            Self::Pal => PAL_APU_FRAME_COUNTER_CYCLES,
            Self::Ntsc | Self::Dendy => NTSC_APU_FRAME_COUNTER_CYCLES,
        }
    }

    pub(crate) const fn max_cpu_cycles_per_frame(self) -> u64 {
        let ppu_dots = self.scanlines_per_frame() as u64 * PPU_DOTS_PER_SCANLINE as u64;
        let numerator = ppu_dots * self.ppu_master_divisor() as u64;
        numerator.div_ceil(self.cpu_master_divisor() as u64)
    }

    pub(crate) const fn nominal_frame_duration_ns(self) -> u64 {
        if matches!(self, Self::Ntsc) {
            return NES_FRAME_DURATION_NS;
        }

        let (master_numerator_hz, master_denominator) = self.master_clock_hz_ratio();
        let ppu_master_ticks = self.scanlines_per_frame() as u64
            * PPU_DOTS_PER_SCANLINE as u64
            * self.ppu_master_divisor() as u64;
        (ppu_master_ticks * master_denominator * 1_000_000_000).div_ceil(master_numerator_hz)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CpuPpuClock {
    timing: NesTiming,
    master_phase: u8,
}

impl CpuPpuClock {
    pub(crate) const fn new(timing: NesTiming) -> Self {
        Self {
            timing,
            master_phase: 0,
        }
    }

    pub(crate) fn next_cpu_cycle_ppu_dots(&mut self) -> u8 {
        let elapsed = self.master_phase + self.timing.cpu_master_divisor();
        let dots = elapsed / self.timing.ppu_master_divisor();
        self.master_phase = elapsed % self.timing.ppu_master_divisor();
        dots
    }

    pub(crate) const fn master_phase(self) -> u8 {
        self.master_phase
    }

    pub(crate) fn restore_master_phase(&mut self, master_phase: u8) -> anyhow::Result<()> {
        let valid = match self.timing {
            NesTiming::Ntsc | NesTiming::Dendy => master_phase == 0,
            NesTiming::Pal => master_phase < self.timing.ppu_master_divisor(),
        };
        if !valid {
            bail!(
                "invalid NES CPU/PPU master-clock phase {master_phase} for {:?} timing",
                self.timing
            );
        }
        self.master_phase = master_phase;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CpuPpuClock, NesTiming};

    #[test]
    fn profiles_have_exact_clock_divisors_and_frame_geometry() {
        assert_eq!(NesTiming::Ntsc.cpu_master_divisor(), 12);
        assert_eq!(NesTiming::Ntsc.ppu_master_divisor(), 4);
        assert_eq!(NesTiming::Ntsc.master_clock_hz_ratio(), (236_250_000, 11));
        assert_eq!(NesTiming::Ntsc.cpu_clock_hz_ratio(), (236_250_000, 132));
        assert_eq!(NesTiming::Ntsc.scanlines_per_frame(), 262);
        assert_eq!(NesTiming::Ntsc.pre_render_scanline(), 261);
        assert_eq!(NesTiming::Ntsc.vblank_start_scanline(), 241);
        assert_eq!(NesTiming::Ntsc.forced_oam_refresh_start_scanline(), None);
        assert!(NesTiming::Ntsc.odd_frame_dot_skip());

        assert_eq!(NesTiming::Pal.cpu_master_divisor(), 16);
        assert_eq!(NesTiming::Pal.ppu_master_divisor(), 5);
        assert_eq!(NesTiming::Pal.master_clock_hz_ratio(), (53_203_425, 2));
        assert_eq!(NesTiming::Pal.cpu_clock_hz_ratio(), (53_203_425, 32));
        assert_eq!(NesTiming::Pal.scanlines_per_frame(), 312);
        assert_eq!(NesTiming::Pal.pre_render_scanline(), 311);
        assert_eq!(NesTiming::Pal.vblank_start_scanline(), 241);
        assert_eq!(
            NesTiming::Pal.forced_oam_refresh_start_scanline(),
            Some(265)
        );
        assert!(!NesTiming::Pal.odd_frame_dot_skip());

        assert_eq!(NesTiming::Dendy.cpu_master_divisor(), 15);
        assert_eq!(NesTiming::Dendy.ppu_master_divisor(), 5);
        assert_eq!(NesTiming::Dendy.master_clock_hz_ratio(), (53_203_425, 2));
        assert_eq!(NesTiming::Dendy.cpu_clock_hz_ratio(), (53_203_425, 30));
        assert_eq!(NesTiming::Dendy.scanlines_per_frame(), 312);
        assert_eq!(NesTiming::Dendy.pre_render_scanline(), 311);
        assert_eq!(NesTiming::Dendy.vblank_start_scanline(), 291);
        assert_eq!(NesTiming::Dendy.forced_oam_refresh_start_scanline(), None);
        assert!(!NesTiming::Dendy.odd_frame_dot_skip());
    }

    #[test]
    fn cpu_cycles_advance_each_region_by_its_exact_ppu_dot_ratio() {
        let mut ntsc = CpuPpuClock::new(NesTiming::Ntsc);
        let mut pal = CpuPpuClock::new(NesTiming::Pal);
        let mut dendy = CpuPpuClock::new(NesTiming::Dendy);

        assert_eq!(
            (0..10)
                .map(|_| ntsc.next_cpu_cycle_ppu_dots())
                .collect::<Vec<_>>(),
            vec![3; 10]
        );
        assert_eq!(
            (0..10)
                .map(|_| pal.next_cpu_cycle_ppu_dots())
                .collect::<Vec<_>>(),
            vec![3, 3, 3, 3, 4, 3, 3, 3, 3, 4]
        );
        assert_eq!(
            (0..10)
                .map(|_| dendy.next_cpu_cycle_ppu_dots())
                .collect::<Vec<_>>(),
            vec![3; 10]
        );
    }

    #[test]
    fn frame_guards_cover_a_whole_region_frame() {
        assert_eq!(NesTiming::Ntsc.max_cpu_cycles_per_frame(), 29_781);
        assert_eq!(NesTiming::Pal.max_cpu_cycles_per_frame(), 33_248);
        assert_eq!(NesTiming::Dendy.max_cpu_cycles_per_frame(), 35_464);
    }

    #[test]
    fn nominal_frame_duration_selects_the_machine_timing() {
        assert_eq!(NesTiming::Ntsc.nominal_frame_duration_ns(), 16_639_267);
        assert_eq!(NesTiming::Pal.nominal_frame_duration_ns(), 19_997_209);
        assert_eq!(NesTiming::Dendy.nominal_frame_duration_ns(), 19_997_209);
    }
}
