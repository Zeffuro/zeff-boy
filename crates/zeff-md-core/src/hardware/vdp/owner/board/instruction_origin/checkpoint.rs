use super::*;
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};

const MAX_CAPSULE_BYTES: usize = 256;

fn binding(board: &Board) -> Result<[u8; 32]> {
    let mut writer = StateWriter::new();
    board.write_state(&mut writer)?;
    Ok(Sha256::digest(writer.into_bytes()).into())
}

fn write_sample(writer: &mut StateWriter, sample: Option<Sample>) {
    writer.write_bool(sample.is_some());
    if let Some(sample) = sample {
        writer.write_u32(sample.key.request.access.logical_address);
        writer.write_u32(sample.key.request.access.bus_address);
        writer.write_u8(sample.key.request.access.function_code as u8);
        writer.write_u64(sample.key.start_half);
        writer.write_u16(sample.input.value);
        for origin in sample.input.origin {
            writer.write_u64(origin.storage_offset().unwrap());
        }
    }
}

fn read_sample(reader: &mut StateReader<'_>) -> Result<Option<Sample>> {
    if !reader.read_bool()? {
        return Ok(None);
    }
    let logical_address = reader.read_u32()?;
    let bus_address = reader.read_u32()?;
    let function_code = match reader.read_u8()? {
        2 => FunctionCode::UserProgram,
        6 => FunctionCode::SupervisorProgram,
        _ => anyhow::bail!("invalid origin request function code"),
    };
    let start_half = reader.read_u64()?;
    let value = reader.read_u16()?;
    let mut origin = [InstructionByteOrigin::UNKNOWN; 2];
    for byte in &mut origin {
        *byte = InstructionByteOrigin::storage(reader.read_u64()?)
            .ok_or_else(|| anyhow::anyhow!("origin offset cannot be encoded"))?;
    }
    Ok(Some(Sample {
        key: ReadKey {
            request: main_bus::Request {
                access: zeff_m68k::bus::BusAccess {
                    logical_address,
                    bus_address,
                    function_code,
                    lanes: ByteLanes::Both,
                    kind: zeff_m68k::bus::AccessKind::Prefetch,
                },
                operation: main_bus::Operation::Read,
            },
            start_half,
        },
        input: InstructionInput { value, origin },
    }))
}

impl Board {
    fn validate_origin_sample(&self, sample: Sample, storage_bytes: u64) -> Result<()> {
        let access = sample.key.request.access;
        ensure!(
            instruction_read(sample.key.request)
                && access.bus_address <= 0x3ffffe
                && access.logical_address & 0x00ff_ffff == access.bus_address
                && access.bus_address & 1 == 0,
            "invalid historical origin request"
        );
        ensure!(
            self.main
                .as_ref()
                .is_some_and(|main| main.origin_start_valid(sample.key.start_half)),
            "invalid historical origin request time"
        );
        let high = sample.input.origin[0].storage_offset();
        let low = sample.input.origin[1].storage_offset();
        ensure!(
            high.zip(low)
                .is_some_and(|(high, low)| high.checked_add(1) == Some(low) && low < storage_bytes),
            "invalid MAIN source offsets"
        );
        Ok(())
    }

    fn validate_origins(&self, origins: MainReadOrigin, storage_bytes: u64) -> Result<()> {
        if let Some(latch) = origins.latch {
            self.validate_origin_sample(latch, storage_bytes)?;
            ensure!(
                self.data_mask == 0xffff && self.data == latch.input.value,
                "origin latch does not bind held data"
            );
        }
        if let Some(sample) = origins.sampled {
            self.validate_origin_sample(sample, storage_bytes)?;
            let snapshot = self
                .snapshot()
                .ok_or_else(|| anyhow::anyhow!("origin sample has no MAIN sequencer"))?;
            let matches = |request, start, word, vpa: bool| {
                sample.key.request == request
                    && sample.key.start_half == start
                    && word == Some(sample.input.value)
                    && !vpa
            };
            let pending = snapshot.phase == main_bus::Phase::S7
                && snapshot.pending.is_some_and(|pending| {
                    matches(
                        pending.request,
                        pending.start_half,
                        pending.read_word,
                        pending.vpa.is_some(),
                    )
                });
            let completed = snapshot.completed.is_some_and(|receipt| {
                matches(
                    receipt.request,
                    receipt.start_half,
                    receipt.read_word,
                    receipt.vpa.is_some(),
                )
            });
            ensure!(
                self.protocol.is_none() && (pending || completed),
                "origin sample does not bind frozen MAIN read"
            );
        }
        Ok(())
    }

    pub(crate) fn capture_instruction_origins(&self, storage_bytes: u64) -> Result<Vec<u8>> {
        self.validate_origins(self.instruction_origin.0, storage_bytes)?;
        let mut writer = StateWriter::new();
        writer.write_u8(1);
        writer.write_bytes(&binding(self)?);
        write_sample(&mut writer, self.instruction_origin.0.latch);
        write_sample(&mut writer, self.instruction_origin.0.sampled);
        let bytes = writer.into_bytes();
        ensure!(
            bytes.len() <= MAX_CAPSULE_BYTES,
            "MAIN origin capsule is too large"
        );
        Ok(bytes)
    }

    pub(crate) fn attach_instruction_origins(
        &mut self,
        bytes: &[u8],
        storage_bytes: u64,
    ) -> Result<()> {
        ensure!(
            bytes.len() <= MAX_CAPSULE_BYTES,
            "MAIN origin capsule is too large"
        );
        let mut reader = StateReader::new(bytes);
        ensure!(
            reader.read_u8()? == 1,
            "unsupported MAIN origin capsule version"
        );
        let mut expected = [0; 32];
        reader.read_exact(&mut expected)?;
        ensure!(
            binding(self)? == expected,
            "MAIN origin hardware binding mismatch"
        );
        let origins = MainReadOrigin {
            latch: read_sample(&mut reader)?,
            sampled: read_sample(&mut reader)?,
        };
        ensure!(reader.is_exhausted(), "trailing MAIN origin capsule data");
        self.validate_origins(origins, storage_bytes)?;
        self.instruction_origin.0 = origins;
        Ok(())
    }
}
