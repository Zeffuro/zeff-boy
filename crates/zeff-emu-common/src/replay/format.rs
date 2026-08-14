/// File format (`.zrpl`):
/// ```text
/// [4 bytes]  magic: "ZRPL"
/// [4 bytes]  version: 1 (u32 LE)
/// [4 bytes]  metadata_length (u32 LE)
/// [N bytes]  replay metadata
/// [4 bytes]  save_state_length (u32 LE)
/// [N bytes]  save state data
/// [4 bytes]  frame_count (u32 LE)
/// [remaining] frames: repeated frame records
///     fixed input record, 18 bytes:
///         p1_buttons: u8, p1_dpad: u8, p2_buttons: u8, p2_dpad: u8,
///         zapper_flags: u8, zapper_x: u16 LE, zapper_y: u16 LE,
///         tilt_x: f32 LE, tilt_y: f32 LE, reserved: u8
///     camera_frame_length: u32 LE
///         0 = no host camera update for this frame
///         0xFFFF_FFFF = repeat previous host camera frame
///         otherwise exactly that many camera bytes follow
/// ```
use anyhow::{Context, Result, bail};

pub(crate) const MAGIC: &[u8; 4] = b"ZRPL";
pub(crate) const VERSION: u32 = 1;
pub(crate) const FRAME_FIXED_BYTES: usize = 18;
pub(crate) const CAMERA_REPEAT_SENTINEL: u32 = u32::MAX;
pub(crate) const MAX_REPLAY_CAMERA_FRAME_BYTES: usize = 1024 * 1024;
// Pre-metadata v1 recordings stored `[save_state_len][save_state][2-byte input frames]`.
// Keep this narrow so corrupt metadata-first files are not silently accepted as legacy.
pub(crate) const LEGACY_GB_SAVE_STATE_MAGIC: &[u8; 8] = b"ZBSTATE\0";
pub(crate) const LEGACY_NES_SAVE_STATE_MAGIC: &[u8; 8] = b"ZBNSTATE";

pub(crate) fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn write_string(out: &mut Vec<u8>, value: &str) {
    write_u32(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}

pub(crate) fn write_optional_string(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            out.push(1);
            write_string(out, value);
        }
        None => out.push(0),
    }
}

pub(crate) fn write_optional_hash(out: &mut Vec<u8>, value: Option<[u8; 32]>) {
    match value {
        Some(value) => {
            out.push(1);
            out.extend_from_slice(&value);
        }
        None => out.push(0),
    }
}

pub(crate) fn write_optional_u8(out: &mut Vec<u8>, value: Option<u8>) {
    match value {
        Some(value) => {
            out.push(1);
            out.push(value);
        }
        None => out.push(0),
    }
}

pub(crate) fn write_optional_u64(out: &mut Vec<u8>, value: Option<u64>) {
    match value {
        Some(value) => {
            out.push(1);
            write_u64(out, value);
        }
        None => out.push(0),
    }
}

pub(crate) fn read_bool(cursor: &mut MetadataCursor<'_>, name: &str) -> Result<bool> {
    match cursor.read_u8()? {
        0 => Ok(false),
        1 => Ok(true),
        value => bail!("invalid replay metadata {name}: {value}"),
    }
}

pub(crate) fn read_optional_u8(cursor: &mut MetadataCursor<'_>, name: &str) -> Result<Option<u8>> {
    match cursor.read_u8()? {
        0 => Ok(None),
        1 => Ok(Some(cursor.read_u8()?)),
        value => bail!("invalid replay metadata {name} tag: {value}"),
    }
}

pub(crate) struct MetadataCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> MetadataCursor<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8> {
        let byte = *self
            .bytes
            .get(self.offset)
            .ok_or_else(|| anyhow::anyhow!("truncated replay metadata"))?;
        self.offset += 1;
        Ok(byte)
    }

    pub(crate) fn read_u32(&mut self) -> Result<u32> {
        let bytes = self.read_exact(4)?;
        let mut buf = [0u8; 4];
        buf.copy_from_slice(bytes);
        Ok(u32::from_le_bytes(buf))
    }

    pub(crate) fn read_u64(&mut self) -> Result<u64> {
        let bytes = self.read_exact(8)?;
        let mut buf = [0u8; 8];
        buf.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(buf))
    }

    pub(crate) fn read_hash(&mut self) -> Result<[u8; 32]> {
        let bytes = self.read_exact(32)?;
        let mut hash = [0u8; 32];
        hash.copy_from_slice(bytes);
        Ok(hash)
    }

    pub(crate) fn read_optional_hash(&mut self) -> Result<Option<[u8; 32]>> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.read_hash()?)),
            tag => bail!("invalid optional hash tag: {tag}"),
        }
    }

    pub(crate) fn read_string(&mut self) -> Result<String> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_exact(len)?;
        String::from_utf8(bytes.to_vec()).context("replay metadata string is not UTF-8")
    }

    pub(crate) fn read_optional_string(&mut self) -> Result<Option<String>> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.read_string()?)),
            tag => bail!("invalid optional string tag: {tag}"),
        }
    }

    pub(crate) fn read_exact(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| anyhow::anyhow!("replay metadata offset overflow"))?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| anyhow::anyhow!("truncated replay metadata"))?;
        self.offset = end;
        Ok(bytes)
    }

    pub(crate) fn finish(self) -> Result<()> {
        if self.is_finished() {
            Ok(())
        } else {
            bail!("trailing replay metadata bytes")
        }
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}
