use std::io::ErrorKind;
use std::path::Path;

use super::super::audio_discovery_input::{
    MAX_ARCHIVE_BYTES, MAX_ROM_BYTES_U64, audio_format, cartridge_system, has_extension,
    minimum_bytes,
};
use super::CorpusInput;

pub(super) fn failure_reason(input: &CorpusInput, error: &anyhow::Error) -> &'static str {
    let path = &input.path;
    if let Some(error) = error.downcast_ref::<std::io::Error>() {
        match error.kind() {
            ErrorKind::NotFound => return "source_not_found",
            ErrorKind::PermissionDenied => return "source_access_denied",
            _ => {}
        }
    }
    let archive = has_extension(path, "zip");
    let bounded_archive = archive
        && input
            .archive_member
            .as_deref()
            .is_some_and(|member| !has_extension(Path::new(member), "cue"));
    if input.archive_member.is_some() && !archive {
        return "invalid_archive_selection";
    }
    if archive && input.archive_member.is_none() {
        return "archive_member_required";
    }
    if !archive
        && !has_extension(path, "cue")
        && cartridge_system(path).is_none()
        && audio_format(path).is_none()
    {
        return "unsupported_input_format";
    }
    if let Ok(metadata) = std::fs::metadata(path) {
        if metadata.is_dir() {
            return "source_is_directory";
        }
        if metadata.len() == 0 {
            return "empty_source";
        }
        if cartridge_system(path).is_some_and(|system| {
            !(minimum_bytes(system)..=MAX_ROM_BYTES_U64).contains(&metadata.len())
        }) || (bounded_archive && metadata.len() > MAX_ARCHIVE_BYTES)
        {
            return "source_size_out_of_range";
        }
    }
    if let Some(member) = &input.archive_member {
        let member = Path::new(member);
        if !has_extension(member, "cue")
            && cartridge_system(member).is_none()
            && audio_format(member).is_none()
        {
            return "unsupported_archive_member_format";
        }
    }
    if archive {
        "archive_load_failed"
    } else if has_extension(path, "cue") {
        "disc_load_failed"
    } else if audio_format(path).is_some() {
        "audio_file_load_failed"
    } else {
        "cartridge_load_failed"
    }
}
