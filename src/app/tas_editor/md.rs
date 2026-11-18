use super::App;

impl App {
    pub(in crate::app) fn loaded_md_tas_loader(
        &self,
    ) -> anyhow::Result<crate::emu_backend::loader::PrivateTasExecutionLoader> {
        anyhow::ensure!(
            self.active_system == crate::emu_backend::ActiveSystem::Md,
            "MD TAS requires the loaded MD game"
        );
        let profile = self.rom_info.md_tas_profile.ok_or_else(|| {
            anyhow::anyhow!("Reload an eligible MD game before creating a TAS project")
        })?;
        let source = self
            .rom_info
            .source_path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("MD loaded source is missing"))?;
        crate::emu_backend::loader::md_product::select_md_tas_loader(
            source,
            self.rom_info.rom_path.clone(),
            profile,
        )
    }
}
