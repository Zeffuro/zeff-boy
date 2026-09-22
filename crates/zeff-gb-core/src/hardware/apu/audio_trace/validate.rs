use anyhow::{Result, bail, ensure};
use zeff_emu_common::audio_trace::{
    AudioTraceSource, AudioTraceStart, AudioTraceTiming, GameBoyAudioTrace, GameBoyResetKind,
    GameBoyTraceChip, GameBoyTraceModel, GameBoyTraceOrigin, GameBoyTraceWrite as Write,
};

use super::Apu;

const CLOCK: u64 = 4_194_304;
const MAX_UNDRAINED_CYCLES: u64 = CLOCK;
const MAX_TRACE_CYCLES: u64 = CLOCK * 7200;

pub(super) fn new_apu(chip: &GameBoyTraceChip, rate: u32) -> Result<Apu> {
    let native = chip.native_replay.as_ref().ok_or_else(|| {
        anyhow::anyhow!("legacy Game Boy trace lacks native batch and output-drain records")
    })?;
    ensure!(
        native.version == 1
            && (8000..=192_000).contains(&native.sample_rate)
            && (8000..=192_000).contains(&rate),
        "unsupported Game Boy native output contract"
    );
    let cgb = chip.model == GameBoyTraceModel::Cgb;
    ensure!(
        chip.clock_hz == CLOCK as u32
            && !chip.reset.double_speed
            && (cgb || !chip.dmg_compatibility),
        "unsupported Game Boy native model or clock"
    );
    let valid_divider = match (chip.reset.kind, cgb, chip.dmg_compatibility) {
        (GameBoyResetKind::PowerOn, _, _) => chip.reset.divider_counter == 0,
        (GameBoyResetKind::PostBoot, false, _) => {
            chip.reset.divider_counter.wrapping_add(4) == 0xabcc
        }
        (GameBoyResetKind::PostBoot, true, false) => matches!(
            chip.reset.divider_counter.wrapping_add(4),
            0x2fa8 | 0x2fc8 | 0x1ec0 | 0x1e9c | 0x1ea0
        ),
        (GameBoyResetKind::PostBoot, true, true) => matches!(
            chip.reset.divider_counter.wrapping_add(4),
            0x3784 | 0x37a4 | 0x269c | 0x2678 | 0x267c
        ),
    };
    ensure!(valid_divider, "unsupported Game Boy native divider seed");
    let mut apu = Apu::new();
    apu.set_cgb_hardware(cgb);
    apu.set_sample_rate(rate);
    if !cgb && chip.reset.kind == GameBoyResetKind::PostBoot {
        apu.apply_dmg_post_boot_io();
    }
    ensure!(
        apu.regs_snapshot() == chip.reset.registers
            && apu.wave_ram_snapshot() == chip.reset.wave_ram
            && apu.nr52_raw() == chip.reset.nr52,
        "unsupported Game Boy native APU reset seed"
    );
    Ok(apu)
}

#[derive(Default)]
struct OutputClock {
    cycles: u64,
    consumed: u64,
}

impl OutputClock {
    fn drain(&mut self, rate: u32) -> u64 {
        // BlipBuf's factor and half-clock initial phase are exact at this power-of-two input rate.
        let total =
            ((u128::from(self.cycles) * 2 + 1) * u128::from(rate) / (2 * u128::from(CLOCK))) as u64;
        let frames = total - self.consumed;
        self.consumed = total;
        frames
    }
}

pub(super) fn schedule(trace: &GameBoyAudioTrace, rate: u32) -> Result<Vec<u64>> {
    trace.validate_complete()?;
    ensure!(
        trace.start == AudioTraceStart::Reset
            && trace.timing == AudioTraceTiming::CpuBusCycleBoundary
            && trace.cycle_hz == CLOCK as u32
            && trace.cycle_hz_denominator == 1
            && trace.end_cycle <= MAX_TRACE_CYCLES,
        "unsupported Game Boy trace timeline"
    );
    let native = trace.chip.native_replay.as_ref().expect("validated seed");
    let cgb = trace.chip.model == GameBoyTraceModel::Cgb;
    let mut source_clock = OutputClock::default();
    let mut output_clock = OutputClock::default();
    let mut powered = trace.chip.reset.nr52 & 0x80 != 0;
    let mut cursor = 0u64;
    let mut drained_at = 0;
    let mut stopped = false;
    let mut double_speed = false;
    let mut phase_pending = None;
    let mut drains = Vec::new();
    for event in &trace.events {
        ensure!(
            event.pc <= u32::from(u16::MAX),
            "Game Boy trace PC exceeds its address space"
        );
        if event.cycle != cursor {
            ensure!(
                stopped && !cgb && event.cycle > cursor,
                "Game Boy native batches leave an unexplained clock gap"
            );
            cursor = event.cycle;
        }
        ensure!(
            cursor.saturating_sub(drained_at) <= MAX_UNDRAINED_CYCLES,
            "Game Boy output-drain interval exceeds one second"
        );
        if phase_pending.is_some() {
            ensure!(
                matches!(event.write, Write::NativeDividerPhase { .. }),
                "missing Game Boy NR52 divider outcome"
            );
        }
        let autonomous = match event.write {
            Write::Register { origin, .. } | Write::WaveRam { origin, .. } => {
                origin == GameBoyTraceOrigin::CpuInterrupt
            }
            Write::NativeBatch { .. }
            | Write::NativeDividerPhase { .. }
            | Write::PcmDrain { .. }
            | Write::NativeOutputChange { .. }
            | Write::SequencerClock { .. } => true,
            _ => false,
        };
        ensure!(
            !autonomous || (event.pc == 0 && event.instruction_source == AudioTraceSource::Unknown),
            "autonomous Game Boy event claims instruction provenance"
        );
        match event.write {
            Write::Register { address, value, .. } => {
                ensure!(
                    (0xff10..=0xff26).contains(&address),
                    "invalid Game Boy APU register"
                );
                if address == 0xff26 {
                    let next = value & 0x80 != 0;
                    phase_pending = Some(!powered && next);
                    powered = next;
                    if !powered {
                        source_clock = OutputClock::default();
                        output_clock = OutputClock::default();
                    }
                }
            }
            Write::WaveRam {
                address,
                applied_index,
                ..
            } => {
                ensure!(
                    (0xff30..=0xff3f).contains(&address)
                        && applied_index.is_none_or(|index| index < 16),
                    "invalid Game Boy wave-RAM event"
                );
            }
            Write::NativeDividerPhase { skip_next } => {
                let can_skip = phase_pending
                    .take()
                    .ok_or_else(|| anyhow::anyhow!("orphan Game Boy divider outcome"))?;
                ensure!(
                    !skip_next || can_skip,
                    "invalid Game Boy NR52 divider outcome"
                );
            }
            Write::NativeBatch {
                cycles,
                repetitions,
            } => {
                ensure!(
                    cycles <= 65_544
                        && repetitions > 0
                        && (cycles != 0 || repetitions == 1)
                        && (!stopped || cgb),
                    "invalid Game Boy native APU batch"
                );
                let elapsed = u64::from(cycles) * u64::from(repetitions);
                cursor = cursor
                    .checked_add(elapsed)
                    .ok_or_else(|| anyhow::anyhow!("Game Boy batch clock overflow"))?;
                ensure!(
                    cursor <= trace.end_cycle && cursor - drained_at <= MAX_UNDRAINED_CYCLES,
                    "Game Boy batch exceeds its native output interval"
                );
                if powered {
                    source_clock.cycles += elapsed;
                    output_clock.cycles += elapsed;
                }
            }
            Write::PcmDrain { frames } => {
                ensure!(
                    source_clock.drain(native.sample_rate) == u64::from(frames),
                    "Game Boy recorded drain frame count differs from its native clock"
                );
                drains.push(output_clock.drain(rate));
                drained_at = cursor;
            }
            Write::Stop { entered } => {
                ensure!(entered != stopped, "repeated Game Boy STOP transition");
                stopped = entered;
            }
            Write::SpeedSwitch { double_speed: next } => {
                ensure!(
                    cgb && !trace.chip.dmg_compatibility && next != double_speed,
                    "invalid Game Boy speed transition"
                );
                double_speed = next;
            }
            Write::SpeedSwitchDelay { cycles } => ensure!(
                cgb && cycles == if double_speed { 65_544 } else { 65_538 },
                "invalid Game Boy speed-switch delay"
            ),
            Write::DividerReset {
                divider_counter,
                apu_bit,
                ..
            } => {
                let mask = 1 << if double_speed { 13 } else { 12 };
                ensure!(
                    apu_bit == (divider_counter & mask != 0),
                    "invalid Game Boy divider-reset phase"
                );
            }
            Write::SequencerClock { primary, secondary } => ensure!(
                primary <= 1 && secondary <= 1 && primary + secondary > 0,
                "invalid Game Boy sequencer clock batch"
            ),
            Write::NativeOutputChange { .. } => {
                bail!("Game Boy trace changes native output settings")
            }
        }
    }
    ensure!(
        phase_pending.is_none()
            && cursor == trace.end_cycle
            && drained_at == trace.end_cycle
            && !drains.is_empty()
            && matches!(
                trace.events.last().map(|event| event.write),
                Some(Write::PcmDrain { .. })
            ),
        "Game Boy trace lacks a complete terminal output drain"
    );
    Ok(drains)
}
