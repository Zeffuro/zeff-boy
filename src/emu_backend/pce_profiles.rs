use zeff_firmware::PceSystemCardTier;
use zeff_pce_core::hardware::PceControllerMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PceCdTitleMetadata {
    pub(crate) id: &'static str,
    pub(crate) title: &'static str,
    pub(crate) region: &'static str,
    pub(crate) normalized_disc_sha256: [u8; 32],
    pub(crate) controller_mode: PceControllerMode,
    pub(crate) memory_base_128: bool,
    pub(crate) arcade_card: bool,
    pub(crate) minimum_system_card: Option<PceSystemCardTier>,
}

const fn super_cd_profile(
    id: &'static str,
    title: &'static str,
    normalized_disc_sha256: [u8; 32],
    controller_mode: PceControllerMode,
    memory_base_128: bool,
    arcade_card: bool,
) -> PceCdTitleMetadata {
    PceCdTitleMetadata {
        id,
        title,
        region: "JP",
        normalized_disc_sha256,
        controller_mode,
        memory_base_128,
        arcade_card,
        minimum_system_card: Some(PceSystemCardTier::Version3),
    }
}

pub(crate) const LEMMINGS_JAPAN_CANONICAL_DISC_SHA256: [u8; 32] = [
    0x91, 0x84, 0x9d, 0xb4, 0x25, 0x64, 0xfe, 0x20, 0x58, 0xa1, 0x79, 0xfd, 0x85, 0xa4, 0x51, 0x39,
    0x69, 0x1f, 0xca, 0x45, 0x88, 0xcd, 0x85, 0x75, 0xb4, 0x8f, 0x5d, 0x76, 0xdf, 0x9e, 0x37, 0x09,
];
pub(crate) const TENGAI_MAKYOU_DEDEN_NO_KABUKI_DEN_CANONICAL_DISC_SHA256: [u8; 32] = [
    0x43, 0x56, 0x1d, 0xc0, 0x9d, 0xa3, 0x83, 0x85, 0x1c, 0xae, 0xab, 0x1b, 0x77, 0x92, 0x79, 0xb0,
    0x46, 0x96, 0xbf, 0x9e, 0x7e, 0x5a, 0xf5, 0x83, 0x9f, 0x78, 0x2e, 0x04, 0x21, 0xf5, 0xad, 0x5a,
];
const SHIN_MEGAMI_TENSEI_JAPAN_NORMALIZED_DISC_SHA256: [u8; 32] = [
    0x6d, 0x9c, 0x62, 0x34, 0x57, 0x8f, 0x65, 0x3d, 0x4c, 0x81, 0x37, 0x9e, 0x0b, 0xef, 0xfb, 0x4b,
    0x80, 0xbe, 0x18, 0x16, 0xf6, 0x61, 0x42, 0xfd, 0x08, 0x63, 0xa7, 0x79, 0xe6, 0x8f, 0xab, 0x8f,
];
const GAROU_DENSETSU_2_JAPAN_NORMALIZED_DISC_SHA256: [u8; 32] = [
    0xa3, 0x88, 0x7d, 0xa6, 0x25, 0xbb, 0x8d, 0xee, 0x4f, 0xe3, 0x44, 0x76, 0x51, 0x52, 0xab, 0x43,
    0x73, 0xe8, 0xc5, 0x3d, 0x80, 0xda, 0x78, 0x1b, 0x1a, 0xc9, 0x3e, 0x7d, 0x0e, 0x6d, 0xb8, 0xb2,
];
const NEMURENU_YORU_NO_CHIISANA_OHANASHI_JAPAN_DISC_SHA256: [u8; 32] = [
    0x59, 0xd1, 0x2a, 0xb1, 0x66, 0x82, 0x9f, 0xef, 0xf4, 0x2f, 0x60, 0xd0, 0x93, 0x0c, 0xc8, 0xd5,
    0x4f, 0xb4, 0x57, 0x1d, 0x46, 0x6d, 0x4c, 0xf4, 0x4d, 0xd2, 0x05, 0xa9, 0xe5, 0xdc, 0xe7, 0x8f,
];
const HU_PGA_TOUR_POWERGOLF_2_GOLFER_JAPAN_DISC_SHA256: [u8; 32] = [
    0x97, 0xb7, 0x23, 0x2d, 0xf3, 0x3d, 0x40, 0x6a, 0x06, 0x80, 0xd4, 0xf3, 0x17, 0xda, 0x2f, 0x01,
    0x4b, 0x27, 0x02, 0xd6, 0xb5, 0xa4, 0xbe, 0xd9, 0x3e, 0xac, 0x41, 0x5f, 0xb7, 0xfa, 0x4e, 0x67,
];

const PCE_CD_TITLES: &[PceCdTitleMetadata] = &[
    super_cd_profile(
        "pce-cd:jp:1552-tenka-tairan",
        "1552 Tenka Tairan",
        [
            0x79, 0x6c, 0x0e, 0xbb, 0x4b, 0x63, 0x27, 0x03, 0x2e, 0xcd, 0xf1, 0x71, 0xde, 0x57,
            0x8c, 0xc2, 0xb6, 0xa3, 0x1a, 0x7a, 0x04, 0x98, 0xc2, 0x78, 0xd1, 0x60, 0x54, 0xd1,
            0x75, 0xf3, 0xd6, 0x61,
        ],
        PceControllerMode::Mouse,
        false,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:a-iii-a-ressha-de-ikou-iii",
        "A.III - A Ressha de Ikou III",
        [
            0x65, 0xca, 0x62, 0xef, 0x00, 0xa6, 0x46, 0xc1, 0x15, 0x35, 0x4b, 0x7e, 0x96, 0xec,
            0xb9, 0xbc, 0x51, 0xca, 0x13, 0x88, 0x0a, 0x94, 0x07, 0x95, 0x81, 0x78, 0x47, 0x6b,
            0x78, 0xc0, 0x84, 0x26,
        ],
        PceControllerMode::Mouse,
        true,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:atlas-renaissance-voyager",
        "Atlas Renaissance Voyager",
        [
            0xce, 0xd4, 0x3d, 0xd5, 0x74, 0xe8, 0xd3, 0xe0, 0xf6, 0xbc, 0x95, 0x53, 0xdb, 0xb9,
            0x9f, 0xec, 0xbf, 0x3b, 0xb0, 0x81, 0x75, 0x54, 0x5b, 0xfc, 0x53, 0xf7, 0x70, 0x74,
            0xc2, 0x8c, 0x23, 0x27,
        ],
        PceControllerMode::Mouse,
        true,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:brandish",
        "Brandish",
        [
            0x06, 0xa9, 0x8c, 0x5c, 0xaf, 0x94, 0x11, 0xd7, 0x98, 0x10, 0xd2, 0x3e, 0x12, 0x18,
            0x06, 0x4d, 0xea, 0x62, 0x03, 0xf7, 0x9c, 0x02, 0x77, 0x7a, 0x8d, 0x12, 0x3f, 0x12,
            0x80, 0x6d, 0x4b, 0x3d,
        ],
        PceControllerMode::Mouse,
        true,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:dennou-tenshi-digital-angel",
        "Dennou Tenshi Digital Angel",
        [
            0x0a, 0x78, 0xcd, 0xe5, 0x69, 0x7d, 0x1b, 0xb9, 0x17, 0x78, 0x9d, 0xe7, 0x50, 0x54,
            0x95, 0x04, 0x32, 0x65, 0xce, 0xcf, 0xcd, 0x94, 0xbf, 0xf0, 0x9d, 0xc4, 0x4a, 0x31,
            0x34, 0x57, 0xbe, 0xcb,
        ],
        PceControllerMode::Mouse,
        false,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:doukyuusei",
        "Doukyuusei",
        [
            0xd2, 0xa8, 0x3b, 0x80, 0x31, 0x16, 0x6e, 0xd5, 0x28, 0x4b, 0x11, 0x16, 0x2f, 0xd9,
            0xa8, 0xe8, 0x6d, 0x28, 0x52, 0x0f, 0xdd, 0x15, 0xd6, 0xc4, 0x55, 0x8f, 0xb1, 0x49,
            0x8b, 0x2d, 0xcf, 0x90,
        ],
        PceControllerMode::Mouse,
        false,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:eikan-wa-kimi-ni",
        "Eikan wa Kimi ni",
        [
            0x65, 0xda, 0xfe, 0x4f, 0x65, 0x22, 0x38, 0x83, 0xac, 0x58, 0xc5, 0x03, 0x12, 0x9d,
            0xc9, 0x93, 0x7b, 0x17, 0xbb, 0x0a, 0x0b, 0x83, 0x5a, 0xda, 0xbe, 0x04, 0x8a, 0x14,
            0xe7, 0x58, 0x76, 0x28,
        ],
        PceControllerMode::Mouse,
        true,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:hatsukoi-monogatari",
        "Hatsukoi Monogatari",
        [
            0xe1, 0xfd, 0x00, 0x48, 0xd0, 0xd9, 0x66, 0x1c, 0x5b, 0xf6, 0x12, 0xbc, 0xca, 0x86,
            0x68, 0x31, 0x98, 0x31, 0x21, 0x1b, 0x30, 0xbe, 0x36, 0x2f, 0x48, 0x70, 0x0a, 0xdc,
            0x71, 0x4a, 0xba, 0xbd,
        ],
        PceControllerMode::Mouse,
        false,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:jantei-monogatari-iii",
        "Jantei Monogatari III",
        [
            0x91, 0xe6, 0x7f, 0xc2, 0xd4, 0xb0, 0x29, 0xcb, 0x19, 0xb5, 0x68, 0x9d, 0xdc, 0xfb,
            0x32, 0x9f, 0x06, 0x9b, 0x7e, 0xf8, 0x9e, 0xbc, 0xd6, 0x41, 0xa1, 0x01, 0x34, 0xd1,
            0xcd, 0x50, 0x98, 0x4e,
        ],
        PceControllerMode::Mouse,
        false,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:lemmings",
        "Lemmings",
        LEMMINGS_JAPAN_CANONICAL_DISC_SHA256,
        PceControllerMode::Mouse,
        false,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:metal-angel",
        "Metal Angel",
        [
            0xaf, 0xc4, 0x62, 0x15, 0x3b, 0xbb, 0x89, 0xa9, 0x75, 0x96, 0xc7, 0x33, 0x99, 0x3a,
            0x06, 0xfc, 0x3b, 0x65, 0x80, 0xc6, 0x1a, 0xce, 0xa7, 0xa8, 0xdb, 0xc9, 0xb0, 0x96,
            0xc1, 0xbe, 0x8e, 0xf8,
        ],
        PceControllerMode::Mouse,
        false,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:princess-maker-2",
        "Princess Maker 2",
        [
            0x8c, 0x08, 0x9d, 0xbb, 0xb5, 0x0f, 0xcc, 0x36, 0x90, 0x21, 0xde, 0x1d, 0x9c, 0xf1,
            0x70, 0xc8, 0x5b, 0x12, 0xd5, 0xbf, 0x13, 0x4a, 0x18, 0xb0, 0x57, 0x3f, 0xef, 0x27,
            0x90, 0x5b, 0x22, 0x79,
        ],
        PceControllerMode::Mouse,
        true,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:tokimeki-memorial",
        "Tokimeki Memorial",
        [
            0x25, 0x42, 0x9e, 0xba, 0xe7, 0xb0, 0xfb, 0xe6, 0x08, 0xe5, 0x8b, 0xb4, 0xf1, 0x43,
            0xe3, 0xa6, 0xf8, 0xd1, 0x9b, 0x87, 0xba, 0xf9, 0xb9, 0xb5, 0xd8, 0x05, 0x4f, 0x2c,
            0x9d, 0xf7, 0x7b, 0xc5,
        ],
        PceControllerMode::Mouse,
        false,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:vasteel-2",
        "Vasteel 2",
        [
            0x04, 0xcf, 0xa1, 0x75, 0x6f, 0x14, 0x71, 0xec, 0x9a, 0xb5, 0x6e, 0xe3, 0xfb, 0x54,
            0x68, 0xf5, 0xd9, 0xfd, 0x35, 0x61, 0x7f, 0xfc, 0x3c, 0x7e, 0x95, 0x41, 0x1c, 0x9a,
            0xca, 0x26, 0x9e, 0x6e,
        ],
        PceControllerMode::Mouse,
        true,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:nemurenu-yoru-no-chiisana-ohanashi",
        "Nemurenu Yoru no Chiisana Ohanashi",
        NEMURENU_YORU_NO_CHIISANA_OHANASHI_JAPAN_DISC_SHA256,
        PceControllerMode::Mouse,
        false,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:hu-pga-tour-powergolf-2-golfer",
        "Hu PGA Tour - PowerGolf 2 - Golfer",
        HU_PGA_TOUR_POWERGOLF_2_GOLFER_JAPAN_DISC_SHA256,
        PceControllerMode::Mouse,
        false,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:tengai-makyou-deden-no-kabuki-den",
        "Tengai Makyou - Deden no Kabuki-den",
        TENGAI_MAKYOU_DEDEN_NO_KABUKI_DEN_CANONICAL_DISC_SHA256,
        PceControllerMode::Multitap,
        false,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:shin-megami-tensei",
        "Shin Megami Tensei",
        SHIN_MEGAMI_TENSEI_JAPAN_NORMALIZED_DISC_SHA256,
        PceControllerMode::TwoButton,
        true,
        false,
    ),
    super_cd_profile(
        "pce-cd:jp:garou-densetsu-2",
        "Garou Densetsu 2 - Aratanaru Tatakai",
        GAROU_DENSETSU_2_JAPAN_NORMALIZED_DISC_SHA256,
        PceControllerMode::TwoButton,
        false,
        true,
    ),
];

pub(crate) fn canonical_title_metadata(
    normalized_disc_sha256: [u8; 32],
) -> Option<&'static PceCdTitleMetadata> {
    PCE_CD_TITLES
        .iter()
        .find(|title| title.normalized_disc_sha256 == normalized_disc_sha256)
}

pub(crate) fn automatic_controller_mode(content_hash: [u8; 32]) -> PceControllerMode {
    #[cfg(test)]
    if let Some((_, mode)) = TEST_CONTROLLER_CATALOG
        .lock()
        .expect("test controller catalogue lock poisoned")
        .iter()
        .find(|(hash, _)| *hash == content_hash)
    {
        return *mode;
    }
    canonical_title_metadata(content_hash)
        .map(|profile| profile.controller_mode)
        .unwrap_or(PceControllerMode::TwoButton)
}

#[cfg(test)]
static TEST_CONTROLLER_CATALOG: std::sync::Mutex<Vec<([u8; 32], PceControllerMode)>> =
    std::sync::Mutex::new(Vec::new());

#[cfg(test)]
pub(crate) struct TestControllerCatalogGuard([u8; 32]);

#[cfg(test)]
pub(crate) fn register_test_controller_catalog_hash(
    normalized_disc_sha256: [u8; 32],
    mode: PceControllerMode,
) -> TestControllerCatalogGuard {
    TEST_CONTROLLER_CATALOG
        .lock()
        .expect("test controller catalogue lock poisoned")
        .push((normalized_disc_sha256, mode));
    TestControllerCatalogGuard(normalized_disc_sha256)
}

#[cfg(test)]
impl Drop for TestControllerCatalogGuard {
    fn drop(&mut self) {
        let mut catalog = TEST_CONTROLLER_CATALOG
            .lock()
            .expect("test controller catalogue lock poisoned");
        let index = catalog
            .iter()
            .position(|(hash, _)| *hash == self.0)
            .expect("test controller catalogue entry was removed");
        catalog.swap_remove(index);
    }
}

pub(crate) fn automatic_memory_base_enabled(content_hash: Option<[u8; 32]>) -> bool {
    let catalogued = content_hash
        .and_then(canonical_title_metadata)
        .is_some_and(|profile| profile.memory_base_128);
    #[cfg(test)]
    let catalogued = catalogued
        || content_hash.is_some_and(|hash| {
            TEST_MEMORY_BASE_CATALOG
                .lock()
                .expect("test Memory Base catalogue lock poisoned")
                .contains(&hash)
        });
    catalogued
}

#[cfg(test)]
static TEST_MEMORY_BASE_CATALOG: std::sync::Mutex<Vec<[u8; 32]>> =
    std::sync::Mutex::new(Vec::new());

#[cfg(test)]
pub(crate) struct TestMemoryBaseCatalogGuard([u8; 32]);

#[cfg(test)]
pub(crate) fn register_test_memory_base_catalog_hash(
    normalized_disc_sha256: [u8; 32],
) -> TestMemoryBaseCatalogGuard {
    TEST_MEMORY_BASE_CATALOG
        .lock()
        .expect("test Memory Base catalogue lock poisoned")
        .push(normalized_disc_sha256);
    TestMemoryBaseCatalogGuard(normalized_disc_sha256)
}

#[cfg(test)]
impl Drop for TestMemoryBaseCatalogGuard {
    fn drop(&mut self) {
        let mut catalog = TEST_MEMORY_BASE_CATALOG
            .lock()
            .expect("test Memory Base catalogue lock poisoned");
        let index = catalog
            .iter()
            .position(|hash| *hash == self.0)
            .expect("test Memory Base catalogue entry was removed");
        catalog.swap_remove(index);
    }
}

pub(crate) fn automatic_arcade_card_enabled(content_hash: Option<[u8; 32]>) -> bool {
    let catalogued = content_hash
        .and_then(canonical_title_metadata)
        .is_some_and(|profile| profile.arcade_card);
    #[cfg(test)]
    let catalogued = catalogued
        || content_hash.is_some_and(|hash| {
            TEST_ARCADE_CARD_CATALOG
                .lock()
                .expect("test Arcade Card catalogue lock poisoned")
                .contains(&hash)
        });
    catalogued
}

#[cfg(test)]
static TEST_ARCADE_CARD_CATALOG: std::sync::Mutex<Vec<[u8; 32]>> =
    std::sync::Mutex::new(Vec::new());

#[cfg(test)]
pub(crate) struct TestArcadeCardCatalogGuard([u8; 32]);

#[cfg(test)]
pub(crate) fn register_test_arcade_card_catalog_hash(
    normalized_disc_sha256: [u8; 32],
) -> TestArcadeCardCatalogGuard {
    TEST_ARCADE_CARD_CATALOG
        .lock()
        .expect("test Arcade Card catalogue lock poisoned")
        .push(normalized_disc_sha256);
    TestArcadeCardCatalogGuard(normalized_disc_sha256)
}

#[cfg(test)]
impl Drop for TestArcadeCardCatalogGuard {
    fn drop(&mut self) {
        let mut catalog = TEST_ARCADE_CARD_CATALOG
            .lock()
            .expect("test Arcade Card catalogue lock poisoned");
        let index = catalog
            .iter()
            .position(|hash| *hash == self.0)
            .expect("test Arcade Card catalogue entry was removed");
        catalog.swap_remove(index);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn canonical_titles_are_unique_and_keep_validated_capability_counts() {
        assert_eq!(PCE_CD_TITLES.len(), 19);
        assert_eq!(
            PCE_CD_TITLES
                .iter()
                .map(|title| title.normalized_disc_sha256)
                .collect::<HashSet<_>>()
                .len(),
            PCE_CD_TITLES.len()
        );
        assert_eq!(
            PCE_CD_TITLES
                .iter()
                .map(|title| title.id)
                .collect::<HashSet<_>>()
                .len(),
            PCE_CD_TITLES.len()
        );
        assert!(PCE_CD_TITLES.iter().all(|title| {
            !title.id.is_empty() && !title.title.is_empty() && title.region == "JP"
        }));
        assert_eq!(
            PCE_CD_TITLES
                .iter()
                .filter(|profile| profile.controller_mode == PceControllerMode::Mouse)
                .count(),
            16
        );
        assert_eq!(
            PCE_CD_TITLES
                .iter()
                .filter(|profile| profile.controller_mode == PceControllerMode::Multitap)
                .count(),
            1
        );
        assert_eq!(
            PCE_CD_TITLES
                .iter()
                .filter(|profile| profile.memory_base_128)
                .count(),
            7
        );
        assert_eq!(
            PCE_CD_TITLES
                .iter()
                .filter(|profile| profile.arcade_card)
                .count(),
            1
        );
        assert!(
            PCE_CD_TITLES.iter().all(|profile| {
                profile.minimum_system_card == Some(PceSystemCardTier::Version3)
            })
        );
    }

    #[test]
    fn canonical_titles_require_an_exact_hash() {
        let lemmings = canonical_title_metadata(LEMMINGS_JAPAN_CANONICAL_DISC_SHA256).unwrap();
        assert_eq!(lemmings.id, "pce-cd:jp:lemmings");
        assert_eq!(lemmings.title, "Lemmings");
        assert_eq!(lemmings.region, "JP");
        assert_eq!(
            lemmings.minimum_system_card,
            Some(PceSystemCardTier::Version3)
        );
        assert_eq!(
            automatic_controller_mode(LEMMINGS_JAPAN_CANONICAL_DISC_SHA256),
            PceControllerMode::Mouse
        );
        assert_eq!(
            automatic_controller_mode(TENGAI_MAKYOU_DEDEN_NO_KABUKI_DEN_CANONICAL_DISC_SHA256),
            PceControllerMode::Multitap
        );
        assert_eq!(
            automatic_controller_mode(NEMURENU_YORU_NO_CHIISANA_OHANASHI_JAPAN_DISC_SHA256),
            PceControllerMode::Mouse
        );
        assert_eq!(
            automatic_controller_mode(HU_PGA_TOUR_POWERGOLF_2_GOLFER_JAPAN_DISC_SHA256),
            PceControllerMode::Mouse
        );
        assert!(automatic_memory_base_enabled(Some(
            SHIN_MEGAMI_TENSEI_JAPAN_NORMALIZED_DISC_SHA256
        )));
        assert!(automatic_arcade_card_enabled(Some(
            GAROU_DENSETSU_2_JAPAN_NORMALIZED_DISC_SHA256
        )));

        let mut near_miss = LEMMINGS_JAPAN_CANONICAL_DISC_SHA256;
        near_miss[31] ^= 1;
        assert_eq!(canonical_title_metadata(near_miss), None);
        assert_eq!(
            automatic_controller_mode(near_miss),
            PceControllerMode::TwoButton
        );
        assert!(!automatic_memory_base_enabled(Some(near_miss)));
        assert!(!automatic_arcade_card_enabled(Some(near_miss)));
        assert!(!automatic_memory_base_enabled(None));
        assert!(!automatic_arcade_card_enabled(None));
    }
}
