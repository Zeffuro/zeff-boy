use super::{ControlCommand, VdpMemory, VdpWriteFifo};
mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpReadError {
    FifoPending,
    BufferReady,
    Empty,
    UnknownIncrement,
    UnknownFifoHistory,
    PrefetchPending,
    PendingIncrement,
    UnsupportedIncrement(u8),
    AddressOverflow,
    UnsupportedCommand(ControlCommand),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VdpReadCache {
    data: Option<u16>,
    ready: bool,
}

impl VdpReadCache {
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn buffered_word(&self) -> Option<u16> {
        self.data
    }

    /// Settled control/data writes clear readiness, retaining the buffered word.
    pub fn invalidate(&mut self) {
        self.ready = false;
    }

    /// Publishes a completed fetch after FIFO drain; byte-slot timing is separate.
    pub fn complete_read(
        &mut self,
        memory: &VdpMemory,
        fifo: &VdpWriteFifo,
        command: &mut ControlCommand,
        increment: Option<u8>,
    ) -> Result<u16, VdpReadError> {
        if self.ready {
            return Err(VdpReadError::BufferReady);
        }
        if !fifo.is_empty() {
            return Err(VdpReadError::FifoPending);
        }
        let data = memory.read_word_with_history(*command, fifo.next_slot_data())?;
        let increment = increment.ok_or(VdpReadError::UnknownIncrement)?;
        self.complete_word(data, command, increment);
        Ok(data)
    }

    pub(super) fn complete_word(&mut self, data: u16, command: &mut ControlCommand, increment: u8) {
        self.data = Some(data);
        self.ready = true;
        command.address = command.next_address(increment);
    }

    pub fn read(&mut self) -> Result<u16, VdpReadError> {
        if !self.ready {
            return Err(VdpReadError::Empty);
        }
        self.ready = false;
        Ok(self.data.unwrap())
    }
}
