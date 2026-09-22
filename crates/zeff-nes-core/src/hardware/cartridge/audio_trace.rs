use super::{Cartridge, dispatch::MapperImpl};

impl Cartridge {
    pub(crate) fn supports_base_audio_trace(&self) -> bool {
        matches!(
            self.mapper,
            MapperImpl::Nrom(_)
                | MapperImpl::Mmc1(_)
                | MapperImpl::Uxrom(_)
                | MapperImpl::Cnrom(_)
                | MapperImpl::Mmc3(_)
                | MapperImpl::Axrom(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_admission_uses_the_loaded_mapper_instead_of_the_raw_header() {
        let mut rom = vec![0; 16 + 0x8000 + 0x2000];
        rom[..4].copy_from_slice(b"NES\x1a");
        rom[4] = 2;
        rom[5] = 1;
        rom[6] = 0x50;
        let mut cartridge = Cartridge::load(&rom).unwrap();
        cartridge.header.mapper_id = 1;
        assert!(!cartridge.supports_base_audio_trace());
    }
}
