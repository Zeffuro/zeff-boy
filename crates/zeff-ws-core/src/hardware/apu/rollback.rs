use super::Apu;
use sha2::{Digest, Sha256};

impl Apu {
    pub(crate) fn hash_rollback_runtime(&self, hash: &mut Sha256) {
        hash.update(self.sample_rate.to_le_bytes());
        hash.update([u8::from(self.sample_generation_enabled)]);
        hash.update(self.sample_cycle_accumulator.to_le_bytes());
        hash.update(self.channel_mutes.map(u8::from));
        hash_samples(hash, self.sample_buffer.iter());
        hash_samples(hash, self.debug_master_samples.iter());
        for samples in &self.debug_channel_samples {
            hash_samples(hash, samples.iter());
        }
    }

    pub(crate) fn rollback_runtime_bytes(&self) -> usize {
        (self.sample_buffer.capacity()
            + self.debug_master_samples.capacity()
            + self
                .debug_channel_samples
                .iter()
                .map(|samples| samples.capacity())
                .sum::<usize>())
            * std::mem::size_of::<f32>()
    }
}

fn hash_samples<'a>(hash: &mut Sha256, samples: impl ExactSizeIterator<Item = &'a f32>) {
    hash.update((samples.len() as u64).to_le_bytes());
    for sample in samples {
        hash.update(sample.to_bits().to_le_bytes());
    }
}
