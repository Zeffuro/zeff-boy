use anyhow::Context;
use serde::Serialize;

use super::{Budget, ScanStop};

mod inspect;
mod it;
mod s3m;

pub mod xm;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddedFormat {
    Xm,
    Mod,
    S3m,
    It,
}

impl EmbeddedFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Xm => "FastTracker II XM",
            Self::Mod => "ProTracker MOD",
            Self::S3m => "Scream Tracker 3 S3M",
            Self::It => "Impulse Tracker IT",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Xm => "xm",
            Self::Mod => "mod",
            Self::S3m => "s3m",
            Self::It => "it",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct FileSpan {
    pub offset: u32,
    pub byte_len: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EmbeddedModule {
    pub format: EmbeddedFormat,
    pub span: FileSpan,
    pub name: String,
    pub channels: u16,
    pub orders: u16,
    pub patterns: u16,
    pub instruments: u16,
    pub samples: u16,
    pub sample_points: u32,
    pub source: ModuleSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModuleSource {
    Embedded,
    Standalone { trailing_bytes: u32 },
}

pub(crate) fn scan(
    bytes: &[u8],
    modules: &mut Vec<EmbeddedModule>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    let mut covered_until = 0;
    for chunk_start in (0..bytes.len()).step_by(256) {
        budget.charge()?;
        for offset in chunk_start..(chunk_start + 256).min(bytes.len()) {
            if offset < covered_until {
                continue;
            }
            let result = if bytes[offset..].starts_with(b"Extended Module: ") {
                inspect::xm(bytes, offset, budget)?
            } else if offset >= 1080 && inspect::mod_channels(&bytes[offset..]).is_some() {
                inspect::mod_file(bytes, offset - 1080, budget)?
            } else if bytes[offset..].starts_with(b"IMPM") {
                it::parse(bytes, offset, budget)?
            } else if offset >= 44 && bytes[offset..].starts_with(b"SCRM") {
                s3m::parse(bytes, offset - 44, budget)?
            } else {
                None
            };
            if let Some(module) = result {
                if (module.span.offset as usize) < covered_until {
                    continue;
                }
                if modules.len() >= remaining {
                    return Err(ScanStop::CandidateLimit);
                }
                covered_until = module.span.offset as usize + module.span.byte_len as usize;
                modules.push(module);
            }
        }
    }
    Ok(())
}

pub(crate) fn scan_standalone(
    bytes: &[u8],
    expected: EmbeddedFormat,
    budget: &mut Budget<'_>,
) -> Result<Option<EmbeddedModule>, ScanStop> {
    let module = match expected {
        EmbeddedFormat::Xm if bytes.starts_with(b"Extended Module: ") => {
            inspect::xm(bytes, 0, budget)?
        }
        EmbeddedFormat::Mod
            if bytes.len() >= 1084 && inspect::mod_channels(&bytes[1080..]).is_some() =>
        {
            inspect::mod_file(bytes, 0, budget)?
        }
        EmbeddedFormat::S3m if bytes.get(44..48) == Some(b"SCRM") => s3m::parse(bytes, 0, budget)?,
        EmbeddedFormat::It if bytes.starts_with(b"IMPM") => it::parse(bytes, 0, budget)?,
        _ => None,
    };
    Ok(module.map(|mut module| {
        module.source = ModuleSource::Standalone {
            trailing_bytes: (bytes.len() - module.span.byte_len as usize) as u32,
        };
        module
    }))
}

pub fn verify_original(
    bytes: &[u8],
    expected: &EmbeddedModule,
    cancel: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: super::MAX_SCAN_WORK,
    };
    let actual = match expected.format {
        EmbeddedFormat::Xm => inspect::xm(bytes, expected.span.offset as usize, &mut budget),
        EmbeddedFormat::Mod => inspect::mod_file(bytes, expected.span.offset as usize, &mut budget),
        EmbeddedFormat::S3m => s3m::parse(bytes, expected.span.offset as usize, &mut budget),
        EmbeddedFormat::It => it::parse(bytes, expected.span.offset as usize, &mut budget),
    };
    let actual = actual.map_err(|reason| match reason {
        ScanStop::Cancelled => anyhow::anyhow!("tracker export cancelled"),
        other => anyhow::anyhow!("tracker verification stopped: {other:?}"),
    })?;
    let actual = actual
        .map(|mut module| match expected.source {
            ModuleSource::Embedded => Ok(module),
            ModuleSource::Standalone { trailing_bytes } => {
                anyhow::ensure!(
                    expected.span.offset == 0,
                    "standalone tracker module must begin at byte offset zero"
                );
                let actual_trailing = bytes
                    .len()
                    .checked_sub(module.span.byte_len as usize)
                    .context("standalone tracker module exceeds its source media")?;
                anyhow::ensure!(
                    actual_trailing == trailing_bytes as usize,
                    "standalone tracker trailing-byte count does not match its source media"
                );
                module.source = ModuleSource::Standalone { trailing_bytes };
                Ok(module)
            }
        })
        .transpose()?;
    anyhow::ensure!(
        actual.as_ref() == Some(expected),
        "tracker module does not match its validated inventory"
    );
    Ok(())
}

#[cfg(test)]
mod metadata_tests;
#[cfg(test)]
mod tests;
