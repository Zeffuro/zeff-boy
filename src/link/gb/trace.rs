use std::io::Write;
use std::path::PathBuf;

use crate::link::LinkEndpointId;

pub(super) struct LinkTrace {
    file: std::fs::File,
}

impl LinkTrace {
    pub(super) fn from_env(endpoint: LinkEndpointId) -> Option<Self> {
        let raw = std::env::var("ZEFF_BOY_LINK_TRACE").ok()?;
        if raw.trim().is_empty() || raw.eq_ignore_ascii_case("0") {
            return None;
        }

        let path = trace_path(&raw, endpoint);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        Some(Self { file })
    }

    pub(super) fn write(&mut self, message: &str) {
        let _ = writeln!(self.file, "{message}");
    }
}

fn trace_path(raw: &str, endpoint: LinkEndpointId) -> PathBuf {
    let pid = std::process::id();
    if raw == "1" {
        return PathBuf::from(".tmp").join(format!("zeff-boy-link-{pid}-e{}.log", endpoint.0));
    }

    let substituted = raw
        .replace("{pid}", &pid.to_string())
        .replace("{endpoint}", &endpoint.0.to_string());
    let path = PathBuf::from(substituted);
    if path.extension().is_some() {
        path
    } else {
        path.join(format!("zeff-boy-link-{pid}-e{}.log", endpoint.0))
    }
}
