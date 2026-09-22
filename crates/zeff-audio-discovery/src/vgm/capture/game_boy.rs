use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde::Serialize;
use zeff_emu_common::audio_trace::{
    AudioTraceSource, AudioTraceTiming, GameBoyAudioTrace, GameBoyDividerResetCause,
    GameBoyResetKind, GameBoyTraceModel, GameBoyTraceOrigin, GameBoyTraceWrite,
};

use super::{GameBoyCaptureMetadata, VgmCapture, stream};

const CLOCK_HZ: u32 = 4_194_304;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameBoyVgmExport {
    pub capture: Option<VgmCapture>,
    pub unavailable: Vec<GameBoyVgmUnavailable>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GameBoyVgmUnavailable {
    pub code: &'static str,
    pub message: &'static str,
    pub first_event_index: Option<usize>,
}

const LIMITATIONS: &[&str] = &[
    "Only DMG power-on traces without divider resets, STOP, speed transitions or redirected/rejected wave RAM accesses are emitted as VGM.",
    "The synthetic preamble clears the powered-off APU and wave RAM; it is not a guest event.",
    "Native sequencer observations remain in trace JSON. VGM clocks its own sequencer and cannot reproduce native batching, oscillator phase, analog filters or every hardware quirk; external PCM equivalence is not established.",
    "VGM writes retain raw address/value order, with floor(absolute_cycle * 44100 / 4194304) waits. Writer provenance and timing controls remain in the trace.",
    "This records an executed interval, including effects and silence. Song selectors, loops, sequence/sample origins and standalone VGM preview remain unqualified.",
];

pub fn encode_game_boy(trace: &GameBoyAudioTrace, cancel: &AtomicBool) -> Result<GameBoyVgmExport> {
    validate(trace, cancel)?;
    let mut unavailable = Vec::new();
    let mut reject = |code, message, first_event_index| {
        if !unavailable
            .iter()
            .any(|row: &GameBoyVgmUnavailable| row.code == code)
        {
            unavailable.push(GameBoyVgmUnavailable {
                code,
                message,
                first_event_index,
            });
        }
    };
    if trace.chip.model == GameBoyTraceModel::Cgb {
        reject(
            "cgb_model",
            "CGB APU behavior and speed controls are not qualified for portable VGM output.",
            None,
        );
    }
    if trace.chip.reset.kind == GameBoyResetKind::PostBoot {
        reject(
            "post_boot_seed",
            "The emulator's post-boot APU/divider seed is not reconstructible by a qualified VGM reset preamble.",
            None,
        );
    }
    for (index, event) in trace.events.iter().enumerate() {
        ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
        let reason = match event.write {
            GameBoyTraceWrite::DividerReset { .. } => Some((
                "divider_reset",
                "VGM cannot encode the recorded divider reset and its APU clock effects.",
            )),
            GameBoyTraceWrite::Stop { .. } => Some((
                "stop",
                "VGM cannot encode the recorded STOP transition and stopped-clock behavior.",
            )),
            GameBoyTraceWrite::SpeedSwitch { .. } | GameBoyTraceWrite::SpeedSwitchDelay { .. } => {
                Some((
                    "speed_switch",
                    "VGM cannot encode the recorded speed switch and frozen-divider delay.",
                ))
            }
            GameBoyTraceWrite::WaveRam {
                address,
                applied_index,
                ..
            } if applied_index != Some((address - 0xff30) as u8) => Some((
                "wave_ram_access",
                "A wave RAM write was rejected or redirected by the native APU; portable VGM equivalence is unqualified.",
            )),
            _ => None,
        };
        if let Some((code, message)) = reason {
            reject(code, message, Some(index));
        }
    }
    if u128::from(trace.end_cycle) * 44_100 / u128::from(CLOCK_HZ)
        > u128::from(crate::vgm::MAX_SAMPLES)
    {
        reject(
            "duration_limit",
            "The complete native trace exceeds the VGM duration limit.",
            None,
        );
    }
    if !unavailable.is_empty() {
        return Ok(GameBoyVgmExport {
            capture: None,
            unavailable,
        });
    }

    let mut writes = trace.clone();
    writes.events.retain(|event| {
        matches!(
            event.write,
            GameBoyTraceWrite::Register { .. } | GameBoyTraceWrite::WaveRam { .. }
        )
    });
    let observed_timing_event_count = trace
        .events
        .iter()
        .filter(|event| matches!(event.write, GameBoyTraceWrite::SequencerClock { .. }))
        .count() as u32;
    let mut header = stream::header();
    stream::put_u32(&mut header, 0x80, CLOCK_HZ);
    let mut preamble = vec![0xb3, 0x16, 0];
    for address in 0x20..=0x2f {
        preamble.extend_from_slice(&[0xb3, address, 0]);
    }
    // DMG length registers accept writes even while NR52 is off.
    for address in [0x01, 0x06, 0x0b, 0x10] {
        preamble.extend_from_slice(&[0xb3, address, 0]);
    }
    let capture = stream::encode(
        &writes,
        stream::StreamConfig {
            header,
            preamble: &preamble,
            preamble_write_count: 21,
            sn76489_flags: None,
            huc6280: None,
            wonder_swan: None,
            game_boy: Some(GameBoyCaptureMetadata {
                model: "dmg",
                master_clock_hz: CLOCK_HZ,
                reset: "power_on",
                observed_timing_event_count,
            }),
            nes: None,
            limitations: LIMITATIONS,
        },
        cancel,
        |write| match *write {
            GameBoyTraceWrite::Register { address, value, .. }
            | GameBoyTraceWrite::WaveRam { address, value, .. } => {
                stream::EventCommand::three([0xb3, (address - 0xff10) as u8, value])
            }
            _ => unreachable!("validated register-only VGM projection"),
        },
    )?;
    Ok(GameBoyVgmExport {
        capture: Some(capture),
        unavailable,
    })
}

fn validate(trace: &GameBoyAudioTrace, cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
    trace.validate_complete()?;
    ensure!(
        trace.cycle_hz == CLOCK_HZ
            && trace.cycle_hz_denominator == 1
            && trace.chip.clock_hz == CLOCK_HZ,
        "Game Boy trace requires the 4194304 Hz master clock"
    );
    ensure!(
        trace.timing == AudioTraceTiming::CpuBusCycleBoundary,
        "Game Boy trace requires native CPU bus-cycle boundaries"
    );
    ensure!(
        trace.chip.model == GameBoyTraceModel::Cgb
            || (!trace.chip.dmg_compatibility && !trace.chip.reset.double_speed),
        "DMG trace cannot use CGB compatibility or double speed"
    );
    let reset = &trace.chip.reset;
    ensure!(
        !reset.double_speed,
        "Game Boy reset capture starts at normal CPU speed"
    );
    let mut expected = [0; 0x17];
    let dmg_post_boot =
        reset.kind == GameBoyResetKind::PostBoot && trace.chip.model == GameBoyTraceModel::Dmg;
    if dmg_post_boot {
        expected[1] = 0x80;
        expected[2] = 0xf3;
        expected[0x14] = 0x77;
        expected[0x15] = 0xf3;
    }
    ensure!(
        reset.registers == expected
            && reset.wave_ram == [0; 16]
            && reset.nr52 == if dmg_post_boot { 0x81 } else { 0 },
        "Game Boy trace requires the declared native reset seed"
    );
    if reset.kind == GameBoyResetKind::PowerOn {
        ensure!(
            reset.divider_counter == 0,
            "Game Boy power-on divider seed must be zero"
        );
    } else {
        let valid_divider = match (trace.chip.model, trace.chip.dmg_compatibility) {
            (GameBoyTraceModel::Dmg, _) => reset.divider_counter == 0xabc8,
            (GameBoyTraceModel::Cgb, false) => {
                matches!(
                    reset.divider_counter,
                    0x2fa4 | 0x2fc4 | 0x1ebc | 0x1e98 | 0x1e9c
                )
            }
            (GameBoyTraceModel::Cgb, true) => {
                matches!(
                    reset.divider_counter,
                    0x3780 | 0x37a0 | 0x2698 | 0x2674 | 0x2678
                )
            }
        };
        ensure!(valid_divider, "Game Boy post-boot divider seed is invalid");
    }
    let mut double_speed = false;
    for event in &trace.events {
        ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
        ensure!(
            event.pc <= u32::from(u16::MAX),
            "Game Boy trace PC exceeds the CPU address space"
        );
        match event.write {
            GameBoyTraceWrite::Register {
                address, origin, ..
            } => {
                ensure!(
                    (0xff10..=0xff26).contains(&address),
                    "Game Boy register event is outside the APU window"
                );
                validate_origin(event.pc, event.instruction_source, origin)?;
            }
            GameBoyTraceWrite::WaveRam {
                address,
                applied_index,
                origin,
                ..
            } => {
                ensure!(
                    (0xff30..=0xff3f).contains(&address)
                        && applied_index.is_none_or(|index| index < 16),
                    "Game Boy wave RAM event has an invalid address or applied index"
                );
                validate_origin(event.pc, event.instruction_source, origin)?;
            }
            GameBoyTraceWrite::SequencerClock { primary, secondary } => {
                ensure!(
                    primary != 0 || secondary != 0,
                    "Game Boy sequencer event is empty"
                );
                ensure!(
                    event.pc == 0 && event.instruction_source == AudioTraceSource::Unknown,
                    "Game Boy sequencer event must not invent an instruction writer"
                );
            }
            GameBoyTraceWrite::SpeedSwitch { .. } | GameBoyTraceWrite::SpeedSwitchDelay { .. } => {
                ensure!(
                    trace.chip.model == GameBoyTraceModel::Cgb && !trace.chip.dmg_compatibility,
                    "Game Boy speed event requires native CGB mode"
                );
                if let GameBoyTraceWrite::SpeedSwitch { double_speed: next } = event.write {
                    ensure!(
                        next != double_speed,
                        "Game Boy speed event does not change speed"
                    );
                    double_speed = next;
                }
                if let GameBoyTraceWrite::SpeedSwitchDelay { cycles } = event.write {
                    ensure!(
                        matches!(cycles, 65_538 | 65_544)
                            && event
                                .cycle
                                .checked_add(cycles)
                                .is_some_and(|end| end <= trace.end_cycle),
                        "Game Boy speed-switch delay is invalid or extends beyond the trace"
                    );
                }
            }
            GameBoyTraceWrite::DividerReset {
                cause,
                divider_counter,
                apu_bit,
            } => {
                ensure!(
                    apu_bit == (divider_counter & (1 << if double_speed { 13 } else { 12 }) != 0),
                    "Game Boy divider reset has an inconsistent APU clock bit"
                );
                ensure!(
                    cause != GameBoyDividerResetCause::SpeedSwitch
                        || (trace.chip.model == GameBoyTraceModel::Cgb
                            && !trace.chip.dmg_compatibility),
                    "Game Boy speed-switch divider reset requires native CGB mode"
                );
            }
            GameBoyTraceWrite::Stop { .. } => {}
            GameBoyTraceWrite::NativeBatch { .. }
            | GameBoyTraceWrite::NativeDividerPhase { .. }
            | GameBoyTraceWrite::PcmDrain { .. }
            | GameBoyTraceWrite::NativeOutputChange { .. } => {
                ensure!(
                    trace.chip.native_replay.is_some()
                        && event.pc == 0
                        && event.instruction_source == AudioTraceSource::Unknown,
                    "native Game Boy output events require an explicit contract and no instruction writer"
                );
            }
        }
    }
    Ok(())
}

fn validate_origin(pc: u32, source: AudioTraceSource, origin: GameBoyTraceOrigin) -> Result<()> {
    ensure!(
        origin != GameBoyTraceOrigin::CpuInterrupt
            || (pc == 0 && source == AudioTraceSource::Unknown),
        "Game Boy interrupt entry must not invent an instruction writer"
    );
    Ok(())
}
