#[cfg(not(target_arch = "wasm32"))]
mod autosave;
mod branch_diff;
mod edit;
#[cfg(not(target_arch = "wasm32"))]
mod editor_autosave;
#[cfg(not(target_arch = "wasm32"))]
mod editor_history;
#[cfg(not(target_arch = "wasm32"))]
mod editor_session;
#[cfg(not(target_arch = "wasm32"))]
mod execution;
mod format;
mod identity;
mod input_pattern;
mod model;
#[cfg(not(target_arch = "wasm32"))]
mod seek_cache;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod verification;
#[cfg(not(target_arch = "wasm32"))]
mod zrpl;

#[cfg(not(target_arch = "wasm32"))]
pub use autosave::*;
pub use branch_diff::*;
pub use edit::*;
#[cfg(not(target_arch = "wasm32"))]
pub use editor_autosave::*;
#[cfg(not(target_arch = "wasm32"))]
pub use editor_session::*;
#[cfg(not(target_arch = "wasm32"))]
#[allow(unused_imports)]
pub(crate) use execution::{
    MAX_EDITOR_SEEK_EXECUTION_FRAMES, TasEditorExecutionAttachment, TasEditorExecutionEngine,
    TasEditorExecutionOutcome, TasEditorExecutionProvider, TasEditorExecutionUnavailableReason,
    TasEditorFramebuffer,
};
pub use input_pattern::*;
pub use model::*;
#[cfg(not(target_arch = "wasm32"))]
pub use seek_cache::*;
#[cfg(not(target_arch = "wasm32"))]
pub use verification::TasExecutionWitness;
#[cfg(not(target_arch = "wasm32"))]
pub use zrpl::TasZrplImportWitness;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
