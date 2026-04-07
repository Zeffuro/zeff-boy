#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum LibretroPlatform {
    Gb = 0,
    Gbc = 1,
    Nes = 2,
}

impl LibretroPlatform {
    pub(crate) fn platform_dir(self) -> &'static str {
        match self {
            Self::Gb => "Nintendo - Game Boy",
            Self::Gbc => "Nintendo - Game Boy Color",
            Self::Nes => "Nintendo - Nintendo Entertainment System",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Gb => "Game Boy",
            Self::Gbc => "Game Boy Color",
            Self::Nes => "NES",
        }
    }

    pub(crate) fn cache_suffix(self) -> &'static str {
        match self {
            Self::Gb => "gb",
            Self::Gbc => "gbc",
            Self::Nes => "nes",
        }
    }

    pub(crate) fn rom_extensions(self) -> &'static [&'static str] {
        match self {
            Self::Gb => &[".gb"],
            Self::Gbc => &[".gbc", ".gb"],
            Self::Nes => &[".nes"],
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) const USER_AGENT: &str = "zeff-boy-emulator";

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn ureq_get(url: &str) -> anyhow::Result<ureq::Body> {
    let resp = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| anyhow::anyhow!("HTTP request failed ({url}): {e}"))?;
    Ok(resp.into_body())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn ureq_get_github_json(url: &str) -> anyhow::Result<ureq::Body> {
    let resp = ureq::get(url)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| anyhow::anyhow!("GitHub API request failed ({url}): {e}"))?;
    Ok(resp.into_body())
}

pub(crate) fn normalized_words(input: &str) -> Vec<String> {
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

pub(crate) fn strip_suffix_groups(input: &str) -> String {
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
