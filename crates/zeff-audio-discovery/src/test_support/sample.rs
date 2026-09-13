use crate::{EngineProfile, SampleInventory, sample::SampleError};

pub fn read_sample_inventory(
    bytes: &[u8],
    header_offset: usize,
    kind: u8,
    profile: EngineProfile,
) -> Result<SampleInventory, SampleError> {
    crate::sample::read_sample_inventory(bytes, header_offset, kind, profile)
}
