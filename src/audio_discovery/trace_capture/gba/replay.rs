use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};
use serde_json::{Value, json};
use zeff_emu_common::audio_trace::{
    AudioTraceStart, AudioTraceTiming, GBA_AUDIO_TRACE_CLOCK_HZ, GbaAudioTrace,
    GbaAudioTraceOrigin, GbaAudioTraceReset, GbaAudioTraceWrite, GbaDirectSoundFifo,
};

mod provenance;

pub(super) struct Feed {
    pub samples: [Vec<u8>; 2],
    pub metadata: Value,
}

#[derive(Clone, Copy)]
struct Byte {
    value: i8,
    event: usize,
    lane: usize,
}

pub(super) fn replay(trace: &GbaAudioTrace, source: &[u8], cancel: &AtomicBool) -> Result<Feed> {
    trace.validate_complete()?;
    ensure!(
        trace.cycle_hz == GBA_AUDIO_TRACE_CLOCK_HZ
            && trace.cycle_hz_denominator == 1
            && trace.chip.clock_hz == GBA_AUDIO_TRACE_CLOCK_HZ
            && trace.chip.reset == GbaAudioTraceReset::PostBiosV1
            && const_hex::encode(trace.chip.source_sha256) == zeff_firmware::sha256_hex(source)
            && trace.start == AudioTraceStart::Reset
            && trace.timing == AudioTraceTiming::BusServiceBoundary,
        "unsupported GBA FIFO trace contract"
    );
    let cartridge = zeff_gba_core::hardware::cartridge::Cartridge::load(source)?;
    let mut queues: [VecDeque<Byte>; 2] = std::array::from_fn(|_| VecDeque::new());
    let mut current = [0i8; 2];
    let mut samples: [Vec<u8>; 2] = std::array::from_fn(|_| Vec::new());
    let mut pops = Vec::new();
    let mut evicted = [0u64; 2];
    let mut terminal = false;
    let mut resets = VecDeque::new();
    let mut word_tail = None;
    for (index, event) in trace.events.iter().enumerate() {
        super::super::check_cancel(cancel)?;
        ensure!(!terminal, "GBA FIFO events follow the terminal snapshot");
        if word_tail.is_some() {
            ensure!(
                matches!(event.write, GbaAudioTraceWrite::FifoHalfword { access, .. }
                if access.width == 4 && access.halfword_lane == 2),
                "GBA FIFO word is missing its second halfword"
            );
        }
        if !matches!(event.write, GbaAudioTraceWrite::FifoReset { .. }) {
            ensure!(resets.is_empty(), "GBA FIFO reset event is missing");
        }
        let origin = match event.write {
            GbaAudioTraceWrite::Control { origin, .. }
            | GbaAudioTraceWrite::FifoHalfword { origin, .. }
            | GbaAudioTraceWrite::FifoReset { origin, .. }
            | GbaAudioTraceWrite::FifoPop { origin, .. }
            | GbaAudioTraceWrite::Terminal { origin, .. } => origin,
        };
        provenance::origin(event, origin, &cartridge)?;
        match event.write {
            GbaAudioTraceWrite::Control {
                address,
                raw_value,
                io_value,
                access,
                ..
            } => {
                ensure!(
                    matches!(
                        origin,
                        GbaAudioTraceOrigin::Cpu { .. }
                            | GbaAudioTraceOrigin::CpuNonInstruction
                            | GbaAudioTraceOrigin::Dma(_)
                    ),
                    "GBA FIFO control has no supported producer"
                );
                ensure!(
                    matches!(
                        address,
                        0x80 | 0x82 | 0x84 | 0x88 | 0x100 | 0x102 | 0x104 | 0x106
                    ) && provenance::applied_address(access)? == 0x0400_0000 + u32::from(address),
                    "invalid GBA FIFO control event"
                );
                provenance::feed(origin, access, raw_value)?;
                if let GbaAudioTraceOrigin::Dma(dma) = origin {
                    ensure!(
                        dma.kind == zeff_emu_common::audio_trace::GbaAudioTraceDmaKind::Normal,
                        "sound FIFO DMA cannot write a control register"
                    );
                }
                let mask = match address {
                    0x82 => 0x770f,
                    0x84 => 0x0080,
                    0x88 => 0xc3fe,
                    _ => 0xffff,
                };
                ensure!(
                    io_value == raw_value & mask,
                    "GBA sound control mask disagrees with its write"
                );
                if address == 0x82 {
                    for (fifo, bit) in [(GbaDirectSoundFifo::A, 11), (GbaDirectSoundFifo::B, 15)] {
                        if raw_value & (1 << bit) != 0 {
                            resets.push_back((fifo, access, origin, event.cycle));
                        }
                    }
                }
            }
            GbaAudioTraceWrite::FifoHalfword {
                fifo,
                value,
                access,
                ..
            } => {
                if access.width == 4 {
                    let pair = (fifo, access.address, origin, event.cycle);
                    if access.halfword_lane == 0 {
                        word_tail = Some(pair);
                    } else {
                        ensure!(
                            word_tail.take() == Some(pair),
                            "GBA FIFO word halves disagree"
                        );
                    }
                }
                let fifo = fifo_index(fifo);
                let address = provenance::applied_address(access)?;
                let base = 0x0400_00a0 + fifo as u32 * 4;
                ensure!(
                    address == base || address == base + 2,
                    "invalid GBA FIFO destination"
                );
                provenance::feed(origin, access, value)?;
                for (lane, value) in value.to_le_bytes().into_iter().enumerate() {
                    if queues[fifo].len() == 32 {
                        queues[fifo].pop_front();
                        evicted[fifo] += 1;
                    }
                    queues[fifo].push_back(Byte {
                        value: value as i8,
                        event: index,
                        lane,
                    });
                }
            }
            GbaAudioTraceWrite::FifoReset { fifo, access, .. } => {
                ensure!(
                    resets.pop_front() == Some((fifo, access, origin, event.cycle)),
                    "GBA FIFO reset has no matching control write"
                );
                ensure!(
                    matches!(
                        origin,
                        GbaAudioTraceOrigin::Cpu { .. }
                            | GbaAudioTraceOrigin::CpuNonInstruction
                            | GbaAudioTraceOrigin::Dma(_)
                    ),
                    "GBA FIFO reset has no supported producer"
                );
                ensure!(
                    provenance::applied_address(access)? == 0x0400_0082,
                    "invalid GBA FIFO reset source"
                );
                let fifo = fifo_index(fifo);
                queues[fifo].clear();
                current[fifo] = 0;
            }
            GbaAudioTraceWrite::FifoPop {
                fifo,
                timer,
                effective_soundcnt_h,
                before_len,
                after_len,
                value,
                underflow,
                ..
            } => {
                let fifo = fifo_index(fifo);
                let shift = 8 + fifo * 4;
                ensure!(
                    timer <= 1
                        && effective_soundcnt_h & !0x770f == 0
                        && origin == GbaAudioTraceOrigin::Timer { timer }
                        && ((effective_soundcnt_h >> (shift + 2)) & 1) == u16::from(timer)
                        && effective_soundcnt_h & (3 << shift) != 0
                        && usize::from(before_len) == queues[fifo].len(),
                    "invalid GBA FIFO pop context"
                );
                let byte = queues[fifo].pop_front();
                ensure!(
                    underflow == byte.is_none()
                        && value == byte.map_or(0, |byte| byte.value)
                        && usize::from(after_len) == queues[fifo].len(),
                    "GBA FIFO replay disagrees with the captured pop"
                );
                current[fifo] = value;
                let sample = samples[fifo].len();
                samples[fifo].push(value as u8);
                pops.push(json!({
                    "event": index, "cycle": event.cycle, "fifo": fifo, "sample": sample,
                    "underflow": underflow,
                    "source": byte.map(|byte| json!({"event": byte.event, "lane": byte.lane})),
                }));
            }
            GbaAudioTraceWrite::Terminal { fifo_a, fifo_b, .. } => {
                ensure!(
                    origin == GbaAudioTraceOrigin::Unknown && event.cycle == trace.end_cycle,
                    "invalid GBA FIFO terminal event"
                );
                for (fifo, state) in [fifo_a, fifo_b].into_iter().enumerate() {
                    ensure!(
                        usize::from(state.len) == queues[fifo].len()
                            && state.current == current[fifo],
                        "GBA FIFO final state disagrees with replay"
                    );
                    for (lane, &value) in state.queue.iter().enumerate() {
                        ensure!(
                            value == queues[fifo].get(lane).map_or(0, |byte| byte.value),
                            "GBA FIFO final queue disagrees with replay"
                        );
                    }
                }
                terminal = true;
            }
        }
    }
    ensure!(terminal, "GBA FIFO trace has no terminal snapshot");
    Ok(Feed {
        samples,
        metadata: json!({
            "schema": "zeff-gba-fifo-feed/1", "pops": pops, "evicted_bytes": evicted,
            "final_queued_bytes": [queues[0].len(), queues[1].len()],
            "final_current": current,
            "source_links": "event and lane refer to the applied FIFO halfword in trace.json",
        }),
    })
}

fn fifo_index(fifo: GbaDirectSoundFifo) -> usize {
    match fifo {
        GbaDirectSoundFifo::A => 0,
        GbaDirectSoundFifo::B => 1,
    }
}
