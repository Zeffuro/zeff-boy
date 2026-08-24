use super::DebugEvent;

pub const MIN_TRACE_CAPACITY: usize = 1_000;
pub const MAX_TRACE_CAPACITY: usize = 100_000;
pub const MAX_TRACE_INSTRUCTION_BYTES: usize = 16;
pub const MAX_TRACE_REGISTER_DELTAS: usize = 16;
pub const MAX_TRACE_WRITES: usize = 16;

pub type TraceEntry = InstructionTraceRecord;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum TraceExecMode {
    #[default]
    Sm83,
    Arm,
    Thumb,
    Mos6502,
    Z80,
    V30,
    HuC6280,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegisterDelta {
    pub register: u8,
    pub value: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum TraceWriteWidth {
    #[default]
    Byte = 1,
    Halfword = 2,
    Word = 4,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum TraceWriteKind {
    #[default]
    Memory,
    Io,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TraceWrite {
    pub address: u32,
    pub old_value: u32,
    pub new_value: u32,
    pub width: TraceWriteWidth,
    pub kind: TraceWriteKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstructionTraceRecord {
    pub sequence: u64,
    pub frame: u64,
    pub cycle: u64,
    pub physical_rom_offset: Option<u64>,
    pub bank: Option<u32>,
    pub pc: u32,
    pub mode: TraceExecMode,
    pub instruction: [u8; MAX_TRACE_INSTRUCTION_BYTES],
    pub instruction_len: u8,
    pub event: Option<DebugEvent>,
    pub register_deltas: [RegisterDelta; MAX_TRACE_REGISTER_DELTAS],
    pub register_delta_len: u8,
    pub register_delta_overflow: u16,
    pub writes: [TraceWrite; MAX_TRACE_WRITES],
    pub write_len: u8,
    pub write_overflow: u16,
}

impl Default for InstructionTraceRecord {
    fn default() -> Self {
        Self::new(TraceExecMode::Sm83, 0, None, 0, 0, &[])
    }
}

impl InstructionTraceRecord {
    #[inline]
    pub fn new(
        mode: TraceExecMode,
        pc: u32,
        physical_rom_offset: Option<u64>,
        frame: u64,
        cycle: u64,
        instruction: &[u8],
    ) -> Self {
        let mut record = Self {
            sequence: 0,
            frame,
            cycle,
            physical_rom_offset,
            bank: None,
            pc,
            mode,
            instruction: [0; MAX_TRACE_INSTRUCTION_BYTES],
            instruction_len: 0,
            event: None,
            register_deltas: [RegisterDelta::default(); MAX_TRACE_REGISTER_DELTAS],
            register_delta_len: 0,
            register_delta_overflow: 0,
            writes: [TraceWrite::default(); MAX_TRACE_WRITES],
            write_len: 0,
            write_overflow: 0,
        };
        record.set_instruction(instruction);
        record
    }

    #[inline]
    pub fn set_instruction(&mut self, instruction: &[u8]) -> bool {
        let len = instruction.len().min(MAX_TRACE_INSTRUCTION_BYTES);
        self.instruction[..len].copy_from_slice(&instruction[..len]);
        self.instruction[len..].fill(0);
        self.instruction_len = len as u8;
        instruction.len() > MAX_TRACE_INSTRUCTION_BYTES
    }

    pub fn instruction_bytes(&self) -> &[u8] {
        &self.instruction[..usize::from(self.instruction_len)]
    }

    #[inline]
    pub fn push_register_delta(&mut self, delta: RegisterDelta) -> bool {
        let len = usize::from(self.register_delta_len);
        if len == MAX_TRACE_REGISTER_DELTAS {
            self.register_delta_overflow = self.register_delta_overflow.saturating_add(1);
            return false;
        }
        self.register_deltas[len] = delta;
        self.register_delta_len += 1;
        true
    }

    #[inline]
    pub fn push_write(&mut self, write: TraceWrite) -> bool {
        let len = usize::from(self.write_len);
        if len == MAX_TRACE_WRITES {
            self.write_overflow = self.write_overflow.saturating_add(1);
            return false;
        }
        self.writes[len] = write;
        self.write_len += 1;
        true
    }

    pub fn register_deltas(&self) -> &[RegisterDelta] {
        &self.register_deltas[..usize::from(self.register_delta_len)]
    }

    pub fn writes(&self) -> &[TraceWrite] {
        &self.writes[..usize::from(self.write_len)]
    }
}

pub struct InstructionTraceStore {
    entries: Vec<InstructionTraceRecord>,
    capacity: usize,
    start: usize,
    next_sequence: u64,
    enabled: bool,
}

impl std::fmt::Debug for InstructionTraceStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstructionTraceStore")
            .field("capacity", &self.capacity)
            .field("len", &self.entries.len())
            .field("next_sequence", &self.next_sequence)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Default for InstructionTraceStore {
    fn default() -> Self {
        Self::new(MIN_TRACE_CAPACITY)
    }
}

impl InstructionTraceStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity: clamp_capacity(capacity),
            start: 0,
            next_sequence: 0,
            enabled: false,
        }
    }

    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if enabled && self.entries.capacity() < self.capacity {
            self.entries
                .reserve_exact(self.capacity - self.entries.capacity());
        }
    }

    pub fn set_capacity(&mut self, capacity: usize) {
        let capacity = clamp_capacity(capacity);
        if capacity == self.capacity {
            return;
        }

        self.capacity = capacity;
        if self.entries.len() <= capacity {
            if self.enabled && self.entries.capacity() < capacity {
                self.entries
                    .reserve_exact(capacity - self.entries.capacity());
            }
            return;
        }

        let keep = capacity;
        let skip = self.entries.len() - keep;
        let mut entries = Vec::with_capacity(capacity);
        entries.extend(self.iter().skip(skip).copied());
        self.entries = entries;
        self.start = 0;
    }

    #[inline]
    pub fn push(&mut self, mut record: InstructionTraceRecord) -> Option<u64> {
        if !self.enabled {
            return None;
        }

        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        record.sequence = sequence;
        if self.entries.len() < self.capacity {
            self.entries.push(record);
        } else {
            self.entries[self.start] = record;
            self.start = (self.start + 1) % self.capacity;
        }
        Some(sequence)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.start = 0;
    }

    pub fn oldest_sequence(&self) -> Option<u64> {
        self.entries
            .first()
            .map(|_| self.entries[self.start].sequence)
    }

    pub fn newest_sequence(&self) -> Option<u64> {
        self.entries.last().map(|_| {
            self.entries[(self.start + self.entries.len() - 1) % self.entries.len()].sequence
        })
    }

    pub fn entries_after(&self, last_seen_sequence: Option<u64>, max: usize) -> Vec<TraceEntry> {
        if max == 0 {
            return Vec::new();
        }

        let mut entries = Vec::with_capacity(max.min(self.entries.len()));
        for entry in self.iter() {
            if last_seen_sequence.is_none_or(|sequence| sequence_is_after(entry.sequence, sequence))
            {
                entries.push(*entry);
                if entries.len() == max {
                    break;
                }
            }
        }
        entries
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &InstructionTraceRecord> {
        (0..self.entries.len())
            .map(move |index| &self.entries[(self.start + index) % self.entries.len()])
    }
}

const fn clamp_capacity(capacity: usize) -> usize {
    if capacity < MIN_TRACE_CAPACITY {
        MIN_TRACE_CAPACITY
    } else if capacity > MAX_TRACE_CAPACITY {
        MAX_TRACE_CAPACITY
    } else {
        capacity
    }
}

const fn sequence_is_after(sequence: u64, cursor: u64) -> bool {
    sequence != cursor && sequence.wrapping_sub(cursor) < (1_u64 << 63)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_after_handles_sequence_wrap() {
        let mut trace = InstructionTraceStore::new(MIN_TRACE_CAPACITY);
        trace.set_enabled(true);
        trace.next_sequence = u64::MAX - 1;
        trace.push(InstructionTraceRecord::default());
        trace.push(InstructionTraceRecord::default());
        trace.push(InstructionTraceRecord::default());

        assert_eq!(
            trace
                .entries_after(Some(u64::MAX - 1), 3)
                .iter()
                .map(|entry| entry.sequence)
                .collect::<Vec<_>>(),
            [u64::MAX, 0]
        );
    }
}
