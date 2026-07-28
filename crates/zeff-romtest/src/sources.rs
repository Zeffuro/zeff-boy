use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::model::LicenseConfidence;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SourceCatalog {
    pub(crate) catalog_version: u32,
    #[serde(default)]
    pub(crate) sources: Vec<SourceSpec>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SourceSpec {
    pub(crate) id: String,
    pub(crate) kind: SourceKind,
    pub(crate) url: String,
    pub(crate) sha256: String,
    pub(crate) license: String,
    #[serde(default = "crate::model::default_license_confidence")]
    pub(crate) license_confidence: LicenseConfidence,
    #[serde(default)]
    pub(crate) redistributable: bool,
    pub(crate) notes: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceKind {
    File,
    Zip,
}

pub(crate) fn load_sources(path: &Path) -> anyhow::Result<HashMap<String, SourceSpec>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read source catalog {}", path.display()))?;
    let catalog: SourceCatalog = toml::from_str(&text)
        .with_context(|| format!("failed to parse source catalog {}", path.display()))?;
    if catalog.catalog_version != 1 {
        bail!(
            "{} uses unsupported catalog_version {}",
            path.display(),
            catalog.catalog_version
        );
    }

    let mut sources = HashMap::new();
    for source in catalog.sources {
        if source.id.trim().is_empty() {
            bail!("{} contains a source with an empty id", path.display());
        }
        if source.url.trim().is_empty() {
            bail!("{} source '{}' has an empty URL", path.display(), source.id);
        }
        if source.sha256.trim().is_empty() {
            bail!(
                "{} source '{}' has an empty sha256",
                path.display(),
                source.id
            );
        }
        if source.license.trim().is_empty() {
            bail!(
                "{} source '{}' has an empty license",
                path.display(),
                source.id
            );
        }
        if source
            .notes
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            bail!("{} source '{}' has empty notes", path.display(), source.id);
        }
        if sources.insert(source.id.clone(), source).is_some() {
            bail!("{} contains duplicate source id", path.display());
        }
    }

    Ok(sources)
}
