use std::path::Path;

pub(super) fn readiness_summary(
    report: &crate::app::tas_control::readiness::TasReadinessReport,
    reload: bool,
) -> String {
    use crate::app::tas_control::readiness::{
        TasReadinessCode, TasReadinessStatus, TasReadinessValue,
    };

    let target = if reload {
        TasReadinessStatus::ReloadRequired
    } else {
        TasReadinessStatus::Incompatible
    };
    let Some(check) = report.checks.iter().find(|check| check.status == target) else {
        return if reload {
            "Reload the loaded game before connecting".to_owned()
        } else {
            "The loaded game does not match this TAS project".to_owned()
        };
    };
    match check.id.code {
        TasReadinessCode::SampleRate => match (&check.loaded, &check.configured) {
            (
                Some(TasReadinessValue::SampleRateProfile { initial, current }),
                Some(TasReadinessValue::SampleRate(configured)),
            ) if reload => format!(
                "Reload required: this game started at {initial} Hz (currently {current} Hz); settings for the next load are {configured} Hz"
            ),
            (_, Some(TasReadinessValue::SampleRate(configured))) => format!(
                "This TAS requires 48000 Hz; settings for the next load are {configured} Hz"
            ),
            _ => "The loaded game's sample-rate profile is incompatible".to_owned(),
        },
        TasReadinessCode::LoadProvenance | TasReadinessCode::InitialInput if reload => {
            "Reload the game directly before connecting this TAS".to_owned()
        }
        TasReadinessCode::System
        | TasReadinessCode::CoreIdentity
        | TasReadinessCode::SourceMedia
        | TasReadinessCode::EffectiveMedia
        | TasReadinessCode::DirectSource => {
            "Load the exact unmodified game identified by this TAS project".to_owned()
        }
        _ => "The loaded game's deterministic profile does not match this TAS project".to_owned(),
    }
}

pub(super) fn has_nes_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("nes"))
}

pub(super) fn has_fds_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("fds"))
}

pub(super) fn has_zip_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

pub(super) fn tas_source_matches(
    source_path: Option<&Path>,
    rom_path: Option<&Path>,
    member_matches: fn(&Path) -> bool,
) -> bool {
    source_path.is_some_and(member_matches)
        || (source_path.is_some_and(has_zip_extension) && rom_path.is_some_and(member_matches))
}

pub(super) fn has_gb_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gb"))
}

pub(super) fn has_gbc_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gbc"))
}

pub(super) fn has_coleco_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("col"))
}

pub(super) fn has_sms_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("sms"))
}

pub(super) fn has_game_gear_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gg"))
}

pub(super) fn has_gba_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gba"))
}

pub(super) fn has_sg1000_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("sg") || extension.eq_ignore_ascii_case("sc")
        })
}

pub(super) fn has_ws_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("ws") || extension.eq_ignore_ascii_case("wsc")
        })
}

pub(super) fn has_pce_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pce"))
}

pub(super) fn has_pce_cd_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cue")
                || extension.eq_ignore_ascii_case("chd")
                || extension.eq_ignore_ascii_case("iso")
        })
}
