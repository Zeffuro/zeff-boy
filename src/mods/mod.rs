#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ModEntry {
    pub(crate) filename: String,
    pub(crate) enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) target: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ModApplicationReport {
    pub(crate) warnings: Vec<String>,
    pub(crate) steps: Vec<ModApplicationStep>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ModApplicationStep {
    pub(crate) filename: String,
    pub(crate) format: String,
    pub(crate) patch_sha256: Option<String>,
    pub(crate) input_sha256: String,
    pub(crate) input_len: usize,
    pub(crate) output_sha256: String,
    pub(crate) output_len: usize,
    pub(crate) outcome: ModApplicationOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum ModApplicationOutcome {
    Applied,
    Failed { error: String },
}

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::*;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::*;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
