use anyhow::{Result, anyhow, bail};
pub use zeff_emu_common::save_state::{StateReader, StateWriter};

pub const NES_SAVE_STATE_MAGIC: [u8; 8] = *b"ZBNSTATE";
pub const NES_SAVE_STATE_FORMAT_VERSION: u32 = 2;

const FORMAT_VERSION_V1_UNCOMPRESSED: u32 = 1;

const CHR_MAX_SIZE: usize = 2 * 1024 * 1024;

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
    // Write the raw state payload (CPU + Bus)
    let mut payload = StateWriter::new();
    payload.write_bytes(&emu.rom_hash);
    emu.cpu.write_state(&mut payload);
    emu.bus.write_state(&mut payload);
    let raw_bytes = payload.into_bytes();

    // Compress payload with lz4
    let compressed = lz4_flex::compress_prepend_size(&raw_bytes);

    // Assemble final: magic + version + compressed_payload
    let mut out = Vec::with_capacity(12 + compressed.len());
    out.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    out.extend_from_slice(&NES_SAVE_STATE_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&compressed);
    Ok(out)
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
    let payload_ref: &[u8] = match format_version {
        FORMAT_VERSION_V1_UNCOMPRESSED => {
            // V1: raw bytes after magic(8) + version(4)
            &bytes[12..]
        }
        NES_SAVE_STATE_FORMAT_VERSION => {
            // V2: lz4-compressed payload after magic(8) + version(4)
            payload = lz4_flex::decompress_size_prepended(&bytes[12..])
                .map_err(|e| anyhow!("failed to decompress save-state: {e}"))?;
            &payload
        }
        other => {
            bail!(
                "unsupported NES save-state format version {} (expected {} or {})",
                other,
                FORMAT_VERSION_V1_UNCOMPRESSED,
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

    // CPU
    emu.cpu.read_state(&mut r)?;
    // Bus
    emu.bus.read_state(&mut r)?;

    if !r.is_exhausted() {
        bail!("save-state has unexpected trailing data");
    }

    Ok(())
}

#[cfg(test)]
mod tests;
