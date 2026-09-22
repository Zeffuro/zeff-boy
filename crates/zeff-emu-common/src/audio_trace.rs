//! Bounded evidence of writes applied to an emulated audio chip.

mod huc6280;
pub use huc6280::*;
mod wonderswan;
pub use wonderswan::*;
mod game_boy;
pub use game_boy::*;
mod nes;
pub use nes::*;

use crate::time::ClockRate;

pub const MAX_AUDIO_TRACE_EVENTS: usize = 262_144;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum AudioTraceTiming {
    InstructionBoundary,
    IoWriteCompletion,
    MemoryWriteCompletion,
    BusServiceBoundary,
    CpuBusCycleBoundary,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum AudioTraceStart {
    Reset,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Sn76489ZeroPeriod {
    ConstantHigh,
    Period1024,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Sn76489Tone2NoiseClock {
    HalfPeriod,
    RisingEdge,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sn76489ResetState {
    pub tone_periods: [u16; 3],
    pub volumes: [u8; 4],
    pub noise_control: u8,
    pub stereo_control: u8,
    pub latched_register: u8,
    pub noise_lfsr: u16,
    pub tone_output_high: [bool; 3],
    pub tone_clocks_remaining: [u32; 3],
    pub noise_clocks_remaining: u32,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sn76489TraceChip {
    pub clock_hz: u32,
    pub feedback_mask: u16,
    pub shift_register_width: u8,
    pub zero_period: Sn76489ZeroPeriod,
    pub period_one_constant_high: bool,
    /// Input clocks per tone counter tick; a full wave requires two periods.
    pub tone_counter_clock_divider: u8,
    pub noise_tone2_clock: Sn76489Tone2NoiseClock,
    pub noise_output_high_when_lfsr_bit_zero: bool,
    pub stereo: bool,
    pub reset: Sn76489ResetState,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum AudioTraceSource {
    /// Offset into the original cartridge input, including any copier header.
    CartridgeRom {
        offset: u64,
        bit_reversed: bool,
    },
    BootRom {
        offset: u64,
    },
    WorkRam {
        offset: u32,
    },
    CartridgeRam {
        offset: u32,
    },
    Unmapped,
    Unknown,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum AudioTraceWrite {
    Sn76489 { port: u8, value: u8 },
    GameGearStereo { port: u8, value: u8 },
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioTraceEvent<W = AudioTraceWrite> {
    pub cycle: u64,
    pub pc: u32,
    pub instruction_source: AudioTraceSource,
    pub write: W,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum AudioTraceInvalidation {
    Reset,
    StateRestore,
    ExternalMutation,
    ClockChanged,
    NonMonotonicCycles,
    ClockOverflow,
    ExecutionFault,
}

pub trait AudioTraceChip {
    fn clock_numerator_hz(&self) -> u64;
    fn clock_denominator(&self) -> u32;
}

impl AudioTraceChip for Sn76489TraceChip {
    fn clock_numerator_hz(&self) -> u64 {
        u64::from(self.clock_hz)
    }

    fn clock_denominator(&self) -> u32 {
        1
    }
}

#[cfg(feature = "serde")]
fn is_one(value: &u32) -> bool {
    *value == 1
}

#[cfg(feature = "serde")]
fn one() -> u32 {
    1
}

pub type AudioTrace = ChipAudioTrace<Sn76489TraceChip, AudioTraceWrite>;
pub type AudioTraceRecorder = ChipAudioTraceRecorder<Sn76489TraceChip, AudioTraceWrite>;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChipAudioTrace<C, W> {
    pub generation: u64,
    pub cycle_hz: u32,
    #[cfg_attr(
        feature = "serde",
        serde(default = "one", skip_serializing_if = "is_one")
    )]
    pub cycle_hz_denominator: u32,
    pub chip: C,
    pub timing: AudioTraceTiming,
    pub start: AudioTraceStart,
    pub end_cycle: u64,
    pub events: Vec<AudioTraceEvent<W>>,
    pub dropped_events: u64,
    pub invalidated: Option<AudioTraceInvalidation>,
}

impl<C: AudioTraceChip, W> ChipAudioTrace<C, W> {
    pub fn validate_complete(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.events.len() <= MAX_AUDIO_TRACE_EVENTS,
            "audio trace exceeds the event limit"
        );
        anyhow::ensure!(
            self.invalidated.is_none(),
            "audio trace invalidated: {:?}",
            self.invalidated
        );
        anyhow::ensure!(
            self.dropped_events == 0,
            "audio trace lost {} events",
            self.dropped_events
        );
        anyhow::ensure!(
            self.cycle_hz != 0
                && self.cycle_hz_denominator != 0
                && self.chip.clock_numerator_hz() != 0
                && self.chip.clock_denominator() != 0,
            "audio trace has a zero clock"
        );
        let mut previous = 0;
        for event in &self.events {
            anyhow::ensure!(
                event.cycle >= previous && event.cycle <= self.end_cycle,
                "audio trace contains invalid event timing"
            );
            previous = event.cycle;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ChipAudioTraceRecorder<C, W> {
    generation: u64,
    trace: Option<ChipAudioTrace<C, W>>,
    max_events: usize,
}

impl<C, W> Default for ChipAudioTraceRecorder<C, W> {
    fn default() -> Self {
        Self {
            generation: 0,
            trace: None,
            max_events: 0,
        }
    }
}

impl<C: AudioTraceChip, W> ChipAudioTraceRecorder<C, W> {
    pub fn prepare(
        &self,
        max_events: usize,
        cycle_hz: u32,
        chip: C,
        timing: AudioTraceTiming,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(cycle_hz != 0, "audio trace clock must be nonzero");
        self.prepare_with_clock(
            max_events,
            ClockRate::from_hz(u64::from(cycle_hz)),
            chip,
            timing,
        )
    }

    pub fn prepare_with_clock(
        &self,
        max_events: usize,
        clock: ClockRate,
        chip: C,
        timing: AudioTraceTiming,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=MAX_AUDIO_TRACE_EVENTS).contains(&max_events),
            "audio trace capacity must be between 1 and {MAX_AUDIO_TRACE_EVENTS}"
        );
        anyhow::ensure!(
            chip.clock_numerator_hz() != 0 && chip.clock_denominator() != 0,
            "audio trace clock must be nonzero"
        );
        let mut events = Vec::new();
        events.try_reserve_exact(max_events)?;
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("audio trace generation exhausted"))?;
        Ok(Self {
            generation,
            trace: Some(ChipAudioTrace {
                generation,
                cycle_hz: u32::try_from(clock.numerator_hz())?,
                cycle_hz_denominator: u32::try_from(clock.denominator())?,
                chip,
                timing,
                start: AudioTraceStart::Reset,
                end_cycle: 0,
                events,
                dropped_events: 0,
                invalidated: None,
            }),
            max_events,
        })
    }

    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.trace
            .as_ref()
            .is_some_and(|trace| trace.invalidated.is_none())
    }

    pub fn record(&mut self, event: AudioTraceEvent<W>) {
        let Some(trace) = self
            .trace
            .as_mut()
            .filter(|trace| trace.invalidated.is_none())
        else {
            return;
        };
        if trace
            .events
            .last()
            .is_some_and(|last| event.cycle < last.cycle)
        {
            trace.invalidated = Some(AudioTraceInvalidation::NonMonotonicCycles);
        } else if trace.events.len() == self.max_events {
            trace.dropped_events = trace.dropped_events.saturating_add(1);
        } else {
            trace.events.push(event);
        }
    }

    pub fn invalidate(&mut self, reason: AudioTraceInvalidation) {
        if let Some(trace) = &mut self.trace {
            trace.invalidated.get_or_insert(reason);
        }
    }

    pub fn finish(&mut self, end_cycle: u64) -> Option<ChipAudioTrace<C, W>> {
        let mut trace = self.trace.take()?;
        trace.end_cycle = end_cycle;
        Some(trace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorder() -> AudioTraceRecorder {
        AudioTraceRecorder::default()
            .prepare(
                2,
                3_579_545,
                Sn76489TraceChip {
                    clock_hz: 3_579_545,
                    feedback_mask: 3,
                    shift_register_width: 15,
                    zero_period: Sn76489ZeroPeriod::Period1024,
                    period_one_constant_high: false,
                    tone_counter_clock_divider: 16,
                    noise_tone2_clock: Sn76489Tone2NoiseClock::RisingEdge,
                    noise_output_high_when_lfsr_bit_zero: false,
                    stereo: false,
                    reset: Sn76489ResetState {
                        tone_periods: [0; 3],
                        volumes: [15; 4],
                        noise_control: 0,
                        stereo_control: 255,
                        latched_register: 0,
                        noise_lfsr: 0x4000,
                        tone_output_high: [false; 3],
                        tone_clocks_remaining: [16; 3],
                        noise_clocks_remaining: 16,
                    },
                },
                AudioTraceTiming::IoWriteCompletion,
            )
            .unwrap()
    }

    fn event(cycle: u64, value: u8) -> AudioTraceEvent {
        AudioTraceEvent {
            cycle,
            pc: 0,
            instruction_source: AudioTraceSource::Unknown,
            write: AudioTraceWrite::Sn76489 { port: 0xE0, value },
        }
    }

    #[test]
    fn inactive_recording_has_no_event_storage() {
        let mut recorder = AudioTraceRecorder::default();
        recorder.record(event(0, 0x9F));
        assert!(recorder.trace.is_none());
        assert!(recorder.finish(1).is_none());
    }

    #[test]
    fn same_cycle_order_is_retained_and_backward_cycles_invalidate() {
        let mut recorder = recorder();
        recorder.record(event(10, 0x84));
        recorder.record(event(10, 0x01));
        let trace = recorder.finish(10).unwrap();
        trace.validate_complete().unwrap();
        assert_eq!(trace.events, [event(10, 0x84), event(10, 0x01)]);

        let mut recorder = self::recorder();
        recorder.record(event(10, 0x84));
        recorder.record(event(9, 0x01));
        let trace = recorder.finish(20).unwrap();
        assert_eq!(
            trace.invalidated,
            Some(AudioTraceInvalidation::NonMonotonicCycles)
        );
        assert!(trace.validate_complete().is_err());
    }

    #[test]
    fn validation_rejects_past_end_events_and_checks_length_first() {
        let mut recorder = recorder();
        recorder.record(event(10, 0x9F));
        let mut trace = recorder.finish(9).unwrap();
        assert!(trace.validate_complete().is_err());
        trace.events.resize(MAX_AUDIO_TRACE_EVENTS + 1, event(0, 0));
        trace.invalidated = Some(AudioTraceInvalidation::Reset);
        assert_eq!(
            trace.validate_complete().unwrap_err().to_string(),
            "audio trace exceeds the event limit"
        );
    }

    #[test]
    fn rational_clock_is_retained_and_invalid_denominators_are_rejected() {
        let chip = Huc6280TraceChip {
            clock_hz_numerator: 315_000_000,
            clock_hz_denominator: 88,
            master_clock_divisor: 6,
            internal_master_clock_divisor: 3,
            revision: Huc6280TraceRevision::HuC6280A,
            reset: Huc6280ResetState::default(),
        };
        let mut recorder = Huc6280AudioTraceRecorder::default()
            .prepare_with_clock(
                1,
                ClockRate::from_ratio(1_890_000_000, 88),
                chip,
                AudioTraceTiming::MemoryWriteCompletion,
            )
            .unwrap();
        let mut trace = recorder.finish(100).unwrap();
        assert_eq!(
            (trace.cycle_hz, trace.cycle_hz_denominator),
            (236_250_000, 11)
        );
        trace.validate_complete().unwrap();
        trace.cycle_hz_denominator = 0;
        assert!(trace.validate_complete().is_err());
        trace.cycle_hz_denominator = 11;
        trace.chip.clock_hz_denominator = 0;
        assert!(trace.validate_complete().is_err());
        assert!(
            recorder
                .prepare_with_clock(
                    1,
                    ClockRate::from_hz(1),
                    trace.chip,
                    AudioTraceTiming::MemoryWriteCompletion,
                )
                .is_err()
        );
    }
}
