use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::certificate_fp::REQUIRED_CONTROLS;

const MAX_SIZE: usize = 64 * 1024;
const DOMAIN: &[u8] = b"ZeffNetplay-ArtifactPair/v1\0";
const PUBLIC_KEY: &str = "c0c3e9303eccde66c0e207a014884b05b660b1f60ac9a16b9f28d1a3e5c12cba";
const WINDOWS: &str = "x86_64-pc-windows-msvc";
const LINUX: &str = "x86_64-unknown-linux-gnu";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: u32,
    payload: String,
    signature: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    format: u32,
    contract: String,
    members: [Member; 2],
    games: Vec<Game>,
    #[serde(default)]
    core: Option<CoreCoverage>,
    fp_controls: u32,
    evidence: String,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
enum CoreCoverage {
    #[serde(rename = "nes-standard-portable-rollback-v1")]
    StandardNes,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Member {
    artifact: String,
    source: String,
    version: String,
    target: String,
    test: bool,
    profile: String,
    opt_level: String,
    debug: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Game {
    source: String,
    mapper: u16,
    submapper: u8,
    timing: u8,
}

#[derive(Clone, Serialize)]
pub(crate) struct CertifiedMember {
    artifact: [u8; 32],
    source: [u8; 32],
    version: String,
    target: String,
    test: bool,
    profile: String,
    opt_level: String,
    debug: String,
}

#[derive(Clone, Serialize)]
pub(crate) struct CertifiedGame {
    source: [u8; 32],
    mapper: u16,
    submapper: u8,
    timing: u8,
}

#[derive(Clone, Serialize)]
pub(crate) struct CertificateSummary {
    pub(crate) certificate: [u8; 32],
    pub(crate) contract: [u8; 32],
    pub(crate) peer_build: [u8; 32],
    evidence: [u8; 32],
    fp_controls: u32,
    local_member: usize,
    members: [CertifiedMember; 2],
    games: Vec<CertifiedGame>,
    #[serde(skip_serializing_if = "Option::is_none")]
    core: Option<CoreCoverage>,
}

pub(super) struct LocalFacts<'a> {
    pub artifact: [u8; 32],
    pub source: &'a str,
    pub version: &'a str,
    pub target: &'a str,
    pub test: bool,
    pub profile: &'a str,
    pub opt_level: &'a str,
    pub debug: &'a str,
}

pub(super) struct VerifiedPair {
    summary: CertificateSummary,
}

impl VerifiedPair {
    pub(super) fn summary(&self) -> CertificateSummary {
        self.summary.clone()
    }

    pub(super) fn different_versions(&self) -> bool {
        self.summary.members[0].version != self.summary.members[1].version
    }

    pub(super) fn permits_game(
        &self,
        source: [u8; 32],
        mapper: u16,
        submapper: u8,
        timing: u8,
    ) -> bool {
        self.summary.games.iter().any(|game| {
            game.source == source
                && game.mapper == mapper
                && game.submapper == submapper
                && game.timing == timing
        })
    }

    pub(super) fn permits_content(
        &self,
        source: [u8; 32],
        mapper: u16,
        submapper: u8,
        timing: u8,
        portable_hardware: bool,
    ) -> bool {
        self.permits_game(source, mapper, submapper, timing)
            || (self.summary.core.is_some() && portable_hardware && timing <= 2)
    }
}

pub(super) fn read_sidecar(path: &Path) -> Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("opening artifact pair certificate"),
    };
    read_bounded(file).map(Some)
}

fn read_bounded(reader: impl Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take((MAX_SIZE + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_SIZE,
        "artifact pair certificate exceeds size limit"
    );
    Ok(bytes)
}

fn hex<const N: usize>(value: &str) -> Result<[u8; N]> {
    ensure!(
        value.len() == N * 2
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "invalid artifact pair hexadecimal field"
    );
    Ok(const_hex::decode_to_array(value)?)
}

fn digest(value: &str) -> Result<[u8; 32]> {
    let digest = hex(value)?;
    ensure!(digest != [0; 32], "artifact pair digest must be nonzero");
    Ok(digest)
}

fn visible_ascii(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

pub(super) fn verify(bytes: &[u8], facts: &LocalFacts<'_>) -> Result<VerifiedPair> {
    verify_with_key(bytes, facts, &VerifyingKey::from_bytes(&hex(PUBLIC_KEY)?)?)
}

fn verify_with_key(
    bytes: &[u8],
    facts: &LocalFacts<'_>,
    key: &VerifyingKey,
) -> Result<VerifiedPair> {
    ensure!(
        bytes.len() <= MAX_SIZE,
        "artifact pair certificate exceeds size limit"
    );
    let envelope: Envelope =
        serde_json::from_slice(bytes).context("invalid artifact pair envelope")?;
    ensure!(envelope.format == 1, "unsupported artifact pair envelope");
    let signature = Signature::from_bytes(&hex::<64>(&envelope.signature)?);
    let message = [DOMAIN, envelope.payload.as_bytes()].concat();
    key.verify_strict(&message, &signature)
        .context("invalid artifact pair signature")?;
    let payload: Payload =
        serde_json::from_str(&envelope.payload).context("invalid artifact pair payload")?;
    ensure!(
        matches!(payload.format, 1 | 2),
        "unsupported artifact pair payload"
    );
    ensure!(
        payload.fp_controls == REQUIRED_CONTROLS,
        "unsupported artifact pair floating point controls"
    );
    ensure!(
        (payload.format == 1
            && payload.core.is_none()
            && !payload.games.is_empty()
            && payload.games.len() <= 16)
            || (payload.format == 2 && payload.core.is_some() && payload.games.is_empty()),
        "invalid artifact pair game coverage"
    );
    let contract = digest(&payload.contract)?;
    let evidence = digest(&payload.evidence)?;
    let [first, second] = payload.members;
    let members = [member(first)?, member(second)?];
    ensure!(
        members[0].artifact != members[1].artifact,
        "duplicate artifact pair member"
    );
    ensure!(
        members[0].source == members[1].source,
        "artifact pair source snapshots differ"
    );
    ensure!(
        (members[0].target == WINDOWS && members[1].target == LINUX)
            || (members[0].target == LINUX && members[1].target == WINDOWS),
        "unsupported artifact pair targets"
    );
    let local_member = members
        .iter()
        .position(|member| member.artifact == facts.artifact)
        .context("actual executable is absent from artifact pair certificate")?;
    let local = &members[local_member];
    ensure!(
        local.source == digest(facts.source)?
            && local.version == facts.version
            && local.target == facts.target
            && local.test == facts.test
            && local.profile == facts.profile
            && local.opt_level == facts.opt_level
            && local.debug == facts.debug,
        "artifact pair compiled facts mismatch"
    );
    let mut games: Vec<CertifiedGame> = Vec::with_capacity(payload.games.len());
    for game in payload.games {
        let source = digest(&game.source)?;
        ensure!(
            matches!(game.mapper, 0 | 2 | 4 | 34) && game.submapper == 0 && game.timing <= 2,
            "unsupported artifact pair game hardware"
        );
        ensure!(
            !games.iter().any(|game| game.source == source),
            "duplicate artifact pair game"
        );
        games.push(CertifiedGame {
            source,
            mapper: game.mapper,
            submapper: game.submapper,
            timing: game.timing,
        });
    }
    Ok(VerifiedPair {
        summary: CertificateSummary {
            certificate: Sha256::digest(&message).into(),
            contract,
            peer_build: members[1 - local_member].artifact,
            evidence,
            fp_controls: payload.fp_controls,
            local_member,
            members,
            games,
            core: payload.core,
        },
    })
}

fn member(member: Member) -> Result<CertifiedMember> {
    ensure!(
        visible_ascii(&member.version) && visible_ascii(&member.profile),
        "invalid artifact pair version or profile"
    );
    ensure!(
        matches!(member.opt_level.as_str(), "0" | "1" | "2" | "3" | "s" | "z"),
        "invalid artifact pair optimization level"
    );
    ensure!(
        matches!(member.debug.as_str(), "true" | "false"),
        "invalid artifact pair debug setting"
    );
    Ok(CertifiedMember {
        artifact: digest(&member.artifact)?,
        source: digest(&member.source)?,
        version: member.version,
        target: member.target,
        test: member.test,
        profile: member.profile,
        opt_level: member.opt_level,
        debug: member.debug,
    })
}

#[cfg(test)]
mod tests;
