use crate::constants::SOUND_RAM_SIZE;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SoundRam(Arc<[u8; SOUND_RAM_SIZE]>);

impl SoundRam {
    pub fn snapshot(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl Default for SoundRam {
    fn default() -> Self {
        Self::from([0; SOUND_RAM_SIZE])
    }
}

impl From<[u8; SOUND_RAM_SIZE]> for SoundRam {
    fn from(bytes: [u8; SOUND_RAM_SIZE]) -> Self {
        Self(Arc::new(bytes))
    }
}

// Ordinary clones remain independent; speculative sharing is explicit.
impl Clone for SoundRam {
    fn clone(&self) -> Self {
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
        Arc::make_mut(&mut self.0)
    }
}
