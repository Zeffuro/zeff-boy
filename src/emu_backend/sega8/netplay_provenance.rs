use super::Sega8Backend;

#[derive(Clone, Copy)]
pub(crate) struct Sega8NetplayLoadProvenance {
    pub(crate) source: [u8; 32],
    pub(crate) source_len: u64,
    pub(crate) direct_source: bool,
    pub(crate) unmodified: bool,
    pub(crate) neutral_input: bool,
    pub(crate) state: [u8; 32],
    pub(crate) runtime: [u8; 32],
    pub(crate) persistent: [u8; 32],
    pub(crate) sample_rate: u32,
}

impl Sega8Backend {
    pub(crate) fn capture_netplay_load_provenance(
        &mut self,
        source: [u8; 32],
        source_len: u64,
        direct_source: bool,
        unmodified: bool,
        neutral_input: bool,
    ) -> anyhow::Result<()> {
        let extension = self
            .paths
            .rom_path()
            .extension()
            .and_then(|value| value.to_str());
        let expected = match self.system() {
            crate::emu_backend::ActiveSystem::MasterSystem => "sms",
            crate::emu_backend::ActiveSystem::Sg1000 => "sg",
            _ => return Ok(()),
        };
        self.netplay_load_provenance = Some(Sega8NetplayLoadProvenance {
            source,
            source_len,
            direct_source: direct_source
                && extension.is_some_and(|ext| ext.eq_ignore_ascii_case(expected)),
            unmodified,
            neutral_input,
            state: zeff_firmware::sha256_bytes(&self.emu.encode_state()?),
            runtime: zeff_firmware::sha256_bytes(&self.emu.encode_rollback_runtime_state()),
            persistent: zeff_firmware::sha256_bytes(self.emu.bus().cartridge_ram_visible()),
            sample_rate: self.emu.sample_rate(),
        });
        Ok(())
    }

    pub(crate) fn netplay_load_provenance(&self) -> Option<Sega8NetplayLoadProvenance> {
        self.netplay_load_provenance
    }
    pub(crate) fn host_persistence_enabled(&self) -> bool {
        self.host_persistence_enabled
    }
    pub(crate) fn set_host_persistence_enabled(&mut self, enabled: bool) {
        self.host_persistence_enabled = enabled;
    }
}
