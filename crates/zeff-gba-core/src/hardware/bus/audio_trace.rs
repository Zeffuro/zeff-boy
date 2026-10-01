use super::{
    AudioTraceAccessContext, BIOS_END, BIOS_START, Bus, EWRAM_END, EWRAM_SIZE, EWRAM_START,
    GAMEPAK_ROM_END, GAMEPAK0_START, IWRAM_END, IWRAM_SIZE, IWRAM_START,
};
use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceInvalidation, AudioTraceSource, AudioTraceTiming,
    GBA_AUDIO_TRACE_CLOCK_HZ, GbaAudioTrace, GbaAudioTraceAccess, GbaAudioTraceChip,
    GbaAudioTraceDma, GbaAudioTraceDmaKind, GbaAudioTraceOrigin, GbaAudioTraceRecorder,
    GbaAudioTraceReset, GbaAudioTraceSource, GbaAudioTraceWrite,
};

#[derive(Clone, Copy)]
pub(super) struct DmaTraceRead {
    pub(super) requested_source: u32,
    pub(super) aligned_source: u32,
    pub(super) width: u8,
    pub(super) value: u32,
    pub(super) source_latched: bool,
}

impl Bus {
    pub(crate) fn prepare_audio_trace(
        &self,
        max_events: usize,
        source_sha256: [u8; 32],
    ) -> anyhow::Result<GbaAudioTraceRecorder> {
        self.audio_trace.prepare(
            max_events,
            GBA_AUDIO_TRACE_CLOCK_HZ,
            GbaAudioTraceChip {
                clock_hz: GBA_AUDIO_TRACE_CLOCK_HZ,
                reset: GbaAudioTraceReset::PostBiosV1,
                source_sha256,
            },
            AudioTraceTiming::BusServiceBoundary,
        )
    }

    pub(crate) fn install_audio_trace(&mut self, trace: GbaAudioTraceRecorder) {
        self.audio_trace = trace;
        self.audio_trace_origin = GbaAudioTraceOrigin::Unknown;
        self.audio_trace_access = None;
    }

    #[cfg(test)]
    pub(crate) fn begin_audio_trace(
        &mut self,
        max_events: usize,
        source_sha256: [u8; 32],
    ) -> anyhow::Result<()> {
        let trace = self.prepare_audio_trace(max_events, source_sha256)?;
        self.install_audio_trace(trace);
        Ok(())
    }

    pub(crate) fn finish_audio_trace(&mut self) -> Option<GbaAudioTrace> {
        if !self.audio_trace.is_active() {
            return None;
        }
        self.materialize_frame_service();
        self.record_audio_trace(GbaAudioTraceWrite::Terminal {
            fifo_a: self.apu.direct_sound_fifo_state(0),
            fifo_b: self.apu.direct_sound_fifo_state(1),
            origin: GbaAudioTraceOrigin::Unknown,
        });
        self.audio_trace.finish(self.master_cycles)
    }

    pub(crate) fn audio_trace_enabled(&self) -> bool {
        self.audio_trace.is_enabled()
    }

    pub(crate) fn invalidate_audio_trace(&mut self, reason: AudioTraceInvalidation) {
        self.audio_trace.invalidate(reason);
    }

    pub(crate) fn set_audio_trace_cpu_active_pc(&mut self, active_pc: u32) {
        if !self.audio_trace.is_enabled() {
            return;
        }
        self.audio_trace_origin = GbaAudioTraceOrigin::Cpu { active_pc };
    }

    pub(crate) fn clear_audio_trace_cpu_origin(&mut self) {
        if matches!(self.audio_trace_origin, GbaAudioTraceOrigin::Cpu { .. }) {
            self.audio_trace_origin = GbaAudioTraceOrigin::Unknown;
        }
    }

    pub(super) fn set_audio_trace_origin(
        &mut self,
        origin: GbaAudioTraceOrigin,
    ) -> GbaAudioTraceOrigin {
        if !self.audio_trace.is_enabled() {
            return GbaAudioTraceOrigin::Unknown;
        }
        std::mem::replace(&mut self.audio_trace_origin, origin)
    }

    pub(super) fn audio_trace_dma(
        &self,
        channel: usize,
        kind: GbaAudioTraceDmaKind,
        read: DmaTraceRead,
    ) -> GbaAudioTraceOrigin {
        if !self.audio_trace.is_enabled() {
            return GbaAudioTraceOrigin::Unknown;
        }
        let mut source_lanes = [GbaAudioTraceSource::Unknown { address: 0 }; 4];
        let unresolved_eeprom =
            read.width == 2 && self.cartridge.is_eeprom_access_addr(read.aligned_source);
        for (lane, source) in source_lanes
            .iter_mut()
            .enumerate()
            .take(usize::from(read.width))
        {
            *source = if read.source_latched {
                GbaAudioTraceSource::Latch
            } else if unresolved_eeprom {
                GbaAudioTraceSource::Unknown {
                    address: read.aligned_source.wrapping_add(lane as u32),
                }
            } else {
                self.audio_trace_source(read.aligned_source.wrapping_add(lane as u32))
            };
        }
        GbaAudioTraceOrigin::Dma(GbaAudioTraceDma {
            channel: channel as u8,
            kind,
            requested_source: read.requested_source,
            aligned_source: read.aligned_source,
            width: read.width,
            value: read.value,
            source_latched: read.source_latched,
            source_lanes,
        })
    }

    pub(super) fn set_audio_trace_access(
        &mut self,
        address: u32,
        width: u8,
    ) -> Option<AudioTraceAccessContext> {
        if !self.audio_trace.is_enabled() {
            return None;
        }
        self.audio_trace_access
            .replace(AudioTraceAccessContext { address, width })
    }

    pub(super) fn audio_trace_access(&self, applied_address: u32) -> GbaAudioTraceAccess {
        let context = self.audio_trace_access.unwrap_or(AudioTraceAccessContext {
            address: applied_address,
            width: 2,
        });
        GbaAudioTraceAccess {
            address: context.address,
            width: context.width,
            halfword_lane: if context.width == 4 {
                applied_address.wrapping_sub(context.address & !3) as u8
            } else {
                0
            },
        }
    }

    pub(super) fn record_audio_trace(&mut self, write: GbaAudioTraceWrite) {
        if !self.audio_trace.is_enabled() {
            return;
        }
        let origin = match write {
            GbaAudioTraceWrite::Control { origin, .. }
            | GbaAudioTraceWrite::FifoHalfword { origin, .. }
            | GbaAudioTraceWrite::FifoReset { origin, .. }
            | GbaAudioTraceWrite::FifoPop { origin, .. }
            | GbaAudioTraceWrite::Terminal { origin, .. } => origin,
        };
        let (pc, instruction_source) = match origin {
            GbaAudioTraceOrigin::Cpu { active_pc } => {
                (active_pc, self.audio_trace_instruction_source(active_pc))
            }
            _ => (0, AudioTraceSource::Unknown),
        };
        self.audio_trace.record(AudioTraceEvent {
            cycle: self.master_cycles,
            pc,
            instruction_source,
            write,
        });
    }

    fn audio_trace_instruction_source(&self, address: u32) -> AudioTraceSource {
        match self.audio_trace_source(address) {
            GbaAudioTraceSource::Rom { offset } => AudioTraceSource::CartridgeRom {
                offset: u64::from(offset),
                bit_reversed: false,
            },
            GbaAudioTraceSource::Bios { offset } => AudioTraceSource::BootRom {
                offset: u64::from(offset),
            },
            GbaAudioTraceSource::Ewram { offset } => AudioTraceSource::WorkRam { offset },
            GbaAudioTraceSource::Iwram { offset } => AudioTraceSource::WorkRam {
                offset: EWRAM_SIZE as u32 + offset,
            },
            _ => AudioTraceSource::Unknown,
        }
    }

    fn audio_trace_source(&self, address: u32) -> GbaAudioTraceSource {
        match address {
            BIOS_START..=BIOS_END if self.has_external_bios() => {
                GbaAudioTraceSource::Bios { offset: address }
            }
            EWRAM_START..=EWRAM_END => GbaAudioTraceSource::Ewram {
                offset: (address as usize & (EWRAM_SIZE - 1)) as u32,
            },
            IWRAM_START..=IWRAM_END => GbaAudioTraceSource::Iwram {
                offset: (address as usize & (IWRAM_SIZE - 1)) as u32,
            },
            GAMEPAK0_START..=GAMEPAK_ROM_END if !self.cartridge.has_rtc() => {
                let offset = address & 0x01FF_FFFF;
                if (offset as usize) < self.cartridge.rom().len() {
                    GbaAudioTraceSource::Rom { offset }
                } else {
                    GbaAudioTraceSource::Unknown { address }
                }
            }
            _ => GbaAudioTraceSource::Unknown { address },
        }
    }
}
