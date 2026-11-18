use super::*;
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_m68k::timing::InstructionOriginCheckpoint;

const MAGIC: &[u8; 8] = b"ZMDDBG01";
const LEGACY_MAX_BYTES: usize = MAX_FRONTEND_STATE_BYTES + 1024;
const MAX_BYTES: usize = LEGACY_MAX_BYTES + call_stack::MAX_CALL_STACK_BYTES;

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn read_digest(reader: &mut StateReader<'_>) -> Result<[u8; 32]> {
    let mut bytes = [0; 32];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

impl Emulator {
    pub fn encode_debugger_checkpoint(&self) -> Result<Vec<u8>> {
        self.encode_debugger_checkpoint_version(2)
    }

    fn encode_debugger_checkpoint_version(&self, version: u32) -> Result<Vec<u8>> {
        ensure!(
            (1..=2).contains(&version),
            "unsupported debugger checkpoint version"
        );
        ensure!(
            !self.bus.audio.batch_active(),
            "debugger checkpoint during audio batch"
        );
        let storage_bytes = u64::try_from(self.bus.cartridge.bytes.len())?;
        let frontend = self.encode_frontend_state()?;
        let cpu = self.cpu.capture_instruction_origins(storage_bytes)?;
        let board = self.bus.capture_board_origins(storage_bytes)?;
        let mut capsule = StateWriter::new();
        cpu.write_owned(&mut capsule);
        let mut writer = StateWriter::new();
        writer.write_bytes(MAGIC);
        writer.write_u32(version);
        writer.write_bytes(&self.bus.cartridge.rom_digest());
        writer.write_bytes(&digest(&frontend));
        writer.write_vec(&frontend);
        writer.write_vec(&capsule.into_bytes());
        writer.write_bool(board.is_some());
        if let Some(board) = board {
            writer.write_vec(&board);
        }
        if version == 2 {
            writer.write_vec(&self.call_stack.write_owned()?);
        }
        let mut bytes = writer.into_bytes();
        let checksum = digest(&bytes);
        bytes.extend_from_slice(&checksum);
        ensure!(
            bytes.len()
                <= if version == 1 {
                    LEGACY_MAX_BYTES
                } else {
                    MAX_BYTES
                },
            "debugger checkpoint exceeds size bound"
        );
        Ok(bytes)
    }

    // Accept only caller-owned history; hashes bind records, not external source authenticity.
    pub fn decode_debugger_checkpoint(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            !self.bus.audio.batch_active(),
            "debugger restore during audio batch"
        );
        ensure!(
            bytes.len() <= MAX_BYTES,
            "debugger checkpoint exceeds size bound"
        );
        if bytes.starts_with(b"ZMDFRONT") {
            let mut candidate = self.clone();
            candidate.decode_frontend_state(bytes)?;
            ensure!(
                candidate.encode_frontend_state()? == bytes,
                "noncanonical legacy debugger checkpoint"
            );
            *self = candidate;
            return Ok(());
        }
        ensure!(bytes.len() >= 44, "truncated debugger checkpoint");
        let (record, checksum) = bytes.split_at(bytes.len() - 32);
        ensure!(
            checksum == &digest(record)[..],
            "debugger checkpoint integrity mismatch"
        );
        let mut reader = StateReader::new(record);
        let mut magic = [0; 8];
        reader.read_exact(&mut magic)?;
        let version = reader.read_u32()?;
        ensure!(
            &magic == MAGIC && (1..=2).contains(&version),
            "unsupported debugger checkpoint"
        );
        ensure!(
            version == 2 || bytes.len() <= LEGACY_MAX_BYTES,
            "legacy debugger checkpoint exceeds size bound"
        );
        ensure!(
            read_digest(&mut reader)? == self.bus.cartridge.rom_digest(),
            "debugger checkpoint immutable ROM mismatch"
        );
        let frontend_digest = read_digest(&mut reader)?;
        let frontend = reader.read_slice(MAX_FRONTEND_STATE_BYTES)?;
        ensure!(
            frontend.starts_with(b"ZMDFRONT") && digest(frontend) == frontend_digest,
            "debugger frontend payload binding mismatch"
        );
        let cpu = reader.read_slice(192)?;
        let board = if reader.read_bool()? {
            Some(reader.read_slice(256)?)
        } else {
            None
        };
        let stack_capsule = if version == 2 {
            Some(reader.read_slice(call_stack::MAX_CALL_STACK_BYTES)?)
        } else {
            None
        };
        ensure!(reader.is_exhausted(), "trailing debugger checkpoint data");
        let mut candidate = self.clone();
        candidate.decode_frontend_state(frontend)?;
        ensure!(
            candidate.encode_frontend_state()? == frontend,
            "noncanonical debugger frontend payload"
        );
        let storage_bytes = u64::try_from(candidate.bus.cartridge.bytes.len())?;
        let mut cpu_reader = StateReader::new(cpu);
        let cpu = InstructionOriginCheckpoint::read_owned(&mut cpu_reader, storage_bytes)?;
        ensure!(
            cpu_reader.is_exhausted(),
            "trailing CPU origin capsule data"
        );
        candidate
            .cpu
            .attach_instruction_origins(&cpu, storage_bytes)?;
        candidate.bus.attach_board_origins(board, storage_bytes)?;
        if let Some(bytes) = stack_capsule {
            candidate.call_stack = call_stack::Tracker::read_owned(bytes)?;
            candidate.cpu.set_control_flow_recording(
                candidate.call_stack_status() != CallStackStatus::Disabled,
            );
            candidate.cpu.clear_control_flow_events();
        }
        ensure!(
            candidate.encode_debugger_checkpoint_version(version)? == bytes,
            "noncanonical debugger checkpoint"
        );
        *self = candidate;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
