use super::{FetchWord, StateReader, StateWriter};
use anyhow::{Result, ensure};

const WORD_MASK: u64 = (1 << 34) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReadWord {
    value: u64,
    known: u64,
}

impl ReadWord {
    const UNKNOWN: Self = Self { value: 0, known: 0 };

    fn known(word: FetchWord) -> Self {
        Self {
            value: u64::from(word.pattern)
                | u64::from(word.x) << 11
                | u64::from(word.flip) << 20
                | u64::from(word.palette) << 21
                | u64::from(word.priority) << 23
                | u64::from(word.width) << 24
                | u64::from(word.height) << 26
                | u64::from(word.row) << 28,
            known: WORD_MASK,
        }
    }

    fn and(self, other: Self) -> Self {
        let value = self.value & other.value;
        let zero = (self.known & !self.value) | (other.known & !other.value);
        Self {
            value,
            known: (zero | value) & WORD_MASK,
        }
    }

    fn decode(self) -> Option<FetchWord> {
        (self.known == WORD_MASK).then_some(FetchWord {
            pattern: (self.value & 2047) as u16,
            x: ((self.value >> 11) & 511) as u16,
            flip: self.value & (1 << 20) != 0,
            palette: ((self.value >> 21) & 3) as u8,
            priority: self.value & (1 << 23) != 0,
            width: ((self.value >> 24) & 3) as u8,
            height: ((self.value >> 26) & 3) as u8,
            row: ((self.value >> 28) & 63) as u8,
        })
    }

    fn write(self, writer: &mut StateWriter) {
        writer.write_u64(self.value);
        writer.write_u64(self.known);
    }

    fn read(reader: &mut StateReader<'_>) -> Result<Self> {
        let word = Self {
            value: reader.read_u64()?,
            known: reader.read_u64()?,
        };
        ensure!(
            word.known & !WORD_MASK == 0 && word.value & !word.known == 0,
            "noncanonical descriptor read word"
        );
        Ok(word)
    }
}

#[derive(Clone, Copy)]
pub(super) struct DescriptorWrite {
    pub pattern: bool,
    pub x: bool,
    pub attributes: bool,
    pub word: FetchWord,
}

impl DescriptorWrite {
    fn apply(self, mut old: FetchWord) -> FetchWord {
        if self.pattern {
            old.pattern = self.word.pattern;
        }
        if self.x {
            old.x = self.word.x;
        }
        if self.attributes {
            old.flip = self.word.flip;
            old.palette = self.word.palette;
            old.priority = self.word.priority;
            old.width = self.word.width;
            old.height = self.word.height;
            old.row = self.word.row;
        }
        old
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Edge {
    common: ReadWord,
    parity: [ReadWord; 2],
    write: Option<(u8, FetchWord)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FunctionalReadResponder {
    words: [FetchWord; 20],
    common: ReadWord,
    parity: [ReadWord; 2],
    pending: Option<Edge>,
}

impl FunctionalReadResponder {
    pub fn software_origin() -> Self {
        Self {
            words: [FetchWord::default(); 20],
            common: ReadWord::UNKNOWN,
            parity: [ReadWord::UNKNOWN; 2],
            pending: None,
        }
    }

    pub fn begin_edge(
        &mut self,
        index: u8,
        horizontal_master: bool,
        write: DescriptorWrite,
    ) -> Result<()> {
        ensure!(self.pending.is_none(), "descriptor edge already pending");
        ensure!(index < 32, "invalid descriptor read address");
        let mut edge = Edge {
            common: self.common,
            parity: self.parity,
            write: None,
        };
        let lane = usize::from(index & 1);
        if let Some(word) = self.words.get(usize::from(index)).copied() {
            // Read outputs consume the backing word before the same-edge write.
            edge.common = ReadWord::known(word);
            edge.parity[lane] = edge.common;
            if horizontal_master && (write.pattern || write.x || write.attributes) {
                edge.write = Some((index, write.apply(word)));
            }
        } else {
            edge.common = self.common.and(self.parity[lane]);
        }
        self.pending = Some(edge);
        Ok(())
    }

    pub fn finish_edge(&mut self) -> Result<bool> {
        let edge = self
            .pending
            .take()
            .ok_or_else(|| anyhow::anyhow!("descriptor edge was not prepared"))?;
        let changed = self.common != edge.common;
        if let Some((index, word)) = edge.write {
            self.words[usize::from(index)] = word;
        }
        self.common = edge.common;
        self.parity = edge.parity;
        Ok(changed)
    }

    pub fn selected(&self) -> Result<FetchWord> {
        self.common.decode().ok_or_else(|| {
            anyhow::anyhow!(
                "unknown functional descriptor read bits known={:09x} value={:09x}",
                self.common.known,
                self.common.value
            )
        })
    }

    pub fn validate_frontier(&self) -> Result<()> {
        ensure!(self.pending.is_none(), "incomplete descriptor MCLK edge");
        Ok(())
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(1);
        for word in self.words {
            word.write(writer);
        }
        self.common.write(writer);
        for word in self.parity {
            word.write(writer);
        }
        writer.write_bool(self.pending.is_some());
        if let Some(edge) = self.pending {
            edge.common.write(writer);
            for word in edge.parity {
                word.write(writer);
            }
            writer.write_bool(edge.write.is_some());
            if let Some((index, word)) = edge.write {
                writer.write_u8(index);
                word.write(writer);
            }
        }
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        ensure!(
            reader.read_u8()? == 1,
            "unsupported descriptor read profile"
        );
        let mut state = Self::software_origin();
        for word in &mut state.words {
            *word = FetchWord::read(reader)?;
        }
        state.common = ReadWord::read(reader)?;
        state.parity = [ReadWord::read(reader)?, ReadWord::read(reader)?];
        ensure!(
            !reader.read_bool()?,
            "incomplete saved descriptor MCLK edge"
        );
        Ok(state)
    }
}
