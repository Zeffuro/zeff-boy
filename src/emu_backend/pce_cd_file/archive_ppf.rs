use super::*;

pub(crate) fn try_load_cached_cue_ppf_overlay_byte_slices(
    sheet: &CueSheet,
    sources: Vec<CueFileSource>,
    patches: &[(&str, &[u8])],
) -> Result<Option<CdDisc>, PceCdLoadError> {
    let files = cached_files(sources)?;
    try_build_ppf_overlay_disc(sheet, &files, |builder| {
        apply_ppf_byte_slices_stack(builder, patches)
    })
}
