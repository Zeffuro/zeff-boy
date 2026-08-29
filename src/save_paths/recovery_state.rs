use std::fmt::{Display, Formatter};
use std::path::PathBuf;

const MAGIC: [u8; 8] = *b"ZRECSTAT";
const VERSION: u16 = 1;
const GENERATION_MAGIC: [u8; 8] = *b"ZBATGEN1";
const MAX_TEXT_BYTES: usize = 255;
const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BatteryGenerationWitness {
    Unknown,
    Committed {
        generation: u64,
        component_sha256: [u8; 32],
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryFreshness {
    Fresh,
    Stale,
    Unknown,
    Inconsistent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryStateIdentity<'a> {
    pub(crate) system: &'a str,
    pub(crate) discriminator: &'a str,
    pub(crate) media_sha256: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryStateEnvelope {
    pub(crate) system: String,
    pub(crate) discriminator: String,
    pub(crate) media_sha256: [u8; 32],
    pub(crate) battery: BatteryGenerationWitness,
    pub(crate) native_payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BatteryGenerationRecord {
    pub(crate) generation: u64,
    pub(crate) component_sha256: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryStateError {
    InvalidMagic,
    UnsupportedVersion(u16),
    Truncated,
    TrailingBytes,
    InvalidUtf8,
    InvalidWitness(u8),
    EmptyIdentity,
    FieldTooLong,
    PayloadTooLarge,
    PayloadHashMismatch,
    WrongIdentity,
    UnsafePathComponent,
}

impl Display for RecoveryStateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMagic => formatter.write_str("invalid recovery-state magic"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported recovery-state version {version}")
            }
            Self::Truncated => formatter.write_str("truncated recovery-state envelope"),
            Self::TrailingBytes => {
                formatter.write_str("recovery-state envelope has trailing bytes")
            }
            Self::InvalidUtf8 => formatter.write_str("recovery-state identity is not UTF-8"),
            Self::InvalidWitness(tag) => {
                write!(formatter, "invalid recovery-state battery witness {tag}")
            }
            Self::EmptyIdentity => formatter.write_str("recovery-state identity is empty"),
            Self::FieldTooLong => formatter.write_str("recovery-state identity field is too long"),
            Self::PayloadTooLarge => formatter.write_str("recovery-state payload exceeds 16 MiB"),
            Self::PayloadHashMismatch => {
                formatter.write_str("recovery-state payload hash does not match")
            }
            Self::WrongIdentity => formatter.write_str("recovery-state identity does not match"),
            Self::UnsafePathComponent => {
                formatter.write_str("unsafe recovery-state path component")
            }
        }
    }
}

impl std::error::Error for RecoveryStateError {}

pub(crate) fn classify_recovery_freshness(
    captured: &BatteryGenerationWitness,
    current: &BatteryGenerationWitness,
) -> RecoveryFreshness {
    let (
        BatteryGenerationWitness::Committed {
            generation: captured_generation,
            component_sha256: captured_hash,
        },
        BatteryGenerationWitness::Committed {
            generation: current_generation,
            component_sha256: current_hash,
        },
    ) = (captured, current)
    else {
        return RecoveryFreshness::Unknown;
    };

    match captured_generation.cmp(current_generation) {
        std::cmp::Ordering::Less => RecoveryFreshness::Stale,
        std::cmp::Ordering::Greater => RecoveryFreshness::Inconsistent,
        std::cmp::Ordering::Equal if captured_hash == current_hash => RecoveryFreshness::Fresh,
        std::cmp::Ordering::Equal => RecoveryFreshness::Inconsistent,
    }
}

pub(crate) fn canonical_battery_component_hash(components: &[(&str, &[u8])]) -> [u8; 32] {
    let mut components = components.to_vec();
    components.sort_unstable_by_key(|(name, _)| *name);
    let mut canonical = Vec::new();
    canonical.extend_from_slice(&(components.len() as u32).to_le_bytes());
    for (name, bytes) in components {
        canonical.extend_from_slice(&(name.len() as u32).to_le_bytes());
        canonical.extend_from_slice(name.as_bytes());
        canonical.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        canonical.extend_from_slice(&zeff_firmware::sha256_bytes(bytes));
    }
    zeff_firmware::sha256_bytes(&canonical)
}

pub(crate) fn reconcile_battery_generation(
    previous: Option<BatteryGenerationRecord>,
    component_sha256: [u8; 32],
) -> Option<BatteryGenerationRecord> {
    match previous {
        Some(previous) if previous.component_sha256 == component_sha256 => Some(previous),
        Some(previous) => Some(BatteryGenerationRecord {
            generation: previous.generation.checked_add(1)?,
            component_sha256,
        }),
        None => Some(BatteryGenerationRecord {
            generation: 0,
            component_sha256,
        }),
    }
}

pub(crate) fn encode_battery_generation(
    media_sha256: [u8; 32],
    record: BatteryGenerationRecord,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(80);
    bytes.extend_from_slice(&GENERATION_MAGIC);
    bytes.extend_from_slice(&media_sha256);
    bytes.extend_from_slice(&record.generation.to_le_bytes());
    bytes.extend_from_slice(&record.component_sha256);
    bytes
}

pub(crate) fn decode_battery_generation(
    bytes: &[u8],
    expected_media_sha256: [u8; 32],
) -> Option<BatteryGenerationRecord> {
    if bytes.len() != 80 || bytes[..8] != GENERATION_MAGIC || bytes[8..40] != expected_media_sha256
    {
        return None;
    }
    Some(BatteryGenerationRecord {
        generation: u64::from_le_bytes(bytes[40..48].try_into().ok()?),
        component_sha256: bytes[48..80].try_into().ok()?,
    })
}

pub(crate) fn encode_recovery_state(
    envelope: &RecoveryStateEnvelope,
) -> Result<Vec<u8>, RecoveryStateError> {
    let system = checked_text(envelope.system.as_bytes())?;
    let discriminator = checked_text(envelope.discriminator.as_bytes())?;
    if envelope.native_payload.len() > MAX_PAYLOAD_BYTES {
        return Err(RecoveryStateError::PayloadTooLarge);
    }

    let witness_bytes = match &envelope.battery {
        BatteryGenerationWitness::Unknown => 1,
        BatteryGenerationWitness::Committed { .. } => 1 + 8 + 32,
    };
    let capacity = MAGIC.len()
        + 2
        + 2
        + 2
        + 32
        + witness_bytes
        + 8
        + 32
        + system.len()
        + discriminator.len()
        + envelope.native_payload.len();
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&(system.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&(discriminator.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&envelope.media_sha256);
    match &envelope.battery {
        BatteryGenerationWitness::Unknown => bytes.push(0),
        BatteryGenerationWitness::Committed {
            generation,
            component_sha256,
        } => {
            bytes.push(1);
            bytes.extend_from_slice(&generation.to_le_bytes());
            bytes.extend_from_slice(component_sha256);
        }
    }
    bytes.extend_from_slice(&(envelope.native_payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&zeff_firmware::sha256_bytes(&envelope.native_payload));
    bytes.extend_from_slice(system);
    bytes.extend_from_slice(discriminator);
    bytes.extend_from_slice(&envelope.native_payload);
    Ok(bytes)
}

pub(crate) fn decode_recovery_state(
    bytes: &[u8],
    expected: RecoveryStateIdentity<'_>,
) -> Result<RecoveryStateEnvelope, RecoveryStateError> {
    let mut decoder = Decoder::new(bytes);
    if decoder.take(MAGIC.len())? != MAGIC {
        return Err(RecoveryStateError::InvalidMagic);
    }
    let version = decoder.u16()?;
    if version != VERSION {
        return Err(RecoveryStateError::UnsupportedVersion(version));
    }
    let system_len = usize::from(decoder.u16()?);
    let discriminator_len = usize::from(decoder.u16()?);
    if system_len > MAX_TEXT_BYTES || discriminator_len > MAX_TEXT_BYTES {
        return Err(RecoveryStateError::FieldTooLong);
    }
    let media_sha256 = decoder.array_32()?;
    let battery = match decoder.u8()? {
        0 => BatteryGenerationWitness::Unknown,
        1 => BatteryGenerationWitness::Committed {
            generation: decoder.u64()?,
            component_sha256: decoder.array_32()?,
        },
        tag => return Err(RecoveryStateError::InvalidWitness(tag)),
    };
    let payload_len_u64 = decoder.u64()?;
    if payload_len_u64 > MAX_PAYLOAD_BYTES as u64 {
        return Err(RecoveryStateError::PayloadTooLarge);
    }
    let payload_len =
        usize::try_from(payload_len_u64).map_err(|_| RecoveryStateError::PayloadTooLarge)?;
    let payload_sha256 = decoder.array_32()?;
    let system = std::str::from_utf8(decoder.take(system_len)?)
        .map_err(|_| RecoveryStateError::InvalidUtf8)?
        .to_owned();
    let discriminator = std::str::from_utf8(decoder.take(discriminator_len)?)
        .map_err(|_| RecoveryStateError::InvalidUtf8)?
        .to_owned();
    if system != expected.system
        || discriminator != expected.discriminator
        || media_sha256 != expected.media_sha256
    {
        return Err(RecoveryStateError::WrongIdentity);
    }
    let native_payload = decoder.take(payload_len)?.to_vec();
    if !decoder.is_empty() {
        return Err(RecoveryStateError::TrailingBytes);
    }
    if zeff_firmware::sha256_bytes(&native_payload) != payload_sha256 {
        return Err(RecoveryStateError::PayloadHashMismatch);
    }

    Ok(RecoveryStateEnvelope {
        system,
        discriminator,
        media_sha256,
        battery,
        native_payload,
    })
}

pub(crate) fn recovery_state_path(
    system: &str,
    state_extension: &str,
    media_sha256: [u8; 32],
) -> Result<PathBuf, RecoveryStateError> {
    validate_path_component(system)?;
    validate_path_component(state_extension)?;
    Ok(crate::platform::save_dir(system)
        .join("recovery")
        .join(const_hex::encode(media_sha256))
        .join("state")
        .join(format!("last.{state_extension}")))
}

pub(crate) fn battery_generation_path(
    system: &str,
    media_sha256: [u8; 32],
) -> Result<PathBuf, RecoveryStateError> {
    validate_path_component(system)?;
    Ok(crate::platform::save_dir(system)
        .join("recovery")
        .join(const_hex::encode(media_sha256))
        .join("battery-generation.bin"))
}

fn checked_text(bytes: &[u8]) -> Result<&[u8], RecoveryStateError> {
    if bytes.is_empty() {
        return Err(RecoveryStateError::EmptyIdentity);
    }
    if bytes.len() > MAX_TEXT_BYTES {
        return Err(RecoveryStateError::FieldTooLong);
    }
    Ok(bytes)
}

fn validate_path_component(component: &str) -> Result<(), RecoveryStateError> {
    if component.is_empty()
        || !component
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(RecoveryStateError::UnsafePathComponent);
    }
    Ok(())
}

struct Decoder<'a> {
    remaining: &'a [u8],
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { remaining: bytes }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], RecoveryStateError> {
        let (value, remaining) = self
            .remaining
            .split_at_checked(len)
            .ok_or(RecoveryStateError::Truncated)?;
        self.remaining = remaining;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, RecoveryStateError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, RecoveryStateError> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("length checked"),
        ))
    }

    fn u64(&mut self) -> Result<u64, RecoveryStateError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("length checked"),
        ))
    }

    fn array_32(&mut self) -> Result<[u8; 32], RecoveryStateError> {
        Ok(self.take(32)?.try_into().expect("length checked"))
    }

    fn is_empty(&self) -> bool {
        self.remaining.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity<'a>() -> RecoveryStateIdentity<'a> {
        RecoveryStateIdentity {
            system: "gbc",
            discriminator: "zeff-gb-native-v4",
            media_sha256: [0x11; 32],
        }
    }

    fn envelope() -> RecoveryStateEnvelope {
        RecoveryStateEnvelope {
            system: identity().system.to_owned(),
            discriminator: identity().discriminator.to_owned(),
            media_sha256: identity().media_sha256,
            battery: BatteryGenerationWitness::Committed {
                generation: 42,
                component_sha256: [0x22; 32],
            },
            native_payload: b"native-state".to_vec(),
        }
    }

    #[test]
    fn envelope_roundtrip() {
        let expected = envelope();
        let encoded = encode_recovery_state(&expected).unwrap();
        assert_eq!(
            decode_recovery_state(&encoded, identity()).unwrap(),
            expected
        );
        let unknown = RecoveryStateEnvelope {
            battery: BatteryGenerationWitness::Unknown,
            ..envelope()
        };
        let encoded = encode_recovery_state(&unknown).unwrap();
        assert_eq!(
            decode_recovery_state(&encoded, identity()).unwrap(),
            unknown
        );
    }

    #[test]
    fn every_truncation_is_rejected() {
        let encoded = encode_recovery_state(&envelope()).unwrap();
        for len in 0..encoded.len() {
            assert!(decode_recovery_state(&encoded[..len], identity()).is_err());
        }
    }

    #[test]
    fn payload_corruption_and_trailing_bytes_are_rejected() {
        let mut corrupt = encode_recovery_state(&envelope()).unwrap();
        *corrupt.last_mut().unwrap() ^= 0x80;
        assert_eq!(
            decode_recovery_state(&corrupt, identity()).unwrap_err(),
            RecoveryStateError::PayloadHashMismatch
        );

        let mut trailing = encode_recovery_state(&envelope()).unwrap();
        trailing.push(0);
        assert_eq!(
            decode_recovery_state(&trailing, identity()).unwrap_err(),
            RecoveryStateError::TrailingBytes
        );
    }

    #[test]
    fn wrong_system_discriminator_and_media_are_rejected() {
        let encoded = encode_recovery_state(&envelope()).unwrap();
        for wrong in [
            RecoveryStateIdentity {
                system: "gba",
                ..identity()
            },
            RecoveryStateIdentity {
                discriminator: "other",
                ..identity()
            },
            RecoveryStateIdentity {
                media_sha256: [0x33; 32],
                ..identity()
            },
        ] {
            assert_eq!(
                decode_recovery_state(&encoded, wrong).unwrap_err(),
                RecoveryStateError::WrongIdentity
            );
        }
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let mut encoded = encode_recovery_state(&envelope()).unwrap();
        encoded[MAGIC.len()..MAGIC.len() + 2].copy_from_slice(&2_u16.to_le_bytes());
        assert_eq!(
            decode_recovery_state(&encoded, identity()).unwrap_err(),
            RecoveryStateError::UnsupportedVersion(2)
        );
    }

    #[test]
    fn freshness_uses_generation_and_component_witness() {
        let known = |generation, byte| BatteryGenerationWitness::Committed {
            generation,
            component_sha256: [byte; 32],
        };
        assert_eq!(
            classify_recovery_freshness(&known(4, 1), &known(4, 1)),
            RecoveryFreshness::Fresh
        );
        assert_eq!(
            classify_recovery_freshness(&known(3, 1), &known(4, 2)),
            RecoveryFreshness::Stale
        );
        assert_eq!(
            classify_recovery_freshness(&BatteryGenerationWitness::Unknown, &known(4, 1)),
            RecoveryFreshness::Unknown
        );
        assert_eq!(
            classify_recovery_freshness(&known(4, 1), &known(4, 2)),
            RecoveryFreshness::Inconsistent
        );
        assert_eq!(
            classify_recovery_freshness(&known(5, 1), &known(4, 1)),
            RecoveryFreshness::Inconsistent
        );
    }

    #[test]
    fn payload_and_identity_fields_are_bounded() {
        let mut oversized = envelope();
        oversized.native_payload = vec![0; MAX_PAYLOAD_BYTES + 1];
        assert_eq!(
            encode_recovery_state(&oversized).unwrap_err(),
            RecoveryStateError::PayloadTooLarge
        );

        let mut long_identity = envelope();
        long_identity.discriminator = "x".repeat(MAX_TEXT_BYTES + 1);
        assert_eq!(
            encode_recovery_state(&long_identity).unwrap_err(),
            RecoveryStateError::FieldTooLong
        );

        let mut declared_oversized = encode_recovery_state(&envelope()).unwrap();
        let payload_len_offset = MAGIC.len() + 2 + 2 + 2 + 32 + 1 + 8 + 32;
        declared_oversized[payload_len_offset..payload_len_offset + 8]
            .copy_from_slice(&((MAX_PAYLOAD_BYTES as u64) + 1).to_le_bytes());
        assert_eq!(
            decode_recovery_state(&declared_oversized, identity()).unwrap_err(),
            RecoveryStateError::PayloadTooLarge
        );
    }

    #[test]
    fn recovery_path_uses_full_identity_and_rejects_unsafe_components() {
        let path = recovery_state_path("gbc", "gbstate", [0xAB; 32]).unwrap();
        assert!(
            path.ends_with(
                PathBuf::from("gbc")
                    .join("recovery")
                    .join("ab".repeat(32))
                    .join("state")
                    .join("last.gbstate")
            )
        );
        assert_eq!(
            recovery_state_path("../gbc", "gbstate", [0; 32]).unwrap_err(),
            RecoveryStateError::UnsafePathComponent
        );
        assert_eq!(
            recovery_state_path("gbc", "state.bak", [0; 32]).unwrap_err(),
            RecoveryStateError::UnsafePathComponent
        );
    }

    #[test]
    fn content_witness_is_order_independent_and_component_wide() {
        let left = canonical_battery_component_hash(&[("bram", b"a"), ("memory", b"b")]);
        let reordered = canonical_battery_component_hash(&[("memory", b"b"), ("bram", b"a")]);
        let changed = canonical_battery_component_hash(&[("bram", b"a"), ("memory", b"c")]);

        assert_eq!(left, reordered);
        assert_ne!(left, changed);
    }

    #[test]
    fn generation_record_roundtrips_and_reconciles_lag() {
        let media = [0x44; 32];
        let initial = reconcile_battery_generation(None, [1; 32]).unwrap();
        assert_eq!(initial.generation, 0);
        assert_eq!(
            decode_battery_generation(&encode_battery_generation(media, initial), media),
            Some(initial)
        );
        assert_eq!(
            decode_battery_generation(&encode_battery_generation(media, initial), [0x45; 32]),
            None
        );

        let unchanged = reconcile_battery_generation(Some(initial), [1; 32]).unwrap();
        let reconciled = reconcile_battery_generation(Some(initial), [2; 32]).unwrap();
        assert_eq!(unchanged.generation, 0);
        assert_eq!(reconciled.generation, 1);
    }
}
