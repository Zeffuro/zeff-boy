use zeff_emu_common::system::System;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RomIdentity {
    pub(crate) system: System,
    pub(crate) sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum IdentityStatus {
    Exact,
    Mismatch,
    #[default]
    Unverified,
}

impl IdentityStatus {
    pub(crate) fn compare(expected: Option<RomIdentity>, actual: RomIdentity) -> Self {
        match expected {
            Some(expected) if expected == actual => Self::Exact,
            Some(_) => Self::Mismatch,
            None => Self::Unverified,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Exact => "Exact",
            Self::Mismatch => "Mismatch",
            Self::Unverified => "Unverified",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_status_distinguishes_exact_mismatch_and_missing_hashes() {
        let actual = RomIdentity {
            system: System::Gb,
            sha256: [1; 32],
        };
        assert_eq!(
            IdentityStatus::compare(Some(actual), actual),
            IdentityStatus::Exact
        );
        assert_eq!(
            IdentityStatus::compare(
                Some(RomIdentity {
                    sha256: [2; 32],
                    ..actual
                }),
                actual,
            ),
            IdentityStatus::Mismatch
        );
        assert_eq!(
            IdentityStatus::compare(None, actual),
            IdentityStatus::Unverified
        );
    }
}
