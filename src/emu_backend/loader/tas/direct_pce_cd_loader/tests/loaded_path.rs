use anyhow::Result;

use super::*;

#[test]
fn loaded_archive_path_preserves_selected_cue_without_reading_source() -> Result<()> {
    for extension in ["7z", "rar", "zip"] {
        let source = PathBuf::from(format!("missing.{extension}"));
        let selected = source.join("second").join("disc.cue");
        let loader =
            DirectPceCdTasExecutionLoader::new_for_loaded_rom_path(source, &selected, Vec::new())?;
        assert_eq!(
            loader.archive_cue_member.as_deref(),
            Some("second/disc.cue")
        );
    }
    Ok(())
}

#[test]
fn loaded_archive_path_rejects_member_outside_source() {
    let source = PathBuf::from("missing.7z");
    let selected = PathBuf::from("other.7z").join("disc.cue");
    assert!(
        DirectPceCdTasExecutionLoader::new_for_loaded_rom_path(source, &selected, Vec::new(),)
            .is_err()
    );
}
