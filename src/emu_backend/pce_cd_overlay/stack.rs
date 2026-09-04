use std::path::Path;

use super::{PatchOverlayApply, PatchOverlayBuilder, PatchOverlayStack};

pub(crate) fn apply_ppf_stack(
    builder: &mut PatchOverlayBuilder,
    dir: &Path,
    mods: &[crate::mods::ModEntry],
) -> PatchOverlayStack {
    let enabled = mods
        .iter()
        .filter(|entry| entry.enabled)
        .collect::<Vec<_>>();
    if enabled.iter().any(|entry| {
        !Path::new(&entry.filename)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ppf"))
    }) {
        return PatchOverlayStack::Fallback;
    }
    let mut patches = Vec::with_capacity(enabled.len());
    for entry in enabled {
        let Some(patch) = super::read_patch(&dir.join(&entry.filename)) else {
            return PatchOverlayStack::Fallback;
        };
        patches.push((entry.filename.clone(), patch));
    }
    apply_ppf_bytes_stack(builder, &patches)
}

pub(crate) fn apply_ppf_bytes_stack(
    builder: &mut PatchOverlayBuilder,
    patches: &[(String, Vec<u8>)],
) -> PatchOverlayStack {
    let patches = patches
        .iter()
        .map(|(filename, bytes)| (filename.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    apply_ppf_byte_slices_stack(builder, &patches)
}

pub(crate) fn apply_ppf_byte_slices_stack(
    builder: &mut PatchOverlayBuilder,
    patches: &[(&str, &[u8])],
) -> PatchOverlayStack {
    let mut applied = Vec::with_capacity(patches.len());
    for (filename, patch) in patches {
        let Ok(PatchOverlayApply::Applied) = builder.apply_ppf(patch) else {
            return PatchOverlayStack::Fallback;
        };
        applied.push((
            (*filename).to_owned(),
            !crate::patching::ppf_has_source_validation(patch),
        ));
    }
    PatchOverlayStack::Applied(applied)
}
