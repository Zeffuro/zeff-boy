use core::fmt;

use sha2::{Digest, Sha256};
use zeff_emu_common::media::MediaObjectId;

pub const FDS_HEADER_SIZE: usize = 16;
pub const FDS_SIDE_SIZE: usize = 65_500;

const FDS_HEADER_MAGIC: &[u8; 4] = b"FDS\x1A";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FdsImage {
    sides: Vec<Vec<u8>>,
    header: Option<[u8; FDS_HEADER_SIZE]>,
}

impl FdsImage {
    pub fn parse(bytes: &[u8]) -> Result<Self, FdsImageError> {
        if bytes.is_empty() {
            return Err(FdsImageError::Empty);
        }

        let (header, side_bytes, declared_sides) = if bytes.starts_with(FDS_HEADER_MAGIC) {
            if bytes.len() < FDS_HEADER_SIZE {
                return Err(FdsImageError::TruncatedHeader {
                    actual: bytes.len(),
                });
            }
            let mut header = [0; FDS_HEADER_SIZE];
            header.copy_from_slice(&bytes[..FDS_HEADER_SIZE]);
            let declared = usize::from(header[4]);
            if declared == 0 {
                return Err(FdsImageError::InvalidHeaderSideCount(0));
            }
            (Some(header), &bytes[FDS_HEADER_SIZE..], Some(declared))
        } else {
            (None, bytes, None)
        };

        if side_bytes.is_empty() {
            return Err(FdsImageError::NoSideData);
        }
        if side_bytes.len() % FDS_SIDE_SIZE != 0 {
            return Err(FdsImageError::SideDataLength {
                actual: side_bytes.len(),
            });
        }

        let actual_sides = side_bytes.len() / FDS_SIDE_SIZE;
        if actual_sides > usize::from(u8::MAX) {
            return Err(FdsImageError::TooManySides(actual_sides));
        }
        if let Some(declared) = declared_sides
            && declared != actual_sides
        {
            return Err(FdsImageError::HeaderSideCountMismatch {
                declared,
                actual: actual_sides,
            });
        }

        let sides = side_bytes
            .chunks_exact(FDS_SIDE_SIZE)
            .map(|side| side.to_vec())
            .collect();

        Ok(Self { sides, header })
    }

    pub fn side_count(&self) -> usize {
        self.sides.len()
    }

    pub fn side_data_len(&self) -> usize {
        self.sides.len() * FDS_SIDE_SIZE
    }

    pub fn media_object_id(&self) -> MediaObjectId {
        let mut hasher = Sha256::new();
        for side in &self.sides {
            hasher.update(side);
        }
        let digest = hasher.finalize();
        MediaObjectId::new(format!("sha256:{}", lower_hex(digest.as_ref())))
    }

    pub fn has_header(&self) -> bool {
        self.header.is_some()
    }

    pub fn header(&self) -> Option<&[u8; FDS_HEADER_SIZE]> {
        self.header.as_ref()
    }

    pub fn side(&self, index: usize) -> Option<&[u8]> {
        self.sides.get(index).map(Vec::as_slice)
    }

    pub fn sides(&self) -> impl Iterator<Item = &[u8]> {
        self.sides.iter().map(Vec::as_slice)
    }

    pub fn into_sides(self) -> Vec<Vec<u8>> {
        self.sides
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FdsImageError {
    Empty,
    TruncatedHeader { actual: usize },
    InvalidHeaderSideCount(u8),
    NoSideData,
    SideDataLength { actual: usize },
    HeaderSideCountMismatch { declared: usize, actual: usize },
    TooManySides(usize),
}

impl fmt::Display for FdsImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Empty => f.write_str("FDS image is empty"),
            Self::TruncatedHeader { actual } => write!(
                f,
                "FDS image header is truncated: expected {FDS_HEADER_SIZE} bytes, got {actual}"
            ),
            Self::InvalidHeaderSideCount(count) => {
                write!(f, "FDS image header declares invalid side count {count}")
            }
            Self::NoSideData => f.write_str("FDS image does not contain any disk side data"),
            Self::SideDataLength { actual } => write!(
                f,
                "FDS image side data length must be a multiple of {FDS_SIDE_SIZE} bytes, got {actual}"
            ),
            Self::HeaderSideCountMismatch { declared, actual } => write!(
                f,
                "FDS image header declares {declared} side(s), but data contains {actual}"
            ),
            Self::TooManySides(actual) => {
                write!(
                    f,
                    "FDS image has too many sides for fwNES metadata: {actual}"
                )
            }
        }
    }
}

impl std::error::Error for FdsImageError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(fill: u8) -> Vec<u8> {
        vec![fill; FDS_SIDE_SIZE]
    }

    #[test]
    fn parses_raw_side_data_without_header() {
        let mut bytes = side(0x11);
        bytes.extend_from_slice(&side(0x22));

        let image = FdsImage::parse(&bytes).expect("raw FDS image should parse");

        assert!(!image.has_header());
        assert_eq!(image.side_count(), 2);
        assert_eq!(image.side_data_len(), FDS_SIDE_SIZE * 2);
        assert_eq!(image.side(0).unwrap()[0], 0x11);
        assert_eq!(image.side(1).unwrap()[FDS_SIDE_SIZE - 1], 0x22);
    }

    #[test]
    fn parses_fw_nes_headered_side_data() {
        let mut bytes = [0; FDS_HEADER_SIZE].to_vec();
        bytes[..4].copy_from_slice(FDS_HEADER_MAGIC);
        bytes[4] = 1;
        bytes.extend_from_slice(&side(0xA5));

        let image = FdsImage::parse(&bytes).expect("headered FDS image should parse");

        assert!(image.has_header());
        assert_eq!(image.header().unwrap()[4], 1);
        assert_eq!(image.side_count(), 1);
        assert!(image.side(0).unwrap().iter().all(|byte| *byte == 0xA5));
    }

    #[test]
    fn media_id_hashes_canonical_side_data_not_container_header() {
        let raw = side(0x66);
        let mut headered = [0; FDS_HEADER_SIZE].to_vec();
        headered[..4].copy_from_slice(FDS_HEADER_MAGIC);
        headered[4] = 1;
        headered.extend_from_slice(&raw);

        let raw_image = FdsImage::parse(&raw).expect("raw FDS image should parse");
        let headered_image = FdsImage::parse(&headered).expect("headered FDS image should parse");

        assert_eq!(
            raw_image.media_object_id(),
            headered_image.media_object_id()
        );
        assert!(raw_image.media_object_id().as_ref().starts_with("sha256:"));
    }

    #[test]
    fn rejects_empty_images() {
        assert_eq!(FdsImage::parse(&[]), Err(FdsImageError::Empty));
    }

    #[test]
    fn rejects_truncated_headers() {
        assert_eq!(
            FdsImage::parse(b"FDS\x1A"),
            Err(FdsImageError::TruncatedHeader { actual: 4 })
        );
    }

    #[test]
    fn rejects_header_without_sides() {
        let mut bytes = [0; FDS_HEADER_SIZE];
        bytes[..4].copy_from_slice(FDS_HEADER_MAGIC);
        bytes[4] = 1;

        assert_eq!(FdsImage::parse(&bytes), Err(FdsImageError::NoSideData));
    }

    #[test]
    fn rejects_non_side_sized_data() {
        let bytes = vec![0; FDS_SIDE_SIZE + 1];

        assert_eq!(
            FdsImage::parse(&bytes),
            Err(FdsImageError::SideDataLength {
                actual: FDS_SIDE_SIZE + 1
            })
        );
    }

    #[test]
    fn rejects_header_side_count_mismatch() {
        let mut bytes = [0; FDS_HEADER_SIZE].to_vec();
        bytes[..4].copy_from_slice(FDS_HEADER_MAGIC);
        bytes[4] = 2;
        bytes.extend_from_slice(&side(0x00));

        assert_eq!(
            FdsImage::parse(&bytes),
            Err(FdsImageError::HeaderSideCountMismatch {
                declared: 2,
                actual: 1
            })
        );
    }
}
