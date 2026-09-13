use super::recipe::DriverVariant;

mod cues;

mod disney;
mod gb;
mod gb2;
mod gb3;

pub(super) fn all() -> impl Iterator<Item = &'static DriverVariant> {
    let profiles = [&gb::VARIANT, &gb2::VARIANT, &gb3::VARIANT, &disney::VARIANT].into_iter();
    #[cfg(any(test, feature = "test-support"))]
    let profiles = profiles.chain([&super::fixture::VARIANT, &super::fixture::RAM_VARIANT]);
    profiles
}
