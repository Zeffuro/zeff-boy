use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

const RAW_BASE_URL: &str =
    "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/";
const GB_DAT: &str = "Nintendo%20-%20Game%20Boy.dat";
const GBC_DAT: &str = "Nintendo%20-%20Game%20Boy%20Color.dat";
const CACHE_FILE_NAME: &str = "gb_gbc_metadata_v1.bin";
const MAGIC: &[u8; 8] = b"ZBMDAT01";

#[derive(Clone, Debug)]
pub(crate) struct RomMetadata {
    pub(crate) crc32: u32,
    pub(crate) title: String,
    pub(crate) rom_name: String,
    pub(crate) is_gbc: bool,
}

#[derive(Default)]
struct MetadataIndex {
    by_crc: HashMap<u32, RomMetadata>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MetadataRefreshStats {
    pub(crate) total_entries: usize,
    pub(crate) gb_entries: usize,
    pub(crate) gbc_entries: usize,
}

fn metadata_lock() -> &'static RwLock<MetadataIndex> {
    static INDEX: OnceLock<RwLock<MetadataIndex>> = OnceLock::new();
    INDEX.get_or_init(|| RwLock::new(load_cached_index().unwrap_or_default()))
}

fn cache_file_path() -> PathBuf {
    crate::settings::Settings::settings_dir()
        .join("libretro-cache")
        .join(CACHE_FILE_NAME)
}

fn download_dat(url_suffix: &str) -> Result<String, String> {
    let url = format!("{RAW_BASE_URL}{url_suffix}");
    let response = ureq::get(&url)
        .header("User-Agent", "zeff-boy-emulator")
        .call()
        .map_err(|e| format!("Download failed ({url}): {e}"))?;

    response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read metadata response: {e}"))
}

fn parse_quoted(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn parse_crc_hex(line: &str) -> Option<u32> {
    let idx = line.find("crc ")? + 4;
    let rest = &line[idx..];
    let hex: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 8 {
        return None;
    }
    u32::from_str_radix(&hex, 16).ok()
}

fn parse_dat_entries(dat: &str, is_gbc: bool) -> Vec<RomMetadata> {
    let mut entries = Vec::new();
    let mut in_game = false;
    let mut current_title: Option<String> = None;

    for raw_line in dat.lines() {
        let line = raw_line.trim();

        if line.starts_with("game (") {
            in_game = true;
            current_title = None;
            continue;
        }

        if !in_game {
            continue;
        }

        if line == ")" {
            in_game = false;
            current_title = None;
            continue;
        }

        if current_title.is_none() && line.starts_with("name \"") {
            current_title = parse_quoted(line, "name \"");
            continue;
        }

        if !line.contains("rom (") {
            continue;
        }

        let rom_name = parse_quoted(line, "name \"");
        let crc = parse_crc_hex(line);

        if let (Some(rom_name), Some(crc32)) = (rom_name, crc) {
            let fallback_title = rom_name
                .trim_end_matches(".gb")
                .trim_end_matches(".gbc")
                .to_string();
            let title = current_title.clone().unwrap_or(fallback_title);
            entries.push(RomMetadata {
                crc32,
                title,
                rom_name,
                is_gbc,
            });
        }
    }

    entries
}

fn write_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}

fn write_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn read_u8(cur: &mut Cursor<&[u8]>) -> Result<u8, String> {
    let mut b = [0u8; 1];
    cur.read_exact(&mut b)
        .map_err(|e| format!("Metadata decode error (u8): {e}"))?;
    Ok(b[0])
}

fn read_u16(cur: &mut Cursor<&[u8]>) -> Result<u16, String> {
    let mut b = [0u8; 2];
    cur.read_exact(&mut b)
        .map_err(|e| format!("Metadata decode error (u16): {e}"))?;
    Ok(u16::from_le_bytes(b))
}

fn read_u32(cur: &mut Cursor<&[u8]>) -> Result<u32, String> {
    let mut b = [0u8; 4];
    cur.read_exact(&mut b)
        .map_err(|e| format!("Metadata decode error (u32): {e}"))?;
    Ok(u32::from_le_bytes(b))
}

fn serialize_entries(entries: &[RomMetadata]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(entries.len() * 48);
    out.extend_from_slice(MAGIC);
    write_u32(
        &mut out,
        u32::try_from(entries.len()).map_err(|_| "Too many metadata entries".to_string())?,
    );

    for entry in entries {
        let title_bytes = entry.title.as_bytes();
        let rom_name_bytes = entry.rom_name.as_bytes();
        if title_bytes.len() > u16::MAX as usize || rom_name_bytes.len() > u16::MAX as usize {
            return Err("Metadata string too long".to_string());
        }

        write_u32(&mut out, entry.crc32);
        write_u8(&mut out, if entry.is_gbc { 1 } else { 0 });
        write_u16(&mut out, title_bytes.len() as u16);
        out.extend_from_slice(title_bytes);
        write_u16(&mut out, rom_name_bytes.len() as u16);
        out.extend_from_slice(rom_name_bytes);
    }

    Ok(out)
}

fn deserialize_entries(bytes: &[u8]) -> Result<Vec<RomMetadata>, String> {
    if bytes.len() < MAGIC.len() + 4 || &bytes[..MAGIC.len()] != MAGIC {
        return Err("Metadata cache format mismatch".to_string());
    }

    let mut cur = Cursor::new(bytes);
    let mut magic = [0u8; 8];
    cur.read_exact(&mut magic)
        .map_err(|e| format!("Metadata decode error (magic): {e}"))?;

    let count = read_u32(&mut cur)? as usize;
    let mut entries = Vec::with_capacity(count);

    for _ in 0..count {
        let crc32 = read_u32(&mut cur)?;
        let is_gbc = read_u8(&mut cur)? != 0;

        let title_len = read_u16(&mut cur)? as usize;
        let mut title = vec![0u8; title_len];
        cur.read_exact(&mut title)
            .map_err(|e| format!("Metadata decode error (title): {e}"))?;

        let rom_name_len = read_u16(&mut cur)? as usize;
        let mut rom_name = vec![0u8; rom_name_len];
        cur.read_exact(&mut rom_name)
            .map_err(|e| format!("Metadata decode error (rom_name): {e}"))?;

        entries.push(RomMetadata {
            crc32,
            title: String::from_utf8(title)
                .map_err(|e| format!("Metadata decode error (title utf8): {e}"))?,
            rom_name: String::from_utf8(rom_name)
                .map_err(|e| format!("Metadata decode error (rom_name utf8): {e}"))?,
            is_gbc,
        });
    }

    Ok(entries)
}

fn build_index(entries: Vec<RomMetadata>) -> MetadataIndex {
    let mut by_crc = HashMap::with_capacity(entries.len());
    for entry in entries {
        by_crc.entry(entry.crc32).or_insert(entry);
    }
    MetadataIndex { by_crc }
}

fn load_cached_index() -> Result<MetadataIndex, String> {
    let path = cache_file_path();
    let bytes = std::fs::read(path).map_err(|e| format!("Failed to read metadata cache: {e}"))?;
    let entries = deserialize_entries(&bytes)?;
    Ok(build_index(entries))
}

fn write_cache_file(path: &Path, entries: &[RomMetadata]) -> Result<(), String> {
    let data = serialize_entries(entries)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create metadata cache dir: {e}"))?;
    }
    let mut file = std::fs::File::create(path)
        .map_err(|e| format!("Failed to create metadata cache file: {e}"))?;
    file.write_all(&data)
        .map_err(|e| format!("Failed to write metadata cache: {e}"))
}

pub(crate) fn refresh_cache_from_libretro() -> Result<MetadataRefreshStats, String> {
    let gb_dat = download_dat(GB_DAT)?;
    let gbc_dat = download_dat(GBC_DAT)?;

    let mut gb_entries = parse_dat_entries(&gb_dat, false);
    let gbc_entries = parse_dat_entries(&gbc_dat, true);

    let gb_count = gb_entries.len();
    let gbc_count = gbc_entries.len();

    gb_entries.extend(gbc_entries);
    let merged_entries = gb_entries;

    write_cache_file(&cache_file_path(), &merged_entries)?;

    let index = build_index(merged_entries.clone());
    if let Ok(mut guard) = metadata_lock().write() {
        *guard = index;
    }

    Ok(MetadataRefreshStats {
        total_entries: merged_entries.len(),
        gb_entries: gb_count,
        gbc_entries: gbc_count,
    })
}

pub(crate) fn lookup_cached(crc32: u32, is_gbc: bool) -> Option<RomMetadata> {
    let guard = metadata_lock().read().ok()?;
    let exact = guard.by_crc.get(&crc32)?;

    if exact.is_gbc == is_gbc {
        return Some(exact.clone());
    }

    Some(exact.clone())
}

fn normalized_words(input: &str) -> Vec<String> {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(|v| v.to_string())
        .collect()
}

fn strip_suffix_groups(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut depth_round = 0usize;
    let mut depth_square = 0usize;

    for c in input.chars() {
        match c {
            '(' => depth_round += 1,
            ')' => depth_round = depth_round.saturating_sub(1),
            '[' => depth_square += 1,
            ']' => depth_square = depth_square.saturating_sub(1),
            _ if depth_round == 0 && depth_square == 0 => out.push(c),
            _ => {}
        }
    }

    out.trim().to_string()
}

fn dedupe_keep_order(items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let key = item.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(item);
        }
    }
    out
}

pub(crate) fn build_cheat_search_hints(
    header_title: &str,
    metadata: Option<&RomMetadata>,
) -> Vec<String> {
    let mut hints = Vec::new();

    if let Some(meta) = metadata {
        hints.push(strip_suffix_groups(&meta.title));
        hints.push(meta.title.clone());

        let rom_stem = meta
            .rom_name
            .trim_end_matches(".gb")
            .trim_end_matches(".gbc")
            .to_string();
        hints.push(strip_suffix_groups(&rom_stem));
        hints.push(rom_stem.clone());
    }

    hints.push(strip_suffix_groups(header_title));
    hints.push(header_title.trim().to_string());

    let mut compact = Vec::new();
    for hint in hints {
        if hint.trim().is_empty() {
            continue;
        }

        compact.push(hint.clone());
        let words = normalized_words(&hint);
        if !words.is_empty() {
            compact.push(words.join(" "));
        }
    }

    dedupe_keep_order(compact)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dat_entries_extracts_crc_title_and_rom_name() {
        let dat = r#"
            game (
                name "Pokemon Red Version (USA, Europe)"
                rom ( name "Pokemon Red Version (USA, Europe) (SGB Enhanced).gb" size 524288 crc D7037C83 md5 0 sha1 0 )
            )
        "#;

        let entries = parse_dat_entries(dat, false);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].crc32, 0xD7037C83);
        assert_eq!(entries[0].title, "Pokemon Red Version (USA, Europe)");
        assert_eq!(
            entries[0].rom_name,
            "Pokemon Red Version (USA, Europe) (SGB Enhanced).gb"
        );
    }

    #[test]
    fn build_cheat_search_hints_prefers_metadata_aliases() {
        let meta = RomMetadata {
            crc32: 0,
            title: "Pokemon Red Version (USA, Europe)".to_string(),
            rom_name: "Pokemon Red Version (USA, Europe) (SGB Enhanced).gb".to_string(),
            is_gbc: false,
        };

        let hints = build_cheat_search_hints("POKEMON RED", Some(&meta));
        assert!(hints.iter().any(|h| h.contains("Pokemon Red Version")));
        assert!(hints.iter().any(|h| h == "pokemon red version usa europe"));
    }

    #[test]
    fn serialize_roundtrip_preserves_entries() {
        let entries = vec![RomMetadata {
            crc32: 0x1234ABCD,
            title: "Test Game".to_string(),
            rom_name: "Test Game.gb".to_string(),
            is_gbc: false,
        }];

        let bytes = serialize_entries(&entries).unwrap();
        let parsed = deserialize_entries(&bytes).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].crc32, 0x1234ABCD);
        assert_eq!(parsed[0].title, "Test Game");
        assert_eq!(parsed[0].rom_name, "Test Game.gb");
        assert!(!parsed[0].is_gbc);
    }
}
