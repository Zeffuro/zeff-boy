use super::HeadlessOptions;
use zeff_md_core::{ControllerKind, Region};

pub(super) fn parse(
    option: &str,
    value: Option<&String>,
    options: &mut HeadlessOptions,
) -> anyhow::Result<()> {
    let value = value
        .ok_or_else(|| anyhow::anyhow!("{option} requires a value"))?
        .to_ascii_lowercase();
    match option {
        "--md-region" => {
            options.md_region = match value.as_str() {
                "auto" => None,
                "ntsc" | "us" => Some(Region::NtscUs),
                "japan" | "jp" | "ntsc-j" => Some(Region::NtscJapan),
                "pal" => Some(Region::Pal),
                _ => anyhow::bail!("--md-region requires auto, ntsc, japan or pal"),
            }
        }
        "--md-controller" => {
            options.md_controller = match value.as_str() {
                "3" | "three" => Some(ControllerKind::ThreeButton),
                "6" | "six" => Some(ControllerKind::SixButton),
                _ => anyhow::bail!("--md-controller requires 3 or 6"),
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::parse_args_from;
    use zeff_md_core::{ControllerKind, Region};
    #[test]
    fn md_region_and_controller_flags() {
        let args = parse_args_from([
            "--headless",
            "--md-region",
            "pal",
            "--md-controller",
            "3",
            "game.md",
        ])
        .unwrap();
        let headless = args.headless.unwrap();
        assert_eq!(headless.md_region, Some(Region::Pal));
        assert_eq!(headless.md_controller, Some(ControllerKind::ThreeButton));
        for args in [
            vec!["--md-region", "invalid"],
            vec!["--md-controller", "9"],
            vec!["--md-region"],
            vec!["--md-region", "pal", "game.md"],
            vec!["--md-controller", "6", "game.md"],
        ] {
            assert!(parse_args_from(args).is_err());
        }
        for value in ["japan", "JP", "ntsc-j"] {
            let args = parse_args_from(["--headless", "--md-region", value, "game.md"]).unwrap();
            assert_eq!(args.headless.unwrap().md_region, Some(Region::NtscJapan));
        }
    }

    #[test]
    fn md_state_output_path_requires_headless_and_preserves_case() {
        let args = parse_args_from([
            "--headless",
            "--md-save-state-out",
            "Saves/Current.mdstate",
            "game.md",
        ])
        .unwrap();
        assert_eq!(
            args.headless.unwrap().md_save_state_path,
            Some("Saves/Current.mdstate".into())
        );
        assert!(parse_args_from(["--headless", "--md-save-state-out"]).is_err());
        assert!(parse_args_from(["--md-save-state-out", "save.mdstate", "game.md"]).is_err());
    }
}
