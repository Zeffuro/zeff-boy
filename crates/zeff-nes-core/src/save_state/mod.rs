use anyhow::{Result, anyhow, bail};
pub use zeff_emu_common::save_state::{StateReader, StateWriter};

pub const NES_SAVE_STATE_MAGIC: [u8; 8] = *b"ZBNSTATE";
pub const NES_SAVE_STATE_FORMAT_VERSION: u32 = 11;
pub const TAS_DETERMINISM_ABI_ID: &str = "zeff-nes-determinism-v1";
pub const TAS_STATE_FORMAT_COMPATIBILITY_ID: &str = "zeff-nes-native-state-v11";

const FORMAT_VERSION_V1_UNCOMPRESSED: u32 = 1;
const FORMAT_VERSION_V2_COMPRESSED: u32 = 2;
const FORMAT_VERSION_V3_COMPRESSED: u32 = 3;
const FORMAT_VERSION_V4_COMPRESSED: u32 = 4;
const FORMAT_VERSION_V5_COMPRESSED: u32 = 5;
const FORMAT_VERSION_V6_COMPRESSED: u32 = 6;
const FORMAT_VERSION_V7_COMPRESSED: u32 = 7;
const FORMAT_VERSION_V8_COMPRESSED: u32 = 8;
const FORMAT_VERSION_V9_COMPRESSED: u32 = 9;
const FORMAT_VERSION_V10_COMPRESSED: u32 = 10;

const CHR_MAX_SIZE: usize = 2 * 1024 * 1024;
const OUTPUT_SUFFIX_LEN: usize = 1 + crate::hardware::constants::FRAMEBUFFER_LEN;
const MAX_REPLAY_PROJECTION_PAYLOAD_LEN: usize = 32 * 1024 * 1024;

pub fn write_chr_state(w: &mut StateWriter, chr: &[u8]) {
    w.write_vec(chr);
}

pub fn read_chr_state(r: &mut StateReader, chr: &mut Vec<u8>, label: &str) -> Result<()> {
    let loaded = r.read_vec(CHR_MAX_SIZE)?;
    if loaded.len() != chr.len() {
        bail!(
            "{label} CHR size mismatch: expected {}, got {}",
            chr.len(),
            loaded.len()
        );
    }
    *chr = loaded;
    Ok(())
}

pub fn encode_mirroring(m: crate::hardware::cartridge::Mirroring) -> u8 {
    use crate::hardware::cartridge::Mirroring;
    match m {
        Mirroring::Horizontal => 0,
        Mirroring::Vertical => 1,
        Mirroring::SingleScreenLower => 2,
        Mirroring::SingleScreenUpper => 3,
        Mirroring::FourScreen => 4,
    }
}

pub fn decode_mirroring(tag: u8) -> Result<crate::hardware::cartridge::Mirroring> {
    use crate::hardware::cartridge::Mirroring;
    match tag {
        0 => Ok(Mirroring::Horizontal),
        1 => Ok(Mirroring::Vertical),
        2 => Ok(Mirroring::SingleScreenLower),
        3 => Ok(Mirroring::SingleScreenUpper),
        4 => Ok(Mirroring::FourScreen),
        _ => bail!("invalid mirroring tag in save-state: {tag}"),
    }
}

// ─── Top-level encode / decode ─────────────────────────────────────

pub fn encode_state(emu: &crate::emulator::Emulator) -> Result<Vec<u8>> {
    let mut payload = StateWriter::new();
    payload.write_bytes(&emu.rom_hash);
    payload.write_u8(emu.bus.timing.state_tag());
    payload.write_u8(emu.bus.ppu_clock.master_phase());
    emu.cpu.write_state(&mut payload);
    emu.bus.write_state(&mut payload);
    emu.cpu.write_jam_state(&mut payload);
    emu.bus.write_dma_state(&mut payload);
    emu.bus.write_apu_runtime_state(&mut payload);
    emu.bus.write_ppu_runtime_state(&mut payload);
    emu.bus.write_mutable_media_state(&mut payload);
    payload.write_bool(emu.bus.ppu.frame_ready);
    payload.write_bytes(&emu.bus.ppu.framebuffer[..]);
    let raw_bytes = payload.into_bytes();

    let compressed = lz4_flex::compress_prepend_size(&raw_bytes);

    let mut out = Vec::with_capacity(12 + compressed.len());
    out.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    out.extend_from_slice(&NES_SAVE_STATE_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&compressed);
    Ok(out)
}

#[cfg(test)]
pub(crate) fn encode_legacy_v10_state(emu: &crate::emulator::Emulator) -> Result<Vec<u8>> {
    let mut payload = StateWriter::new();
    payload.write_bytes(&emu.rom_hash);
    payload.write_u8(emu.bus.timing.state_tag());
    payload.write_u8(emu.bus.ppu_clock.master_phase());
    emu.cpu.write_state(&mut payload);
    emu.bus.write_state(&mut payload);
    emu.cpu.write_jam_state(&mut payload);
    emu.bus.write_dma_state(&mut payload);
    emu.bus.write_apu_runtime_state(&mut payload);
    emu.bus.write_ppu_runtime_state(&mut payload);
    emu.bus.write_mutable_media_state(&mut payload);
    let compressed = lz4_flex::compress_prepend_size(&payload.into_bytes());

    let mut out = Vec::with_capacity(12 + compressed.len());
    out.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    out.extend_from_slice(&FORMAT_VERSION_V10_COMPRESSED.to_le_bytes());
    out.extend_from_slice(&compressed);
    Ok(out)
}

pub fn project_replay_state_bytes(bytes: &mut Vec<u8>) -> Result<()> {
    if bytes.len() < 12 {
        bail!("NES save-state data is too short for header");
    }
    if bytes[0..8] != NES_SAVE_STATE_MAGIC {
        bail!("not a valid NES save-state for replay projection");
    }

    let format_version = u32::from_le_bytes(bytes[8..12].try_into().expect("four-byte version"));
    if (FORMAT_VERSION_V1_UNCOMPRESSED..=FORMAT_VERSION_V10_COMPRESSED).contains(&format_version) {
        return Ok(());
    }
    if format_version != NES_SAVE_STATE_FORMAT_VERSION {
        bail!("unsupported NES save-state format {format_version} for replay projection");
    }
    if bytes.len() < 16 {
        bail!("NES save-state data is too short for compressed header");
    }

    let declared_len =
        u32::from_le_bytes(bytes[12..16].try_into().expect("four-byte payload length")) as usize;
    if declared_len > MAX_REPLAY_PROJECTION_PAYLOAD_LEN {
        bail!(
            "NES replay projection payload length {declared_len} exceeds maximum {MAX_REPLAY_PROJECTION_PAYLOAD_LEN}"
        );
    }
    if declared_len < OUTPUT_SUFFIX_LEN {
        bail!("NES v11 save-state payload is too short for output suffix");
    }

    let mut payload = vec![0; declared_len];
    let decoded_len = lz4_flex::decompress_into(&bytes[16..], &mut payload)
        .map_err(|error| anyhow!("failed to decompress NES save-state: {error}"))?;
    if decoded_len != declared_len {
        bail!(
            "NES save-state decompressed length mismatch: expected {declared_len}, got {decoded_len}"
        );
    }

    let suffix_start = payload.len() - OUTPUT_SUFFIX_LEN;
    if !matches!(payload[suffix_start], 0 | 1) {
        bail!("invalid NES v11 frame-ready boolean");
    }
    payload.truncate(suffix_start);

    let compressed = lz4_flex::compress_prepend_size(&payload);
    bytes.truncate(12);
    bytes[8..12].copy_from_slice(&FORMAT_VERSION_V10_COMPRESSED.to_le_bytes());
    bytes.extend_from_slice(&compressed);
    Ok(())
}

pub fn decode_state(emu: &mut crate::emulator::Emulator, bytes: &[u8]) -> Result<()> {
    // Read and validate the outer header (magic + version)
    if bytes.len() < 12 {
        bail!("save-state data is too short for header");
    }
    let magic = &bytes[0..8];
    if magic != NES_SAVE_STATE_MAGIC {
        bail!("not a valid NES save-state (bad magic)");
    }
    let format_version = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);

    // Get the payload bytes (either raw or lz4-decompressed)
    let payload: Vec<u8>;
    let (
        payload_ref,
        has_jam_state,
        has_dma_state,
        has_apu_runtime_state,
        has_ppu_runtime_state,
        has_mutable_media_state,
        has_sprite_evaluation_state,
        has_timing_state,
        has_output_state,
    ): (&[u8], bool, bool, bool, bool, bool, bool, bool, bool) = match format_version {
        FORMAT_VERSION_V1_UNCOMPRESSED => {
            // V1: raw bytes after magic(8) + version(4)
            (
                &bytes[12..],
                false,
                false,
                false,
                false,
                false,
                false,
                false,
                false,
            )
        }
        FORMAT_VERSION_V2_COMPRESSED
        | FORMAT_VERSION_V3_COMPRESSED
        | FORMAT_VERSION_V4_COMPRESSED
        | FORMAT_VERSION_V5_COMPRESSED
        | FORMAT_VERSION_V6_COMPRESSED
        | FORMAT_VERSION_V7_COMPRESSED
        | FORMAT_VERSION_V8_COMPRESSED
        | FORMAT_VERSION_V9_COMPRESSED
        | FORMAT_VERSION_V10_COMPRESSED
        | NES_SAVE_STATE_FORMAT_VERSION => {
            payload = lz4_flex::decompress_size_prepended(&bytes[12..])
                .map_err(|e| anyhow!("failed to decompress save-state: {e}"))?;
            (
                &payload,
                format_version >= FORMAT_VERSION_V3_COMPRESSED,
                format_version >= FORMAT_VERSION_V4_COMPRESSED,
                format_version >= FORMAT_VERSION_V5_COMPRESSED,
                format_version >= FORMAT_VERSION_V6_COMPRESSED,
                format_version >= FORMAT_VERSION_V7_COMPRESSED,
                format_version >= FORMAT_VERSION_V9_COMPRESSED,
                format_version >= FORMAT_VERSION_V10_COMPRESSED,
                format_version >= NES_SAVE_STATE_FORMAT_VERSION,
            )
        }
        other => {
            bail!(
                "unsupported NES save-state format version {} (expected {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, or {})",
                other,
                FORMAT_VERSION_V1_UNCOMPRESSED,
                FORMAT_VERSION_V2_COMPRESSED,
                FORMAT_VERSION_V3_COMPRESSED,
                FORMAT_VERSION_V4_COMPRESSED,
                FORMAT_VERSION_V5_COMPRESSED,
                FORMAT_VERSION_V6_COMPRESSED,
                FORMAT_VERSION_V7_COMPRESSED,
                FORMAT_VERSION_V8_COMPRESSED,
                FORMAT_VERSION_V9_COMPRESSED,
                FORMAT_VERSION_V10_COMPRESSED,
                NES_SAVE_STATE_FORMAT_VERSION
            );
        }
    };

    let mut r = StateReader::new(payload_ref);

    let mut rom_hash = [0u8; 32];
    r.read_exact(&mut rom_hash)?;
    if rom_hash != emu.rom_hash {
        bail!("save-state ROM hash does not match the currently loaded ROM");
    }

    let restored_ppu_clock = if has_timing_state {
        let saved_timing = crate::hardware::timing::NesTiming::from_state_tag(r.read_u8()?)?;
        if saved_timing != emu.bus.timing {
            bail!(
                "save-state timing {:?} does not match the currently loaded {:?} machine",
                saved_timing,
                emu.bus.timing
            );
        }
        let mut clock = crate::hardware::timing::CpuPpuClock::new(saved_timing);
        clock.restore_master_phase(r.read_u8()?)?;
        clock
    } else {
        if emu.bus.timing != crate::hardware::timing::NesTiming::Ntsc {
            bail!("legacy NES save-state does not identify non-NTSC machine timing");
        }
        crate::hardware::timing::CpuPpuClock::new(crate::hardware::timing::NesTiming::Ntsc)
    };

    // CPU
    emu.cpu.read_state(&mut r)?;
    // Bus
    emu.bus.read_state(&mut r)?;
    if has_jam_state {
        emu.cpu.read_jam_state(&mut r)?;
    }
    if has_dma_state {
        emu.bus.read_dma_state(&mut r)?;
    }
    if has_apu_runtime_state {
        emu.bus.read_apu_runtime_state(&mut r)?;
    }
    if has_ppu_runtime_state {
        emu.bus
            .read_ppu_runtime_state(&mut r, has_sprite_evaluation_state)?;
    }
    if has_mutable_media_state {
        emu.bus.read_mutable_media_state(&mut r)?;
    } else {
        emu.bus.reset_mutable_media_to_source();
    }
    let restored_output = if has_output_state {
        let frame_ready = r.read_bool()?;
        let mut framebuffer = vec![0; crate::hardware::constants::FRAMEBUFFER_LEN];
        r.read_exact(&mut framebuffer)?;
        Some((frame_ready, framebuffer))
    } else {
        None
    };
    if !r.is_exhausted() {
        bail!("save-state has unexpected trailing data");
    }

    emu.bus.ppu_clock = restored_ppu_clock;
    if let Some((frame_ready, framebuffer)) = restored_output {
        emu.bus.ppu.frame_ready = frame_ready;
        emu.bus.ppu.framebuffer.copy_from_slice(&framebuffer);
    } else {
        emu.bus.ppu.frame_ready = false;
    }

    Ok(())
}

#[cfg(test)]
mod tests;
