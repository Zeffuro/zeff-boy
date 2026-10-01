use super::{FixedProfile, InitialWord, Profile, WsToseHardware};

#[path = "legacy_profiles_paragraph.rs"]
mod paragraph;
#[path = "legacy_profiles_1.rs"]
mod part_1;
#[path = "legacy_profiles_2.rs"]
mod part_2;
#[path = "legacy_profiles_3.rs"]
mod part_3;
#[path = "legacy_profiles_4.rs"]
mod part_4;

pub(super) const PROFILES: &[&[FixedProfile]] = &[
    part_1::PROFILES,
    part_2::PROFILES,
    part_3::PROFILES,
    part_4::PROFILES,
    paragraph::PROFILES,
];

pub(super) fn is_paragraph(name: &str) -> bool {
    paragraph::PROFILES
        .iter()
        .any(|profile| profile.driver.name == name)
}
