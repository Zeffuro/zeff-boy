use super::Profile;

mod disney;
mod gb;
mod gb2;
mod gb3;

pub(super) fn all() -> impl Iterator<Item = &'static Profile> {
    let profiles = [&gb::PROFILE, &gb2::PROFILE, &gb3::PROFILE, &disney::PROFILE].into_iter();
    #[cfg(any(test, feature = "test-support"))]
    let profiles = profiles.chain([&super::fixture::PROFILE, &super::fixture::RAM_PROFILE]);
    profiles
}
