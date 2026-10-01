use anyhow::{Result, ensure};
use zeff_emu_common::audio_trace::{
    AudioTraceEvent, AudioTraceSource, GbaAudioTraceAccess, GbaAudioTraceDmaKind,
    GbaAudioTraceOrigin, GbaAudioTraceSource, GbaAudioTraceWrite,
};
use zeff_gba_core::hardware::cartridge::Cartridge;

pub(super) fn applied_address(access: GbaAudioTraceAccess) -> Result<u32> {
    ensure!(
        matches!(access.width, 1 | 2 | 4)
            && (access.halfword_lane == 0 || (access.width == 4 && access.halfword_lane == 2)),
        "invalid GBA FIFO access width or lane"
    );
    let mask = if access.width == 4 { 3 } else { 1 };
    Ok((access.address & !mask) + u32::from(access.halfword_lane))
}

pub(super) fn origin(
    event: &AudioTraceEvent<GbaAudioTraceWrite>,
    origin: GbaAudioTraceOrigin,
    cartridge: &Cartridge,
) -> Result<()> {
    if let GbaAudioTraceOrigin::Cpu { active_pc } = origin {
        ensure!(
            event.pc == active_pc,
            "GBA FIFO CPU writer disagrees with its event"
        );
        match event.instruction_source {
            AudioTraceSource::CartridgeRom {
                offset,
                bit_reversed,
            } => ensure!(
                !bit_reversed
                    && (0x0800_0000..=0x0dff_ffff).contains(&active_pc)
                    && offset == u64::from(active_pc & 0x01ff_ffff)
                    && offset < cartridge.rom().len() as u64
                    && !(cartridge.has_rtc() && (0xc4..=0xc9).contains(&offset)),
                "GBA FIFO CPU ROM origin disagrees with its address"
            ),
            AudioTraceSource::WorkRam { offset } => ensure!(
                ((0x0200_0000..=0x02ff_ffff).contains(&active_pc) && offset == active_pc & 0x3ffff)
                    || ((0x0300_0000..=0x03ff_ffff).contains(&active_pc)
                        && offset == 0x40000 + (active_pc & 0x7fff)),
                "GBA FIFO CPU RAM origin disagrees with its address"
            ),
            AudioTraceSource::Unknown | AudioTraceSource::Unmapped => {}
            _ => anyhow::bail!("unsupported GBA FIFO CPU source"),
        }
    } else {
        ensure!(
            event.pc == 0 && event.instruction_source == AudioTraceSource::Unknown,
            "GBA FIFO autonomous event has a CPU writer"
        );
    }
    let GbaAudioTraceOrigin::Dma(dma) = origin else {
        return Ok(());
    };
    ensure!(
        dma.channel < 4 && matches!(dma.width, 2 | 4),
        "invalid GBA FIFO DMA descriptor"
    );
    let requested = match dma.kind {
        GbaAudioTraceDmaKind::Normal => {
            dma.requested_source
                & if dma.channel == 0 {
                    0x07ff_ffff
                } else {
                    0x0fff_ffff
                }
        }
        GbaAudioTraceDmaKind::Fifo => {
            ensure!(
                matches!(dma.channel, 1 | 2) && dma.width == 4 && !dma.source_latched,
                "invalid GBA sound FIFO DMA descriptor"
            );
            dma.requested_source
        }
    };
    ensure!(
        dma.aligned_source == requested & !(u32::from(dma.width) - 1),
        "GBA DMA source alignment disagrees with the transfer"
    );
    if dma.kind == GbaAudioTraceDmaKind::Normal {
        ensure!(
            dma.source_latched == (dma.aligned_source < 0x0200_0000),
            "GBA DMA latch selection disagrees with its source"
        );
    }
    for lane in 0..usize::from(dma.width) {
        let address = dma.aligned_source + lane as u32;
        let value = (dma.value >> (lane * 8)) as u8;
        let source = dma.source_lanes[lane];
        ensure!(
            (source == GbaAudioTraceSource::Latch) == dma.source_latched,
            "GBA DMA latch provenance disagrees with the transfer"
        );
        match source {
            GbaAudioTraceSource::Rom { offset } => {
                ensure!(
                    (0x0800_0000..=0x0dff_ffff).contains(&address)
                        && offset == address & 0x01ff_ffff
                        && !(cartridge.has_rtc() && (0xc4..=0xc9).contains(&offset))
                        && !(dma.width == 2 && cartridge.is_eeprom_access_addr(dma.aligned_source))
                        && cartridge.rom().get(offset as usize) == Some(&value),
                    "GBA DMA ROM provenance does not match the loaded source"
                );
            }
            GbaAudioTraceSource::Ewram { offset } => ensure!(
                (0x0200_0000..=0x02ff_ffff).contains(&address) && offset == address & 0x3ffff,
                "GBA DMA EWRAM provenance is outside its mirror"
            ),
            GbaAudioTraceSource::Iwram { offset } => ensure!(
                (0x0300_0000..=0x03ff_ffff).contains(&address) && offset == address & 0x7fff,
                "GBA DMA IWRAM provenance is outside its mirror"
            ),
            GbaAudioTraceSource::Bios { offset } => ensure!(
                address < 0x4000 && offset == address,
                "GBA DMA BIOS provenance is outside the BIOS"
            ),
            GbaAudioTraceSource::Unknown { address: declared } => ensure!(
                declared == address,
                "GBA DMA unresolved address disagrees with the read"
            ),
            GbaAudioTraceSource::Latch => {}
        }
    }
    Ok(())
}

pub(super) fn feed(
    origin: GbaAudioTraceOrigin,
    access: GbaAudioTraceAccess,
    value: u16,
) -> Result<()> {
    match origin {
        GbaAudioTraceOrigin::Dma(dma) => ensure!(
            access.width == dma.width
                && value == (dma.value >> (u32::from(access.halfword_lane) * 8)) as u16,
            "GBA DMA payload disagrees with the applied FIFO bytes"
        ),
        GbaAudioTraceOrigin::Cpu { .. } | GbaAudioTraceOrigin::CpuNonInstruction => {}
        _ => anyhow::bail!("GBA FIFO write has no supported producer"),
    }
    Ok(())
}
