use super::Audio;
use crate::cartridge::Cartridge;
use crate::constants::*;
use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceInvalidation, AudioTraceSource, MdTraceChip, MdTraceOrigin,
    MdTraceRegion, MdTraceReset, MdTraceSource, MdTraceWrite, MdTraceWriter,
};
use zeff_m68k::bus::BUS_ADDRESS_MASK;

pub(crate) fn trace_chip(region: crate::Region) -> MdTraceChip {
    MdTraceChip {
        master_hz: region.master_hz(),
        region: match region {
            crate::Region::NtscUs => MdTraceRegion::NtscUs,
            crate::Region::NtscJapan => MdTraceRegion::NtscJapan,
            crate::Region::Pal => MdTraceRegion::Pal,
        },
        psg_divisor: super::constants::PSG_CLOCK_TICKS as u32,
        opn2_divisor: OPN2_CLOCK_TICKS as u32,
        reset: MdTraceReset::ZeffPowerOnV1,
    }
}

impl Audio {
    pub(crate) fn record_trace(&mut self, at: u64, write: MdTraceWrite) {
        let origin = match write {
            MdTraceWrite::Psg { origin, .. }
            | MdTraceWrite::Opn2Port { origin, .. }
            | MdTraceWrite::Opn2Reset { origin } => origin,
        };
        self.trace.record(AudioTraceEvent {
            cycle: at,
            pc: origin.boundary_pc,
            instruction_source: AudioTraceSource::Unknown,
            write,
        });
    }

    pub(crate) fn invalidate_trace(&mut self, reason: AudioTraceInvalidation) {
        self.trace.invalidate(reason);
        self.trace_origin = None;
    }
}

pub(crate) fn main_origin(pc: u32, cartridge: &Cartridge) -> MdTraceOrigin {
    MdTraceOrigin {
        writer: MdTraceWriter::M68000,
        boundary_pc: pc,
        source: main_source(pc & BUS_ADDRESS_MASK, cartridge),
    }
}

pub(crate) fn sound_origin(pc: u16, bank: u16, cartridge: &Cartridge) -> MdTraceOrigin {
    MdTraceOrigin {
        writer: MdTraceWriter::Z80,
        boundary_pc: u32::from(pc),
        source: if pc <= SOUND_RAM_END {
            MdTraceSource::SoundRam {
                offset: pc & SOUND_RAM_MASK as u16,
            }
        } else if pc >= SOUND_BANK_WINDOW {
            let address = (u32::from(bank) << SOUND_BANK_SHIFT) | (u32::from(pc) & SOUND_MAIN_MASK);
            main_source(address, cartridge)
        } else {
            MdTraceSource::Unknown
        },
    }
}

fn main_source(address: u32, cartridge: &Cartridge) -> MdTraceSource {
    if (WORK_RAM_START..=BUS_ADDRESS_MASK).contains(&address) {
        MdTraceSource::MainRam {
            offset: address & WORK_RAM_MASK as u32,
        }
    } else if address <= CARTRIDGE_END
        && (address as usize) < cartridge.bytes.len()
        && !cartridge
            .save
            .as_ref()
            .is_some_and(|save| save.contains(address))
    {
        MdTraceSource::EffectiveRom { offset: address }
    } else {
        MdTraceSource::Unknown
    }
}
