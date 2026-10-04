use anyhow::Result;
use zeff_netplay::wire::BuildInfo;

pub(crate) fn describe(_: &crate::emu_backend::EmuBackend, _: bool) -> BuildInfo {
    BuildInfo::default()
}
pub(crate) fn available() -> bool {
    false
}
pub(crate) fn signed_pair_available() -> bool {
    false
}
pub(crate) fn certificate_status() -> Result<()> {
    Ok(())
}
