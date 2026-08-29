use std::collections::HashSet;
use std::io::Read;

use crate::cli::types::{HeadlessInputEvent, HeadlessTasAssertion, HeadlessTasScript};

use super::input::parse_input_event_arg;

const MAX_SCRIPT_BYTES: u64 = 1 << 20;
const MAX_SCRIPT_FRAMES: u64 = 1_000_000_000;
const MAX_SCRIPT_ITEMS: usize = 10_000;
const MAX_LINE_BYTES: usize = 4096;

pub(super) struct ParsedTasScript {
    pub(super) frames: u64,
    pub(super) inputs: [Vec<HeadlessInputEvent>; 5],
    pub(super) script: HeadlessTasScript,
}

pub(super) fn parse_tas_script(path: &str) -> anyhow::Result<ParsedTasScript> {
    anyhow::ensure!(
        !std::path::Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ztas")),
        "--tas-script cannot read .ztas project packages; use .ztascript or .zts for text scripts"
    );
    let metadata = std::fs::metadata(path)
        .map_err(|err| anyhow::anyhow!("failed to read --tas-script '{}': {err}", path))?;
    anyhow::ensure!(
        metadata.len() <= MAX_SCRIPT_BYTES,
        "--tas-script '{}' exceeds the 1 MiB limit",
        path
    );
    let file = std::fs::File::open(path)
        .map_err(|err| anyhow::anyhow!("failed to read --tas-script '{}': {err}", path))?;
    let mut contents = String::new();
    file.take(MAX_SCRIPT_BYTES + 1)
        .read_to_string(&mut contents)
        .map_err(|err| anyhow::anyhow!("failed to read --tas-script '{}': {err}", path))?;
    anyhow::ensure!(
        contents.len() as u64 <= MAX_SCRIPT_BYTES,
        "--tas-script '{}' exceeds the 1 MiB limit",
        path
    );
    parse_tas_script_text(&contents).map_err(|err| anyhow::anyhow!("{err} in {path}"))
}

fn parse_tas_script_text(contents: &str) -> anyhow::Result<ParsedTasScript> {
    let mut parser = TasParser::new();

    for (index, raw_line) in contents.lines().enumerate() {
        let line_number = index + 1;
        anyhow::ensure!(
            raw_line.len() <= MAX_LINE_BYTES,
            "line {line_number} exceeds the 4096-byte limit"
        );
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        parser
            .parse_line(line)
            .map_err(|err| anyhow::anyhow!("{err} at line {line_number}"))?;
    }

    parser.finish()
}

struct TasParser {
    header_seen: bool,
    system: Option<String>,
    frames: Option<u64>,
    inputs: [Vec<HeadlessInputEvent>; 5],
    assertions: Vec<HeadlessTasAssertion>,
    assertion_names: HashSet<String>,
}

impl TasParser {
    fn new() -> Self {
        Self {
            header_seen: false,
            system: None,
            frames: None,
            inputs: std::array::from_fn(|_| Vec::new()),
            assertions: Vec::new(),
            assertion_names: HashSet::new(),
        }
    }

    fn finish(mut self) -> anyhow::Result<ParsedTasScript> {
        anyhow::ensure!(self.header_seen, "missing 'zeff-tas-script 1' header");
        let system = self
            .system
            .ok_or_else(|| anyhow::anyhow!("missing system directive"))?;
        let frames = self
            .frames
            .ok_or_else(|| anyhow::anyhow!("missing frames directive"))?;
        if !self.inputs[1].is_empty() {
            anyhow::ensure!(
                matches!(
                    system.as_str(),
                    "nes" | "coleco" | "pce" | "sms" | "gg" | "sg"
                ),
                "TAS p2 input is not supported for system {system:?}"
            );
        }
        if self.inputs[2..].iter().any(|events| !events.is_empty()) {
            anyhow::ensure!(
                system == "pce",
                "TAS p3-p5 input is only supported for system \"pce\""
            );
        }
        for input in self.inputs.iter().flatten() {
            anyhow::ensure!(
                input.end_frame <= frames,
                "input ending at frame {} exceeds script length {frames}",
                input.end_frame
            );
        }
        for assertion in &self.assertions {
            anyhow::ensure!(
                assertion.frame <= frames,
                "assertion {:?} at frame {} exceeds script length {frames}",
                assertion.name,
                assertion.frame
            );
        }
        self.assertions.sort_by_key(|assertion| assertion.frame);

        Ok(ParsedTasScript {
            frames,
            inputs: self.inputs,
            script: HeadlessTasScript {
                system,
                assertions: self.assertions,
            },
        })
    }

    fn parse_line(&mut self, line: &str) -> anyhow::Result<()> {
        if line == "zeff-tas-script 1" {
            anyhow::ensure!(!self.header_seen, "duplicate script header");
            self.header_seen = true;
            return Ok(());
        }
        if line.starts_with("zeff-tas-script ") {
            anyhow::bail!("unsupported TAS script version");
        }
        anyhow::ensure!(
            self.header_seen,
            "script header must be the first directive"
        );

        let (directive, rest) = line
            .split_once(char::is_whitespace)
            .map(|(directive, rest)| (directive, rest.trim()))
            .unwrap_or((line, ""));
        match directive {
            "system" => {
                anyhow::ensure!(self.system.is_none(), "duplicate system directive");
                anyhow::ensure!(
                    matches!(
                        rest,
                        "gb" | "gba" | "nes" | "coleco" | "pce" | "ws" | "sms" | "gg" | "sg"
                    ),
                    "invalid system {rest:?}; expected gb, gba, nes, coleco, pce, ws, sms, gg, or sg"
                );
                self.system = Some(rest.to_owned());
            }
            "frames" => {
                anyhow::ensure!(self.frames.is_none(), "duplicate frames directive");
                let value = rest
                    .parse::<u64>()
                    .map_err(|_| anyhow::anyhow!("frames must be a decimal integer"))?;
                anyhow::ensure!(
                    (1..=MAX_SCRIPT_FRAMES).contains(&value),
                    "frames must be in 1..={MAX_SCRIPT_FRAMES}"
                );
                self.frames = Some(value);
            }
            "input" => self.parse_input(rest)?,
            "assert" => self.parse_assertion(rest)?,
            _ => anyhow::bail!("unknown TAS directive {directive:?}"),
        }
        Ok(())
    }

    fn parse_input(&mut self, rest: &str) -> anyhow::Result<()> {
        let (player, spec) = rest
            .split_once(char::is_whitespace)
            .map(|(player, spec)| (player, spec.trim()))
            .ok_or_else(|| anyhow::anyhow!("input requires a player and input spec"))?;
        let player_index = match player {
            "p1" => 0,
            "p2" => 1,
            "p3" => 2,
            "p4" => 3,
            "p5" => 4,
            _ => anyhow::bail!("input player must be p1, p2, p3, p4, or p5"),
        };
        let events = parse_input_event_arg(spec, "TAS input")?;
        let count = self.inputs.iter().map(Vec::len).sum::<usize>();
        anyhow::ensure!(
            count.saturating_add(events.len()) <= MAX_SCRIPT_ITEMS,
            "TAS script exceeds {MAX_SCRIPT_ITEMS} input events"
        );
        self.inputs[player_index].extend(events);
        Ok(())
    }

    fn parse_assertion(&mut self, rest: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.assertions.len() < MAX_SCRIPT_ITEMS,
            "TAS script exceeds {MAX_SCRIPT_ITEMS} assertions"
        );
        let mut tokens = rest.split_whitespace();
        let name = tokens
            .next()
            .ok_or_else(|| anyhow::anyhow!("assert requires a name"))?;
        anyhow::ensure!(valid_name(name), "invalid assertion name {name:?}");
        anyhow::ensure!(
            self.assertion_names.insert(name.to_owned()),
            "duplicate assertion name {name:?}"
        );

        let mut frame = None;
        let mut pc = None;
        let mut state_sha256 = None;
        let mut framebuffer_sha256 = None;
        for token in tokens {
            let (key, value) = token
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("assertion field must be key=value"))?;
            match key {
                "frame" => set_once(&mut frame, parse_frame(value)?, key)?,
                "pc" => set_once(&mut pc, parse_pc(value)?, key)?,
                "state-sha256" => set_once(&mut state_sha256, parse_sha256(value)?, key)?,
                "framebuffer-sha256" => {
                    set_once(&mut framebuffer_sha256, parse_sha256(value)?, key)?
                }
                _ => anyhow::bail!("unknown assertion field {key:?}"),
            }
        }
        let frame = frame.ok_or_else(|| anyhow::anyhow!("assertion requires frame=<number>"))?;
        anyhow::ensure!(
            pc.is_some() || state_sha256.is_some() || framebuffer_sha256.is_some(),
            "assertion requires pc, state-sha256, or framebuffer-sha256"
        );
        self.assertions.push(HeadlessTasAssertion {
            name: name.to_owned(),
            frame,
            pc,
            state_sha256,
            framebuffer_sha256,
        });
        Ok(())
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn parse_frame(value: &str) -> anyhow::Result<u64> {
    let frame = value
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("assertion frame must be a decimal integer"))?;
    anyhow::ensure!(frame != 0, "assertion frame must be at least 1");
    Ok(frame)
}

fn parse_pc(value: &str) -> anyhow::Result<u32> {
    let (digits, radix) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or((value, 10), |digits| (digits, 16));
    u32::from_str_radix(digits, radix).map_err(|_| anyhow::anyhow!("invalid PC value {value:?}"))
}

fn parse_sha256(value: &str) -> anyhow::Result<[u8; 32]> {
    anyhow::ensure!(
        value.len() == 64,
        "SHA-256 values must contain 64 hex digits"
    );
    let bytes = const_hex::decode(value).map_err(|_| anyhow::anyhow!("invalid SHA-256 hex"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("SHA-256 values must contain 32 bytes"))
}

fn set_once<T>(slot: &mut Option<T>, value: T, key: &str) -> anyhow::Result<()> {
    anyhow::ensure!(slot.is_none(), "duplicate assertion field {key:?}");
    *slot = Some(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    #[test]
    fn parses_versioned_multiplayer_script_and_assertions() {
        let script = parse_tas_script_text(&format!(
            "zeff-tas-script 1\nsystem pce\nframes 120\ninput p1 i+right@1-4\ninput p5 run@100\nassert boot frame=120 pc=0xE123 framebuffer-sha256={ZERO_HASH}\n"
        ))
        .unwrap();

        assert_eq!(script.frames, 120);
        assert_eq!(script.script.system, "pce");
        assert_eq!(script.inputs[0].len(), 1);
        assert_eq!(script.inputs[4].len(), 1);
        assert_eq!(script.script.assertions[0].pc, Some(0xE123));
    }

    #[test]
    fn rejects_duplicates_overflow_and_system_mismatch_inputs() {
        assert!(
            parse_tas_script_text("zeff-tas-script 1\nsystem gb\nsystem gb\nframes 1\n").is_err()
        );
        assert!(
            parse_tas_script_text("zeff-tas-script 1\nsystem gb\nframes 18446744073709551616\n")
                .is_err()
        );
        assert!(
            parse_tas_script_text("zeff-tas-script 1\nsystem gb\nframes 2\ninput p1 a@3\n")
                .is_err()
        );
    }

    #[test]
    fn rejects_unknown_fields_and_duplicate_assertion_names() {
        let duplicate = format!(
            "zeff-tas-script 1\nsystem gb\nframes 2\nassert same frame=1 state-sha256={ZERO_HASH}\nassert same frame=2 pc=1\n"
        );
        assert!(parse_tas_script_text(&duplicate).is_err());
        assert!(
            parse_tas_script_text(
                "zeff-tas-script 1\nsystem gb\nframes 1\nassert a frame=1 memory=00\n"
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_players_not_supported_by_the_script_system() {
        assert!(
            parse_tas_script_text("zeff-tas-script 1\nsystem gb\nframes 1\ninput p2 a@1\n")
                .is_err()
        );
        assert!(
            parse_tas_script_text("zeff-tas-script 1\nsystem nes\nframes 1\ninput p3 a@1\n")
                .is_err()
        );
        parse_tas_script_text("zeff-tas-script 1\nsystem pce\nframes 1\ninput p5 i@1\n").unwrap();
    }

    #[test]
    fn reserves_ztas_for_project_packages() {
        let error = match parse_tas_script("movie.ztas") {
            Err(error) => error.to_string(),
            Ok(_) => panic!(".ztas must be reserved for project packages"),
        };
        assert!(error.contains("project packages"), "error was: {error}");
    }
}
