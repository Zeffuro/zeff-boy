#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ModEntry {
    pub(crate) filename: String,
    pub(crate) enabled: bool,
}

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::*;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::*;

#[cfg(test)]
mod tests;
