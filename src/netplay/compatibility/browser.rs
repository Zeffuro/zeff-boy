use anyhow::{Context, Result, ensure};
use zeff_netplay::wire::BuildInfo;

#[path = "certificate.rs"]
mod certificate;
#[path = "content.rs"]
mod content;
mod certificate_fp {
    pub(super) const REQUIRED_CONTROLS: u32 = 0x1f80;
}

fn global(name: &str) -> Result<wasm_bindgen::JsValue> {
    js_sys::Reflect::get(&js_sys::global(), &name.into())
        .map_err(|_| anyhow::anyhow!("Browser pair metadata unavailable"))
}

fn local_pair() -> Result<Option<certificate::VerifiedPair>> {
    let error = global("zeffBoyPairCertificateError")?;
    ensure!(
        error.is_null() || error.is_undefined(),
        "Browser pair certificate unavailable"
    );
    let value = global("zeffBoyPairCertificate")?;
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    let bytes = value
        .as_string()
        .context("Invalid browser pair certificate")?;
    let facts = certificate::compiled_facts(super::connect::executable_build()?);
    certificate::verify(bytes.as_bytes(), &facts).map(Some)
}

pub(crate) fn describe(
    backend: &crate::emu_backend::EmuBackend,
    allow_different_versions: bool,
) -> BuildInfo {
    let mut result = BuildInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        allow_different_versions,
        ..BuildInfo::default()
    };
    if let Ok(Some(pair)) = local_pair()
        && content::permitted(&pair, backend)
    {
        let summary = pair.summary();
        result.contract = summary.contract;
        result.certificate = summary.certificate;
        result.peer_build = summary.peer_build;
        result.platform = 3;
    }
    result
}

pub(crate) fn available() -> bool {
    local_pair()
        .ok()
        .flatten()
        .is_some_and(|pair| pair.different_versions())
}

pub(crate) fn signed_pair_available() -> bool {
    local_pair().ok().flatten().is_some()
}

pub(crate) fn certificate_status() -> Result<Option<certificate::CertificateSummary>> {
    local_pair().map(|pair| pair.map(|pair| pair.summary()))
}

#[cfg(all(test, feature = "wasm-browser-tests"))]
#[path = "browser_tests.rs"]
mod tests;
