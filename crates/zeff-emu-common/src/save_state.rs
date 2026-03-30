use anyhow::{Result, anyhow, bail};

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
}

