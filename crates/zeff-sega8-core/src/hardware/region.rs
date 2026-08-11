use std::path::Path;

use super::cartridge::Region;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sega8Region {
    Japanese,
    JapanesePowerBaseConverter,
    #[default]
    Export,
}

impl Sega8Region {
    pub fn label(self) -> &'static str {
        match self {
            Self::Japanese => "japanese",
            Self::JapanesePowerBaseConverter => "japanese_pbc",
            Self::Export => "export",
        }
    }

    pub fn display_label(self) -> &'static str {
        match self {
            Self::Japanese => "Japanese",
            Self::JapanesePowerBaseConverter => "Japanese PBC",
            Self::Export => "Export",
        }
    }

    pub fn is_export(self) -> bool {
        self == Self::Export
    }

    pub fn parse(value: &str) -> Option<Self> {
        let normalized = normalize_region_tag(value);
        match normalized.as_str() {
            "japanese" | "japan" | "jp" | "jpn" | "korea" | "kr" => Some(Self::Japanese),
            "pbc" | "powerbase" | "powerbaseconverter" | "japanesepbc" | "jpnpbc" | "jppbc"
            | "mdjapanese" | "japanesemd" => Some(Self::JapanesePowerBaseConverter),
            "export" | "international" | "world" | "usa" | "us" | "europe" | "eu" | "eur"
            | "australia" | "aus" | "brazil" | "br" => Some(Self::Export),
            _ => None,
        }
    }

    pub fn from_header_region(region: Region) -> Option<Self> {
        match region {
            Region::SmsJapan | Region::GameGearJapan => Some(Self::Japanese),
            Region::SmsExport | Region::GameGearExport | Region::GameGearInternational => {
                Some(Self::Export)
            }
            Region::Unknown(_) => None,
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let text = path.to_string_lossy().to_ascii_lowercase();
        Self::from_region_tag_text(&text)
    }

    fn from_region_tag_text(text: &str) -> Option<Self> {
        let pbc_tag = text.contains("[pbc]")
            || text.contains("(pbc)")
            || text.contains("powerbase")
            || text.contains("power base")
            || text.contains("power-base");
        if pbc_tag {
            return Some(Self::JapanesePowerBaseConverter);
        }

        if ["[j]", "(j)"].iter().any(|tag| text.contains(tag)) {
            return Some(Self::Japanese);
        }
        if ["[u]", "(u)", "[e]", "(e)", "[w]", "(w)"]
            .iter()
            .any(|tag| text.contains(tag))
        {
            return Some(Self::Export);
        }

        let tokens = text
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();

        let japanese_tokens = ["japan", "japanese", "jp", "jpn", "korea", "kr"];
        let has_pbc_token = tokens
            .iter()
            .any(|token| matches!(*token, "pbc" | "powerbase" | "powerbaseconverter"));
        if has_pbc_token {
            return Some(Self::JapanesePowerBaseConverter);
        }
        if tokens.iter().any(|token| japanese_tokens.contains(token)) {
            return Some(Self::Japanese);
        }

        let export_tokens = [
            "export",
            "international",
            "world",
            "usa",
            "us",
            "europe",
            "eu",
            "eur",
            "australia",
            "aus",
            "brazil",
            "br",
        ];
        if tokens.iter().any(|token| export_tokens.contains(token)) {
            return Some(Self::Export);
        }

        None
    }
}

fn normalize_region_tag(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_console_regions() {
        assert_eq!(Sega8Region::parse("japan"), Some(Sega8Region::Japanese));
        assert_eq!(
            Sega8Region::parse("japanese-pbc"),
            Some(Sega8Region::JapanesePowerBaseConverter)
        );
        assert_eq!(Sega8Region::parse("export"), Some(Sega8Region::Export));
        assert_eq!(Sega8Region::parse("bad"), None);
    }

    #[test]
    fn detects_common_region_tags_from_paths() {
        assert_eq!(
            Sega8Region::from_path(Path::new("Game (Japan).gg")),
            Some(Sega8Region::Japanese)
        );
        assert_eq!(
            Sega8Region::from_path(Path::new("Game [E].sms")),
            Some(Sega8Region::Export)
        );
        assert_eq!(
            Sega8Region::from_path(Path::new("Game (Japan) (PBC).sms")),
            Some(Sega8Region::JapanesePowerBaseConverter)
        );
        assert_eq!(
            Sega8Region::from_path(Path::new("Game [J] [PBC].sms")),
            Some(Sega8Region::JapanesePowerBaseConverter)
        );
        assert_eq!(
            Sega8Region::from_path(Path::new("Game (PowerBase).sms")),
            Some(Sega8Region::JapanesePowerBaseConverter)
        );
        assert_eq!(Sega8Region::from_path(Path::new("Game.sms")), None);
    }
}
