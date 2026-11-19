use super::*;
use crate::timing::PrefetchCpu;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

const MAX_BINDING_BYTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstructionOriginCheckpoint {
    boundary: Vec<u8>,
    queue: Option<[[InstructionByteOrigin; 2]; 2]>,
}

fn boundary(cpu: &PrefetchCpu) -> Result<Vec<u8>> {
    let mut writer = StateWriter::new();
    cpu.write_boundary_state(&mut writer)?;
    let bytes = writer.into_bytes();
    ensure!(
        bytes.len() <= MAX_BINDING_BYTES,
        "CPU origin binding is too large"
    );
    Ok(bytes)
}

fn validate(origin: InstructionByteOrigin, storage_bytes: u64) -> Result<()> {
    ensure!(
        origin
            .storage_offset()
            .is_none_or(|offset| offset < storage_bytes),
        "instruction source exceeds immutable storage"
    );
    Ok(())
}

impl InstructionOriginCheckpoint {
    pub fn write_owned(&self, writer: &mut StateWriter) {
        writer.write_u8(1);
        writer.write_vec(&self.boundary);
        writer.write_bool(self.queue.is_some());
        if let Some(queue) = self.queue {
            for origin in queue.into_iter().flatten() {
                writer.write_u64(if origin == InstructionByteOrigin::UNKNOWN {
                    0
                } else if origin == InstructionByteOrigin::NON_STORAGE {
                    1
                } else {
                    origin.storage_offset().unwrap() + 2
                });
            }
        }
    }

    pub fn read_owned(reader: &mut StateReader<'_>, storage_bytes: u64) -> Result<Self> {
        ensure!(
            reader.read_u8()? == 1,
            "unsupported CPU origin checkpoint version"
        );
        let boundary = reader.read_slice(MAX_BINDING_BYTES)?.to_vec();
        let queue = if reader.read_bool()? {
            let mut queue = [[InstructionByteOrigin::UNKNOWN; 2]; 2];
            for origin in queue.iter_mut().flatten() {
                *origin = match reader.read_u64()? {
                    0 => InstructionByteOrigin::UNKNOWN,
                    1 => InstructionByteOrigin::NON_STORAGE,
                    value => InstructionByteOrigin::storage(value - 2).unwrap(),
                };
                validate(*origin, storage_bytes)?;
            }
            Some(queue)
        } else {
            None
        };
        Ok(Self { boundary, queue })
    }
}

impl PrefetchCpu {
    pub fn capture_instruction_origins(
        &self,
        storage_bytes: u64,
    ) -> Result<InstructionOriginCheckpoint> {
        let queue = self
            .pipeline
            .map(|pipeline| [pipeline.input(0).origin, pipeline.input(1).origin]);
        if let Some(queue) = queue {
            for origin in queue.into_iter().flatten() {
                validate(origin, storage_bytes)?;
            }
        }
        Ok(InstructionOriginCheckpoint {
            boundary: boundary(self)?,
            queue,
        })
    }

    pub fn attach_instruction_origins(
        &mut self,
        checkpoint: &InstructionOriginCheckpoint,
        storage_bytes: u64,
    ) -> Result<()> {
        ensure!(
            boundary(self)? == checkpoint.boundary,
            "CPU origin checkpoint boundary mismatch"
        );
        ensure!(
            self.pipeline.is_some() == checkpoint.queue.is_some(),
            "CPU origin queue presence mismatch"
        );
        if let Some(queue) = checkpoint.queue {
            for origin in queue.into_iter().flatten() {
                validate(origin, storage_bytes)?;
            }
            let pipeline = self.pipeline.unwrap();
            self.pipeline = Some(Pipeline::sampled(
                pipeline.address,
                [
                    InstructionInput {
                        value: pipeline.words[0],
                        origin: queue[0],
                    },
                    InstructionInput {
                        value: pipeline.words[1],
                        origin: queue[1],
                    },
                ],
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
