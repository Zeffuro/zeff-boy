use super::*;

impl SaveMemory {
    pub fn write_state(&self, writer: &mut StateWriter) {
        self.write_bounded_bytes_tag(writer);
        self.write_expansion_bytes_tag(writer);
        writer.write_bool(self.enabled);
        writer.write_bytes(self.bytes.as_ref());
        if let Some(serial) = &self.serial {
            serial.write_state(writer);
        }
        {
            writer.write_bool(self.write_protected);
        }
    }

    pub fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        self.read_state_inner(reader, false)
    }

    pub(crate) fn read_state_with_fixed_latch(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<()> {
        self.read_state_inner(reader, true)
    }

    fn read_state_inner(&mut self, reader: &mut StateReader<'_>, fixed_latch: bool) -> Result<()> {
        self.read_bounded_bytes_tag(reader)?;
        self.read_expansion_bytes_tag(reader)?;
        self.enabled = reader.read_bool()?;
        reader.read_exact(self.bytes.as_mut())?;
        if let Some(serial) = &mut self.serial {
            serial.read_state(reader)?;
        }
        self.write_protected = reader.read_bool()?;
        if fixed_latch {
            self.validate_state_with_fixed_latch()
        } else {
            self.validate_state()
        }
    }
}
