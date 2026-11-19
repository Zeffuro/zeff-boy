use super::*;

impl std::fmt::Debug for SoundRam {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("SoundRam").field(&self.0).finish()
    }
}

impl PartialEq for SoundRam {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for SoundRam {}

#[cfg(test)]
impl SoundRam {
    pub(crate) fn enroll_observer(&mut self) {
        super::super::observer::enroll(&mut self.1, Arc::as_ptr(&self.0) as usize);
    }

    pub(crate) fn observer_weak(&self) -> std::sync::Weak<[u8; SOUND_RAM_SIZE]> {
        Arc::downgrade(&self.0)
    }
}
