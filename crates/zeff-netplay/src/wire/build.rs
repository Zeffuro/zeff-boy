use anyhow::{Result, ensure};

pub(super) const ENCODED_LEN: usize = 163;
const VERSION_CAPACITY: usize = 64;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "native-proof", derive(serde::Serialize))]
pub struct BuildInfo {
    pub version: String,
    pub contract: [u8; 32],
    pub certificate: [u8; 32],
    pub peer_build: [u8; 32],
    /// 0: exact artifact only; 1: Windows x86_64; 2: Linux x86_64.
    pub platform: u8,
    pub allow_different_versions: bool,
}

impl BuildInfo {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.version.len() <= VERSION_CAPACITY
                && self
                    .version
                    .bytes()
                    .all(|byte| (0x21..=0x7e).contains(&byte)),
            "invalid App version"
        );
        if self.contract == [0; 32] {
            ensure!(
                self.platform == 0 && self.certificate == [0; 32] && self.peer_build == [0; 32],
                "unqualified build fields must be zero"
            );
        } else {
            ensure!(matches!(self.platform, 1 | 2), "unknown build platform");
            ensure!(
                !self.version.is_empty(),
                "qualified build requires App version"
            );
        }
        ensure!(
            (self.certificate == [0; 32]) == (self.peer_build == [0; 32]),
            "artifact pair certificate requires an expected peer build"
        );
        Ok(())
    }

    pub(super) fn encode(&self) -> Result<[u8; ENCODED_LEN]> {
        self.validate()?;
        let mut bytes = [0; ENCODED_LEN];
        bytes[0] = self.version.len() as u8;
        bytes[1..1 + self.version.len()].copy_from_slice(self.version.as_bytes());
        bytes[65..97].copy_from_slice(&self.contract);
        bytes[97] = self.platform;
        bytes[98] = u8::from(self.allow_different_versions);
        bytes[99..131].copy_from_slice(&self.certificate);
        bytes[131..163].copy_from_slice(&self.peer_build);
        Ok(bytes)
    }

    pub(super) fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() == ENCODED_LEN,
            "invalid build descriptor length"
        );
        let length = usize::from(bytes[0]);
        ensure!(length <= VERSION_CAPACITY, "invalid App version length");
        ensure!(
            bytes[1 + length..65].iter().all(|byte| *byte == 0),
            "invalid App version padding"
        );
        ensure!(bytes[98] <= 1, "invalid different-version consent flag");
        let info = Self {
            version: std::str::from_utf8(&bytes[1..1 + length])?.to_owned(),
            contract: bytes[65..97].try_into()?,
            certificate: bytes[99..131].try_into()?,
            peer_build: bytes[131..163].try_into()?,
            platform: bytes[97],
            allow_different_versions: bytes[98] == 1,
        };
        info.validate()?;
        Ok(info)
    }
}

pub(super) fn admit_builds(
    local_sha: &[u8; 32],
    local: &BuildInfo,
    remote_sha: &[u8; 32],
    remote: &BuildInfo,
) -> Result<()> {
    local.validate()?;
    remote.validate()?;
    if local.version != remote.version {
        ensure!(
            local.allow_different_versions && remote.allow_different_versions,
            "different App versions require both players' consent"
        );
    }
    if local_sha != remote_sha {
        ensure!(
            local.contract != [0; 32] && local.contract == remote.contract,
            "build compatibility contract mismatch"
        );
        ensure!(
            matches!(local.platform, 1 | 2) && matches!(remote.platform, 1 | 2),
            "unqualified build platform"
        );
        if local.certificate != [0; 32] || remote.certificate != [0; 32] {
            ensure!(
                local.certificate != [0; 32] && local.certificate == remote.certificate,
                "artifact pair certificate mismatch"
            );
            ensure!(
                local.peer_build == *remote_sha && remote.peer_build == *local_sha,
                "artifact pair expected peer mismatch"
            );
        }
    } else {
        if local.certificate != [0; 32] || remote.certificate != [0; 32] {
            return Ok(());
        }
        ensure!(
            local.contract == remote.contract,
            "build compatibility contract mismatch"
        );
        ensure!(
            local.platform == remote.platform,
            "same artifact platform mismatch"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_visible_version_boundary_are_canonical() {
        for info in [
            BuildInfo::default(),
            BuildInfo {
                version: "v".repeat(64),
                contract: [1; 32],
                platform: 1,
                allow_different_versions: true,
                ..BuildInfo::default()
            },
        ] {
            assert_eq!(BuildInfo::decode(&info.encode().unwrap()).unwrap(), info);
        }
    }

    #[test]
    fn local_descriptor_rejects_invalid_or_noncanonical_values() {
        for version in [
            "v".repeat(65),
            "1.0\n".into(),
            "1.0 beta".into(),
            "é".into(),
        ] {
            let info = BuildInfo {
                version,
                ..BuildInfo::default()
            };
            assert!(info.encode().is_err());
        }
        for (contract, platform, version) in [
            ([0; 32], 1, "1"),
            ([1; 32], 0, "1"),
            ([1; 32], 3, "1"),
            ([1; 32], 1, ""),
        ] {
            let info = BuildInfo {
                version: version.into(),
                contract,
                platform,
                allow_different_versions: false,
                ..BuildInfo::default()
            };
            assert!(info.encode().is_err());
        }
    }

    #[test]
    fn pair_fields_are_canonical_and_round_trip() {
        let info = BuildInfo {
            version: "1.0".into(),
            contract: [1; 32],
            certificate: [2; 32],
            peer_build: [3; 32],
            platform: 1,
            allow_different_versions: true,
        };
        assert_eq!(BuildInfo::decode(&info.encode().unwrap()).unwrap(), info);
        for (contract, certificate, peer_build) in [
            ([0; 32], [2; 32], [3; 32]),
            ([1; 32], [0; 32], [3; 32]),
            ([1; 32], [2; 32], [0; 32]),
        ] {
            let invalid = BuildInfo {
                contract,
                certificate,
                peer_build,
                ..info.clone()
            };
            assert!(invalid.encode().is_err());
        }
        let valid = info.encode().unwrap();
        for range in [65..97, 99..131, 131..163] {
            let mut malformed = valid;
            malformed[range].fill(0);
            assert!(BuildInfo::decode(&malformed).is_err());
        }
    }
}
