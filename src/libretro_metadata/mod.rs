use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use anyhow::Context;

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

fn download_dat(url_suffix: &str) -> anyhow::Result<String> {
    let url = format!("{RAW_BASE_URL}{url_suffix}");
    crate::libretro_common::ureq_get(&url)?
        .read_to_string()
        .context("Failed to read metadata response")
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

fn read_u8(cur: &mut Cursor<&[u8]>) -> anyhow::Result<u8> {
    let mut b = [0u8; 1];
    cur.read_exact(&mut b)
        .context("metadata decode error (u8)")?;
    Ok(b[0])
}

fn read_u16(cur: &mut Cursor<&[u8]>) -> anyhow::Result<u16> {
    let mut b = [0u8; 2];
    cur.read_exact(&mut b)
        .context("metadata decode error (u16)")?;
    Ok(u16::from_le_bytes(b))
}

fn read_u32(cur: &mut Cursor<&[u8]>) -> anyhow::Result<u32> {
    let mut b = [0u8; 4];
    cur.read_exact(&mut b)
        .context("metadata decode error (u32)")?;
    Ok(u32::from_le_bytes(b))
}

fn serialize_entries(entries: &[RomMetadata]) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(entries.len() * 48);
    out.extend_from_slice(MAGIC);
    write_u32(
        &mut out,
        u32::try_from(entries.len()).context("too many metadata entries")?,
    );

    for entry in entries {
        let title_bytes = entry.title.as_bytes();
        let rom_name_bytes = entry.rom_name.as_bytes();
        if title_bytes.len() > u16::MAX as usize || rom_name_bytes.len() > u16::MAX as usize {
            anyhow::bail!("metadata string too long");
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

fn deserialize_entries(bytes: &[u8]) -> anyhow::Result<Vec<RomMetadata>> {
    if bytes.len() < MAGIC.len() + 4 || &bytes[..MAGIC.len()] != MAGIC {
        anyhow::bail!("metadata cache format mismatch");
    }

    let mut cur = Cursor::new(bytes);
    let mut magic = [0u8; 8];
    cur.read_exact(&mut magic)
        .context("metadata decode error (magic)")?;

    let count = read_u32(&mut cur)? as usize;
    let mut entries = Vec::with_capacity(count);

    for _ in 0..count {
        let crc32 = read_u32(&mut cur)?;
        let is_gbc = read_u8(&mut cur)? != 0;

        let title_len = read_u16(&mut cur)? as usize;
        let mut title = vec![0u8; title_len];
        cur.read_exact(&mut title)
            .context("metadata decode error (title)")?;

        let rom_name_len = read_u16(&mut cur)? as usize;
        let mut rom_name = vec![0u8; rom_name_len];
        cur.read_exact(&mut rom_name)
            .context("metadata decode error (rom_name)")?;

        entries.push(RomMetadata {
            crc32,
            title: String::from_utf8(title).context("metadata decode error (title utf8)")?,
            rom_name: String::from_utf8(rom_name)
                .context("metadata decode error (rom_name utf8)")?,
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

fn load_cached_index() -> anyhow::Result<MetadataIndex> {
    let path = cache_file_path();
    let bytes = std::fs::read(&path)
        .with_context(|| format!("failed to read metadata cache: {}", path.display()))?;
    let entries = deserialize_entries(&bytes)?;
    Ok(build_index(entries))
}

fn write_cache_file(path: &Path, entries: &[RomMetadata]) -> anyhow::Result<()> {
    let data = serialize_entries(entries)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("failed to create metadata cache directory")?;
    }
    let mut file = std::fs::File::create(path)
        .with_context(|| format!("failed to create metadata cache file: {}", path.display()))?;
    file.write_all(&data)
        .context("failed to write metadata cache")
}

pub(crate) fn refresh_cache_from_libretro() -> anyhow::Result<MetadataRefreshStats> {
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
    crate::libretro_common::normalized_words(input)
}

fn strip_suffix_groups(input: &str) -> String {
    crate::libretro_common::strip_suffix_groups(input)
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
mod tests;
