use super::{Cpu, CpuTrap};
use sha2::{Digest, Sha256};

impl Cpu {
    pub(crate) fn hash_rollback_runtime(&self, hash: &mut Sha256) {
        hash.update([
            u8::from(self.last_step_was_interrupt),
            u8::from(self.last_mul_overflow),
            self.instruction_len,
        ]);
        hash.update(self.instruction_bytes);
        if let Some(fetch) = self.last_fetch {
            hash.update([1]);
            hash.update(fetch.cs.to_le_bytes());
            hash.update(fetch.ip.to_le_bytes());
            hash.update(fetch.pc.to_le_bytes());
            hash.update([fetch.opcode]);
            hash.update(fetch.cycles.to_le_bytes());
        } else {
            hash.update([0]);
        }
        match self.last_trap {
            None => hash.update([0]),
            Some(CpuTrap::UnsupportedOpcode { cs, ip, opcode }) => {
                hash.update([1, opcode]);
                hash.update(cs.to_le_bytes());
                hash.update(ip.to_le_bytes());
            }
            Some(CpuTrap::UnsupportedInstructionForm {
                cs,
                ip,
                opcode,
                modrm,
            }) => {
                hash.update([2, opcode, modrm]);
                hash.update(cs.to_le_bytes());
                hash.update(ip.to_le_bytes());
            }
            Some(CpuTrap::DivideError {
                cs,
                ip,
                opcode,
                modrm,
            }) => {
                hash.update([3, opcode, modrm]);
                hash.update(cs.to_le_bytes());
                hash.update(ip.to_le_bytes());
            }
        }
    }
}
