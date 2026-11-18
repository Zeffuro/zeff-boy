use crate::constants::SOUND_RAM_SIZE;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

#[cfg_attr(not(feature = "sound-shadow-observer"), derive(Debug, PartialEq, Eq))]
pub(crate) struct SoundRam(
    Arc<[u8; SOUND_RAM_SIZE]>,
    #[cfg(feature = "sound-shadow-observer")] pub(super) super::observer::Token,
);

#[cfg(feature = "sound-shadow-observer")]
mod observer_traits;

impl SoundRam {
    pub(crate) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut crate::emulator::rollback::MdRetainedAllocations<'a>,
    ) -> anyhow::Result<()> {
        #[cfg(not(feature = "sound-shadow-observer"))]
        let Self(ram) = self;
        #[cfg(feature = "sound-shadow-observer")]
        let Self(ram, _) = self;
        into.arc(ram, crate::emulator::rollback::MdAllocationKind::SoundRam)?;
        Ok(())
    }

    pub fn snapshot(&self) -> Self {
        Self(
            Arc::clone(&self.0),
            #[cfg(feature = "sound-shadow-observer")]
            super::observer::snapshot(&self.1),
        )
    }
}

impl Default for SoundRam {
    fn default() -> Self {
        Self::from([0; SOUND_RAM_SIZE])
    }
}

impl From<[u8; SOUND_RAM_SIZE]> for SoundRam {
    fn from(bytes: [u8; SOUND_RAM_SIZE]) -> Self {
        let allocation = Arc::new(bytes);
        #[cfg(feature = "sound-shadow-observer")]
        let token = super::observer::allocation(Arc::as_ptr(&allocation) as usize);
        Self(
            allocation,
            #[cfg(feature = "sound-shadow-observer")]
            token,
        )
    }
}

// Ordinary clones remain independent; speculative sharing is explicit.
impl Clone for SoundRam {
    fn clone(&self) -> Self {
        #[cfg(feature = "sound-shadow-observer")]
        let _clone = super::observer::deep_clone(&self.1);
        Self::from(*self.0)
    }
}

impl Deref for SoundRam {
    type Target = [u8; SOUND_RAM_SIZE];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SoundRam {
    fn deref_mut(&mut self) -> &mut Self::Target {
        #[cfg(feature = "sound-shadow-observer")]
        let before = super::observer::before_mut(
            &self.1,
            Arc::as_ptr(&self.0) as usize,
            Arc::strong_count(&self.0),
            Arc::weak_count(&self.0),
        );
        let bytes = Arc::make_mut(&mut self.0);
        #[cfg(feature = "sound-shadow-observer")]
        super::observer::after_mut(&self.1, before, bytes.as_ptr() as usize);
        bytes
    }
}
