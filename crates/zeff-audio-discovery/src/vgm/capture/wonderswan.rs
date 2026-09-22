use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, bail, ensure};
use zeff_emu_common::audio_trace::{
    AudioTraceSource, AudioTraceStart, AudioTraceTiming, WonderSwanAudioTrace,
    WonderSwanResetState, WonderSwanTraceOrigin, WonderSwanTraceWrite,
};

use super::{
    VgmCapture, WonderSwanCaptureMetadata,
    stream::{self, EventCommand, StreamConfig, put_u32},
};

const MASTER_CLOCK_HZ: u32 = 3_072_000;
const WAVE_RAM_BYTES: usize = 0x4000;
const PORT_START: u16 = 0x80;
const PORT_END: u16 = 0x9b;

const LIMITATIONS: &[&str] = &[
    "The trace begins at emulator reset; the preamble reconstructs ordinary sound ports and the complete 16 KiB wave RAM, but is not a guest write record.",
    "VGM waits use floor(absolute_cycle * 44100 / 3072000); source PCs and write provenance remain in the trace.",
    "Timestamps preserve native bus-service ordering: CPU and General DMA writes precede their cycle advance; Sound DMA transfers are batched after the service advance, not assigned inferred physical transfer times.",
    "VGM cannot represent applied WonderSwan Color HyperVoice ports or Sound DMA targeted at HyperVoice; these intervals are rejected before a complete VGM is produced.",
    "VGM does not implement the sound-test fast-sweep mode, so traces that enable it are rejected before a complete VGM is produced.",
    "VGM cannot restore oscillator, sweep, noise, or HyperVoice phase exactly, including hidden player state after the reset preamble.",
    "This capture contains applied APU register and wave-RAM writes only; it does not establish song identity, loop metadata, or PCM equivalence.",
];

pub fn encode(trace: &WonderSwanAudioTrace, cancel: &AtomicBool) -> Result<VgmCapture> {
    ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
    validate_trace(trace, cancel)?;
    let mut header = stream::header();
    put_u32(&mut header, 0xc0, MASTER_CLOCK_HZ);
    let preamble = reset_preamble();
    stream::encode(
        trace,
        StreamConfig {
            header,
            preamble: &preamble,
            preamble_write_count: WAVE_RAM_BYTES + usize::from(PORT_END - PORT_START + 1),
            sn76489_flags: None,
            huc6280: None,
            wonder_swan: Some(WonderSwanCaptureMetadata {
                model: if trace.chip.color {
                    "wonderswan_color"
                } else {
                    "wonderswan"
                },
                master_clock_hz: MASTER_CLOCK_HZ,
                wave_ram_bytes: WAVE_RAM_BYTES as u32,
                canonical_reset: "all ordinary sound ports and the 16 KiB wave RAM are zero",
                hyper_voice: "unsupported; applied color HyperVoice I/O and HyperVoice-target Sound DMA reject capture; monochrome color-only writes are ignored",
                sound_test: "fast-sweep mode unsupported; writes with bit 1 set reject capture",
            }),
            limitations: LIMITATIONS,
            game_boy: None,
            nes: None,
        },
        cancel,
        |write| match *write {
            WonderSwanTraceWrite::Register { port, value, .. } => {
                EventCommand::three([0xbc, (port - PORT_START) as u8, value])
            }
            WonderSwanTraceWrite::WaveRam { address, value, .. } => {
                EventCommand::four([0xc6, (address >> 8) as u8, address as u8, value])
            }
        },
    )
}

fn validate_trace(trace: &WonderSwanAudioTrace, cancel: &AtomicBool) -> Result<()> {
    trace.validate_complete()?;
    ensure!(
        matches!(trace.start, AudioTraceStart::Reset),
        "VGM capture requires a reset-to-end audio trace"
    );
    ensure!(
        trace.timing == AudioTraceTiming::BusServiceBoundary,
        "WonderSwan VGM capture requires bus-service-boundary timing"
    );
    ensure!(
        trace.cycle_hz == MASTER_CLOCK_HZ && trace.cycle_hz_denominator == 1,
        "WonderSwan VGM capture requires the 3072000 Hz master clock"
    );
    ensure!(
        trace.chip.clock_hz == MASTER_CLOCK_HZ,
        "WonderSwan APU clock does not match the VGM master clock"
    );
    ensure!(
        trace.chip.reset == WonderSwanResetState::default(),
        "WonderSwan VGM capture requires the canonical emulator reset state"
    );
    for event in &trace.events {
        ensure!(!cancel.load(Ordering::Relaxed), "VGM capture cancelled");
        ensure!(
            event.pc <= 0x0f_ffff,
            "WonderSwan trace event PC is outside the native address space"
        );
        let origin = match event.write {
            WonderSwanTraceWrite::Register { origin, .. }
            | WonderSwanTraceWrite::WaveRam { origin, .. } => origin,
        };
        ensure!(
            !matches!(
                origin,
                WonderSwanTraceOrigin::CpuInterrupt | WonderSwanTraceOrigin::SoundDma
            ) || (event.pc == 0 && event.instruction_source == AudioTraceSource::Unknown),
            "WonderSwan interrupt and Sound DMA events require unknown provenance"
        );
        match event.write {
            WonderSwanTraceWrite::Register {
                port,
                value,
                origin,
            } => {
                if origin == WonderSwanTraceOrigin::SoundDma && port == 0x69 {
                    bail!("WonderSwan Sound DMA targeted HyperVoice is not representable by VGM");
                }
                if (0x64..=0x6b).contains(&port) {
                    bail!("WonderSwan HyperVoice I/O is not representable by VGM");
                }
                ensure!(
                    (PORT_START..=0x95).contains(&port),
                    "WonderSwan trace register write is outside the writable ordinary sound window"
                );
                ensure!(
                    origin != WonderSwanTraceOrigin::SoundDma || port == 0x89,
                    "WonderSwan Sound DMA write does not target the ordinary voice-volume port"
                );
                ensure!(
                    origin != WonderSwanTraceOrigin::SoundDma || trace.chip.color,
                    "WonderSwan Sound DMA cannot produce an event on a monochrome model"
                );
                ensure!(
                    origin != WonderSwanTraceOrigin::GeneralDma,
                    "WonderSwan General DMA cannot produce an ordinary sound-register write"
                );
                ensure!(
                    port != 0x95 || value & 0x02 == 0,
                    "WonderSwan sound-test fast-sweep mode is not representable by VGM"
                );
            }
            WonderSwanTraceWrite::WaveRam {
                address, origin, ..
            } => {
                ensure!(
                    usize::from(address) < WAVE_RAM_BYTES,
                    "WonderSwan trace wave-RAM write is outside the VGM address window"
                );
                ensure!(
                    origin != WonderSwanTraceOrigin::SoundDma,
                    "WonderSwan Sound DMA cannot produce a wave-RAM write"
                );
                ensure!(
                    origin != WonderSwanTraceOrigin::GeneralDma || trace.chip.color,
                    "WonderSwan General DMA cannot produce an event on a monochrome model"
                );
            }
        }
    }
    Ok(())
}

fn reset_preamble() -> Vec<u8> {
    let mut preamble =
        Vec::with_capacity(WAVE_RAM_BYTES * 4 + usize::from(PORT_END - PORT_START + 1) * 3);
    for address in 0..WAVE_RAM_BYTES as u16 {
        preamble.extend_from_slice(&[0xc6, (address >> 8) as u8, address as u8, 0]);
    }
    for port in PORT_START..=PORT_END {
        preamble.extend_from_slice(&[0xbc, (port - PORT_START) as u8, 0]);
    }
    preamble
}
