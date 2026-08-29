use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use crate::cli::Cli;
use crate::model::Core;

const DEFAULT_COMPAT_OUTPUT: &str = "rom-tests/manifests/compat-games/local-generated.toml";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompatEntry {
    pub(crate) core: Core,
    pub(crate) path: PathBuf,
}

pub(crate) fn generate_compat_manifest(cli: &Cli) -> anyhow::Result<()> {
    let rom_dir = cli
        .compat_rom_dir
        .as_deref()
        .context("generate-compat requires --rom-dir PATH")?;
    let output = cli
        .compat_output
        .as_deref()
        .unwrap_or_else(|| Path::new(DEFAULT_COMPAT_OUTPUT));

    let entries = scan_compat_entries(
        rom_dir,
        cli.filter.core,
        cli.compat_limit,
        &cli.compat_name_matches,
    )
    .with_context(|| format!("failed to scan {}", rom_dir.display()))?;

    if entries.is_empty() {
        let supported_extensions = supported_rom_extension_label();
        bail!(
            "no supported ROM files found under {}; supported extensions: {supported_extensions}; .zip is supported when --core is set",
            rom_dir.display(),
        );
    }

    let manifest_slug = output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(sanitize_id_part)
        .filter(|slug| !slug.is_empty())
        .unwrap_or_else(|| "local-generated".to_string());
    let manifest = render_compat_manifest(&entries, cli.compat_max_frames, &manifest_slug);
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(output, manifest).with_context(|| format!("failed to write {}", output.display()))?;

    println!(
        "wrote {} local compatibility entries to {}",
        entries.len(),
        output.display()
    );
    println!("local*.toml is ignored by git; do not copy this file into committed manifests");
    Ok(())
}

pub(crate) fn scan_compat_entries(
    rom_dir: &Path,
    core_filter: Option<Core>,
    limit: Option<usize>,
    name_matches: &[String],
) -> anyhow::Result<Vec<CompatEntry>> {
    if !rom_dir.is_dir() {
        bail!("{} is not a directory", rom_dir.display());
    }

    let mut entries = Vec::new();
    let name_matches = normalized_name_matches(name_matches);
    scan_dir_recursive(rom_dir, core_filter, limit, &name_matches, &mut entries)?;
    entries.sort_by(|a, b| {
        a.core
            .cmp(&b.core)
            .then_with(|| a.path.to_string_lossy().cmp(&b.path.to_string_lossy()))
    });
    Ok(entries)
}

fn scan_dir_recursive(
    dir: &Path,
    core_filter: Option<Core>,
    limit: Option<usize>,
    name_matches: &[String],
    entries: &mut Vec<CompatEntry>,
) -> anyhow::Result<()> {
    if limit.is_some_and(|limit| entries.len() >= limit) {
        return Ok(());
    }

    let mut children = fs::read_dir(dir)
        .with_context(|| format!("failed to read {}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("failed to read child entry under {}", dir.display()))?;
    children.sort_by_key(|entry| entry.path());

    for child in children {
        if limit.is_some_and(|limit| entries.len() >= limit) {
            break;
        }
        let path = child.path();
        let file_type = child
            .file_type()
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        if file_type.is_dir() {
            scan_dir_recursive(&path, core_filter, limit, name_matches, entries)?;
        } else if file_type.is_file()
            && let Some(core) = infer_core_from_path(&path, core_filter)
            && core_filter.is_none_or(|filter| filter == core)
            && !is_obvious_firmware_file_name(&path)
            && file_name_matches(&path, name_matches)
        {
            entries.push(CompatEntry { core, path });
        }
    }

    Ok(())
}

fn normalized_name_matches(name_matches: &[String]) -> Vec<String> {
    name_matches
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

fn file_name_matches(path: &Path, name_matches: &[String]) -> bool {
    if name_matches.is_empty() {
        return true;
    }
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let file_name = file_name.to_ascii_lowercase();
    name_matches
        .iter()
        .any(|name_match| file_name.contains(name_match))
}

fn is_obvious_firmware_file_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|file_name| {
            file_name
                .split(|ch: char| !ch.is_ascii_alphanumeric())
                .any(|word| word.eq_ignore_ascii_case("bios"))
        })
}

fn infer_core_from_path(path: &Path, core_filter: Option<Core>) -> Option<Core> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    if ext == "zip" {
        core_filter
    } else {
        Core::from_extension(&ext)
    }
}

fn supported_rom_extension_label() -> String {
    Core::specs()
        .iter()
        .flat_map(|spec| spec.rom_extensions.iter())
        .map(|ext| format!(".{ext}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn render_compat_manifest(
    entries: &[CompatEntry],
    max_frames: u64,
    manifest_slug: &str,
) -> String {
    let mut out = String::new();
    out.push_str("manifest_version = 1\n\n");
    out.push_str("[suite]\n");
    out.push_str(&format!(
        "id = \"compat-games-{}\"\n",
        toml_escape(manifest_slug)
    ));
    out.push_str("name = \"Local generated game compatibility\"\n");
    out.push_str("license = \"copyrighted; user-owned local dumps only\"\n\n");

    for (index, entry) in entries.iter().enumerate() {
        let id = compat_id(manifest_slug, index, entry);
        out.push_str("[[tests]]\n");
        out.push_str(&format!("id = \"{}\"\n", toml_escape(&id)));
        out.push_str(&format!("core = \"{}\"\n", entry.core));
        out.push_str("tier = \"compat\"\n");
        out.push_str(&format!("max_frames = {max_frames}\n"));
        out.push_str(&format!("tags = {}\n", compat_tags_toml(entry.core)));
        out.push_str("notes = \"Generated local-only compatibility entry. Do not commit this manifest or reports derived from commercial ROMs.\"\n\n");
        out.push_str("[tests.artifact]\n");
        out.push_str("kind = \"game_rom\"\n");
        out.push_str("license = \"copyrighted\"\n");
        out.push_str("license_confidence = \"user_owned\"\n");
        out.push_str("redistributable = false\n");
        out.push_str("source_url = \"local user-owned cartridge dump\"\n");
        out.push_str("source_version = \"local\"\n\n");
        out.push_str("[tests.rom]\n");
        out.push_str(&format!(
            "path = \"{}\"\n\n",
            toml_escape(&entry.path.to_string_lossy())
        ));
        out.push_str("[tests.pass]\n");
        out.push_str("kind = \"headless_exit\"\n\n");
        out.push_str("[tests.expectation]\n");
        out.push_str("kind = \"pass\"\n\n");
    }

    out
}

fn compat_tags_toml(core: Core) -> String {
    let mut tags = vec![
        "compat".to_string(),
        core.to_string(),
        "local-only".to_string(),
        "generated".to_string(),
    ];
    if is_sega8_core(core) {
        tags.push("sega8".to_string());
    }
    format!(
        "[{}]",
        tags.iter()
            .map(|tag| format!("\"{}\"", toml_escape(tag)))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn is_sega8_core(core: Core) -> bool {
    matches!(core, Core::Sms | Core::Gg | Core::Sg)
}

fn compat_id(manifest_slug: &str, index: usize, entry: &CompatEntry) -> String {
    let stem = compat_file_stem(&entry.path)
        .map(sanitize_id_part)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "game".to_string());
    format!(
        "compat/local/{}/{}/{:04}-{stem}",
        entry.core,
        manifest_slug,
        index + 1
    )
}

fn compat_file_stem(path: &Path) -> Option<&str> {
    let path = path.to_str()?;
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    (!stem.is_empty()).then_some(stem)
}

fn sanitize_id_part(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn toml_escape(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_compat_manifest_uses_game_rom_and_headless_exit() {
        let manifest = render_compat_manifest(
            &[CompatEntry {
                core: Core::Gba,
                path: PathBuf::from(r"user-owned\gba\Test Game.gba"),
            }],
            123,
            "local-generated",
        );

        assert!(manifest.contains("kind = \"game_rom\""));
        assert!(manifest.contains("kind = \"headless_exit\""));
        assert!(manifest.contains("max_frames = 123"));
        assert!(manifest.contains("compat/local/gba/local-generated/0001-test-game"));
        assert!(manifest.contains(r#"path = "user-owned\\gba\\Test Game.gba""#));
    }

    #[test]
    fn render_compat_manifest_tags_sega8_entries_by_family() {
        let manifest = render_compat_manifest(
            &[CompatEntry {
                core: Core::Sms,
                path: PathBuf::from(r"user-owned\sms\Test Game.sms"),
            }],
            123,
            "local-generated",
        );

        assert!(
            manifest.contains(r#"tags = ["compat", "sms", "local-only", "generated", "sega8"]"#)
        );
        assert!(manifest.contains("compat/local/sms/local-generated/0001-test-game"));
    }

    #[test]
    fn infer_core_from_supported_extensions() {
        assert_eq!(
            infer_core_from_path(Path::new("foo.gb"), None),
            Some(Core::Gb)
        );
        assert_eq!(
            infer_core_from_path(Path::new("foo.gbc"), None),
            Some(Core::Gb)
        );
        assert_eq!(
            infer_core_from_path(Path::new("foo.sgb"), None),
            Some(Core::Gb)
        );
        assert_eq!(
            infer_core_from_path(Path::new("foo.gba"), None),
            Some(Core::Gba)
        );
        assert_eq!(
            infer_core_from_path(Path::new("foo.nes"), None),
            Some(Core::Nes)
        );
        assert_eq!(
            infer_core_from_path(Path::new("foo.ws"), None),
            Some(Core::Ws)
        );
        assert_eq!(
            infer_core_from_path(Path::new("foo.wsc"), None),
            Some(Core::Ws)
        );
        assert_eq!(
            infer_core_from_path(Path::new("foo.sms"), None),
            Some(Core::Sms)
        );
        assert_eq!(
            infer_core_from_path(Path::new("foo.gg"), None),
            Some(Core::Gg)
        );
        assert_eq!(
            infer_core_from_path(Path::new("foo.sg"), None),
            Some(Core::Sg)
        );
        assert_eq!(
            infer_core_from_path(Path::new("foo.sc"), None),
            Some(Core::Sg)
        );
        assert_eq!(infer_core_from_path(Path::new("foo.sfc"), None), None);
        assert_eq!(infer_core_from_path(Path::new("foo.zip"), None), None);
        assert_eq!(
            infer_core_from_path(Path::new("foo.zip"), Some(Core::Gba)),
            Some(Core::Gba)
        );
    }

    #[test]
    fn file_name_match_filters_case_insensitively() {
        assert!(file_name_matches(
            Path::new("Example Gem.gbc"),
            &normalized_name_matches(&["gem".to_string()])
        ));
        assert!(!file_name_matches(
            Path::new("Example Gem.gbc"),
            &normalized_name_matches(&["emerald".to_string()])
        ));
    }

    #[test]
    fn compatibility_scan_excludes_bios_names_without_excluding_game_names() {
        assert!(is_obvious_firmware_file_name(Path::new(
            "[BIOS] ColecoVision (USA, Europe).zip"
        )));
        assert!(is_obvious_firmware_file_name(Path::new(
            "ColecoVision BIOS.zip"
        )));
        assert!(!is_obvious_firmware_file_name(Path::new(
            "Zaxxon (USA, Europe).zip"
        )));
        assert!(!is_obvious_firmware_file_name(Path::new("Bioshock.gba")));
    }
}
