use anyhow::{Result, anyhow, bail};
use sha2::{Digest, Sha256};

pub const ZPST_FOOTER_MAGIC: [u8; 4] = *b"ZPST";
pub const ZPST_AUTH_TAG: [u8; 4] = *b"AUTH";
pub const ZPST_END_TAG: [u8; 4] = *b"END ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZpstBlock<'a> {
    pub tag: [u8; 4],
    pub payload: &'a [u8],
}

#[derive(Debug)]
pub struct ZpstEnvelope<'a> {
    pub native_prefix: &'a [u8],
    pub blocks: Vec<ZpstBlock<'a>>,
}

pub struct ZpstWriter {
    native_prefix: Vec<u8>,
    blocks: Vec<([u8; 4], Vec<u8>)>,
    max_bytes: usize,
    max_blocks: usize,
}

impl ZpstWriter {
    pub fn new(native_prefix: Vec<u8>, max_bytes: usize, max_blocks: usize) -> Result<Self> {
        if native_prefix.len() > max_bytes || max_blocks < 2 {
            bail!("invalid ZPST envelope limits");
        }
        Ok(Self {
            native_prefix,
            blocks: Vec::new(),
            max_bytes,
            max_blocks,
        })
    }

    pub fn push_block(&mut self, tag: [u8; 4], payload: Vec<u8>) -> Result<()> {
        if tag == ZPST_AUTH_TAG || tag == ZPST_END_TAG {
            bail!("ZPST reserves AUTH and END block tags");
        }
        if self
            .blocks
            .len()
            .checked_add(3)
            .ok_or_else(|| anyhow!("ZPST block overflow"))?
            > self.max_blocks
        {
            bail!("ZPST block count exceeds maximum {}", self.max_blocks);
        }
        let encoded_len = self
            .native_prefix
            .len()
            .checked_add(self.blocks.iter().try_fold(0usize, |sum, (_, block)| {
                sum.checked_add(
                    8usize
                        .checked_add(block.len())
                        .ok_or_else(|| anyhow!("ZPST block size overflow"))?,
                )
                .ok_or_else(|| anyhow!("ZPST size overflow"))
            })?)
            .and_then(|sum| sum.checked_add(8 + payload.len()))
            .and_then(|sum| sum.checked_add(8 + 32 + 8 + 8))
            .ok_or_else(|| anyhow!("ZPST size overflow"))?;
        if encoded_len > self.max_bytes {
            bail!("ZPST envelope exceeds maximum {} bytes", self.max_bytes);
        }
        self.blocks.push((tag, payload));
        Ok(())
    }

    pub fn finish(self) -> Result<Vec<u8>> {
        let first_block_offset = u32::try_from(self.native_prefix.len())
            .map_err(|_| anyhow!("ZPST first-block offset exceeds u32"))?;
        let mut output = self.native_prefix;
        for (tag, payload) in self.blocks {
            output.extend_from_slice(&tag);
            output.extend_from_slice(
                &u32::try_from(payload.len())
                    .map_err(|_| anyhow!("ZPST block exceeds u32"))?
                    .to_le_bytes(),
            );
            output.extend_from_slice(&payload);
        }
        let auth = Sha256::digest(&output[first_block_offset as usize..]);
        output.extend_from_slice(&ZPST_AUTH_TAG);
        output.extend_from_slice(&(32u32).to_le_bytes());
        output.extend_from_slice(&auth);
        output.extend_from_slice(&ZPST_END_TAG);
        output.extend_from_slice(&0u32.to_le_bytes());
        output.extend_from_slice(&first_block_offset.to_le_bytes());
        output.extend_from_slice(&ZPST_FOOTER_MAGIC);
        if output.len() > self.max_bytes {
            bail!("ZPST envelope exceeds maximum {} bytes", self.max_bytes);
        }
        Ok(output)
    }
}

pub fn parse_zpst_envelope(
    bytes: &[u8],
    max_bytes: usize,
    max_blocks: usize,
) -> Result<ZpstEnvelope<'_>> {
    if bytes.len() > max_bytes || bytes.len() < 24 || max_blocks < 2 {
        bail!("invalid or oversized ZPST envelope");
    }
    let footer_start = bytes.len() - 8;
    if bytes[footer_start + 4..] != ZPST_FOOTER_MAGIC {
        bail!("missing ZPST footer");
    }
    let first = u32::from_le_bytes(
        bytes[footer_start..footer_start + 4]
            .try_into()
            .expect("footer length"),
    ) as usize;
    if first > footer_start {
        bail!("ZPST first-block offset exceeds footer");
    }
    let native_prefix = &bytes[..first];
    let mut blocks = Vec::new();
    let mut offset = first;
    let mut saw_auth = false;
    let mut count = 0usize;
    while offset < footer_start {
        count = count
            .checked_add(1)
            .ok_or_else(|| anyhow!("ZPST block count overflow"))?;
        if count > max_blocks || footer_start - offset < 8 {
            bail!("malformed ZPST block stream");
        }
        let tag: [u8; 4] = bytes[offset..offset + 4]
            .try_into()
            .expect("block tag length");
        let len = u32::from_le_bytes(
            bytes[offset + 4..offset + 8]
                .try_into()
                .expect("block length"),
        ) as usize;
        let payload_start = offset
            .checked_add(8)
            .ok_or_else(|| anyhow!("ZPST offset overflow"))?;
        let end = payload_start
            .checked_add(len)
            .ok_or_else(|| anyhow!("ZPST block length overflow"))?;
        if end > footer_start {
            bail!("truncated ZPST block");
        }
        if tag == ZPST_AUTH_TAG {
            if saw_auth
                || count == max_blocks
                || len != 32
                || end.checked_add(8) != Some(footer_start)
                || bytes[end..footer_start] != [b'E', b'N', b'D', b' ', 0, 0, 0, 0]
            {
                bail!("ZPST AUTH must be the 32-byte block immediately before END");
            }
            let expected_auth = Sha256::digest(&bytes[first..offset]);
            if bytes.get(payload_start..end) != Some(expected_auth.as_slice()) {
                bail!("ZPST authentication failed");
            }
            saw_auth = true;
            offset = footer_start;
            break;
        } else if tag == ZPST_END_TAG {
            bail!("ZPST END must follow AUTH");
        } else {
            if saw_auth {
                bail!("ZPST block follows AUTH");
            }
            blocks.push(ZpstBlock {
                tag,
                payload: &bytes[payload_start..end],
            });
        }
        offset = end;
    }
    if !saw_auth || offset != footer_start {
        bail!("ZPST stream is missing AUTH or END");
    }
    Ok(ZpstEnvelope {
        native_prefix,
        blocks,
    })
}

pub struct StateWriter {
    bytes: Vec<u8>,
}

impl Default for StateWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl StateWriter {
    pub fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(cap),
        }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn position(&self) -> usize {
        self.bytes.len()
    }

    pub fn write_u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub fn write_bool(&mut self, value: bool) {
        self.write_u8(u8::from(value));
    }

    pub fn write_u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_f64(&mut self, value: f64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    /// Write a length-prefixed byte slice (u32 length + raw bytes).
    pub fn write_vec(&mut self, data: &[u8]) {
        self.write_u32(data.len() as u32);
        self.write_bytes(data);
    }

    pub fn write_section(&mut self, write: impl FnOnce(&mut Self)) {
        let length_position = self.position();
        self.write_u32(0);
        let body_position = self.position();
        write(self);
        let length = u32::try_from(self.position() - body_position)
            .expect("save-state section exceeds u32 length");
        self.bytes[length_position..body_position].copy_from_slice(&length.to_le_bytes());
    }
}

pub struct StateReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> StateReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub fn is_exhausted(&self) -> bool {
        self.offset >= self.bytes.len()
    }

    pub fn position(&self) -> usize {
        self.offset
    }

    pub fn set_position(&mut self, pos: usize) {
        self.offset = pos;
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| anyhow!("save-state offset overflow"))?;
        if end > self.bytes.len() {
            bail!("save-state data is truncated");
        }
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    pub fn read_u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn read_bool(&mut self) -> Result<bool> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => bail!("invalid boolean value in save-state: {other}"),
        }
    }

    pub fn read_u16(&mut self) -> Result<u16> {
        let mut buf = [0u8; 2];
        buf.copy_from_slice(self.take(2)?);
        Ok(u16::from_le_bytes(buf))
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(buf))
    }

    pub fn read_u64(&mut self) -> Result<u64> {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(buf))
    }

    pub fn read_f64(&mut self) -> Result<f64> {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(self.take(8)?);
        Ok(f64::from_le_bytes(buf))
    }

    pub fn read_exact(&mut self, out: &mut [u8]) -> Result<()> {
        out.copy_from_slice(self.take(out.len())?);
        Ok(())
    }

    /// Read a length-prefixed byte vector, rejecting anything beyond `max_len`.
    pub fn read_vec(&mut self, max_len: usize) -> Result<Vec<u8>> {
        let len = self.read_u32()? as usize;
        if len > max_len {
            bail!("save-state vector length {len} exceeds maximum {max_len}");
        }
        Ok(self.take(len)?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_primitives() {
        let mut w = StateWriter::new();
        w.write_u8(0xAB);
        w.write_bool(true);
        w.write_bool(false);
        w.write_u16(0x1234);
        w.write_u32(0xDEADBEEF);
        w.write_u64(0x0102030405060708);
        w.write_f64(std::f64::consts::PI);

        let bytes = w.into_bytes();
        let mut r = StateReader::new(&bytes);
        assert_eq!(r.read_u8().unwrap(), 0xAB);
        assert!(r.read_bool().unwrap());
        assert!(!r.read_bool().unwrap());
        assert_eq!(r.read_u16().unwrap(), 0x1234);
        assert_eq!(r.read_u32().unwrap(), 0xDEADBEEF);
        assert_eq!(r.read_u64().unwrap(), 0x0102030405060708);
        assert_eq!(r.read_f64().unwrap(), std::f64::consts::PI);
        assert!(r.is_exhausted());
    }

    #[test]
    fn roundtrip_vec() {
        let mut w = StateWriter::new();
        w.write_vec(&[1, 2, 3, 4]);
        let bytes = w.into_bytes();
        let mut r = StateReader::new(&bytes);
        assert_eq!(r.read_vec(100).unwrap(), vec![1, 2, 3, 4]);
        assert!(r.is_exhausted());
    }

    #[test]
    fn read_vec_rejects_oversized() {
        let mut w = StateWriter::new();
        w.write_vec(&[0; 10]);
        let bytes = w.into_bytes();
        let mut r = StateReader::new(&bytes);
        assert!(r.read_vec(5).is_err());
    }

    #[test]
    fn truncated_data_errors() {
        let r_bytes = [0u8; 1];
        let mut r = StateReader::new(&r_bytes);
        assert!(r.read_u16().is_err());
    }

    #[test]
    fn invalid_bool_errors() {
        let mut r = StateReader::new(&[2]);
        assert!(r.read_bool().is_err());
    }

    #[test]
    fn position_tracks_writes() {
        let mut w = StateWriter::new();
        assert_eq!(w.position(), 0);
        w.write_u32(42);
        assert_eq!(w.position(), 4);
        w.write_bytes(&[1, 2, 3]);
        assert_eq!(w.position(), 7);
    }

    #[test]
    fn with_capacity_works() {
        let w = StateWriter::with_capacity(1024);
        assert_eq!(w.position(), 0);
    }

    #[test]
    fn direct_section_matches_buffered_format() {
        let mut buffered_inner = StateWriter::new();
        buffered_inner.write_u16(0x1234);
        buffered_inner.write_vec(&[5, 6, 7]);
        let mut buffered = StateWriter::new();
        buffered.write_u8(0xAB);
        buffered.write_vec(&buffered_inner.into_bytes());
        buffered.write_u8(0xCD);

        let mut direct = StateWriter::new();
        direct.write_u8(0xAB);
        direct.write_section(|section| {
            section.write_u16(0x1234);
            section.write_vec(&[5, 6, 7]);
        });
        direct.write_u8(0xCD);

        assert_eq!(direct.into_bytes(), buffered.into_bytes());
    }

    #[test]
    fn zpst_roundtrip_authenticates_blocks() {
        let mut writer = ZpstWriter::new(vec![1, 2, 3], 128, 4).unwrap();
        writer.push_block(*b"ONE ", vec![4]).unwrap();
        writer.push_block(*b"TWO ", vec![5, 6]).unwrap();
        let bytes = writer.finish().unwrap();

        let parsed = parse_zpst_envelope(&bytes, 128, 4).unwrap();
        assert_eq!(parsed.native_prefix, [1, 2, 3]);
        assert_eq!(parsed.blocks.len(), 2);
        assert_eq!(parsed.blocks[0].tag, *b"ONE ");
        assert_eq!(parsed.blocks[1].payload, [5, 6]);
    }

    #[test]
    fn zpst_rejects_tampered_authentication() {
        let mut writer = ZpstWriter::new(vec![1], 128, 3).unwrap();
        writer.push_block(*b"DATA", vec![2]).unwrap();
        let mut bytes = writer.finish().unwrap();
        let auth_payload = bytes.len() - 8 - 8 - 32 + 8;
        bytes[auth_payload] ^= 1;
        assert!(parse_zpst_envelope(&bytes, 128, 3).is_err());
    }
}
