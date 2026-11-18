use super::{Core, FLAG_N, FLAG_Z, FLAGS, Fault};

impl Core {
    pub(super) fn require_a(&self, mask: u32) -> Result<(), Fault> {
        let missing = mask & !self.a_known;
        if missing != 0 {
            return Err(Fault::UnknownAccumulator(missing));
        }
        Ok(())
    }

    pub(super) fn assign_a(&mut self, value: u32, known: u32) {
        self.a = value & known;
        self.a_known = known;
    }

    pub(super) fn result_flags(&mut self, value: u32, known: u32) {
        self.st &= !FLAGS;
        self.flags_known = 0;
        if known & 0x80000000 != 0 {
            self.flags_known |= FLAG_N;
            if value & 0x80000000 != 0 {
                self.st |= FLAG_N;
            }
        }
        if value & known != 0 {
            self.flags_known |= FLAG_Z;
        } else if known == u32::MAX {
            self.flags_known |= FLAG_Z;
            self.st |= FLAG_Z;
        }
    }
}
