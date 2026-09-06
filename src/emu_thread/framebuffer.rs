use std::cell::RefCell;
use std::ops::Deref;
use std::sync::Arc;

use arc_swap::ArcSwapOption;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PublishedFramebuffer {
    pixels: Vec<u8>,
    dimensions: Option<(u32, u32)>,
}

impl PublishedFramebuffer {
    pub(crate) fn dimensions(&self) -> Option<(u32, u32)> {
        self.dimensions
    }
}

impl From<Vec<u8>> for PublishedFramebuffer {
    fn from(pixels: Vec<u8>) -> Self {
        Self {
            pixels,
            dimensions: None,
        }
    }
}

impl Deref for PublishedFramebuffer {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.pixels
    }
}

pub(crate) type SharedFramebuffer = Arc<ArcSwapOption<PublishedFramebuffer>>;

const MAX_CACHED_FRAMEBUFFER_BYTES: usize = 640 * 480 * 4;

thread_local! {
    static FRAMEBUFFER_SPARE: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

pub(crate) fn new_shared_framebuffer() -> SharedFramebuffer {
    Arc::new(ArcSwapOption::empty())
}

#[cfg(any(test, feature = "profile-cores"))]
pub(crate) fn publish_framebuffer(shared: &SharedFramebuffer, pixels: &[u8]) {
    publish_framebuffer_with_dimensions(shared, pixels, None);
}

pub(crate) fn publish_backend_framebuffer(
    shared: &SharedFramebuffer,
    backend: &crate::emu_backend::EmuBackend,
) {
    let (pixels, dimensions) = backend.display_framebuffer();
    publish_framebuffer_with_dimensions(shared, pixels, dimensions);
}

pub(crate) fn publish_framebuffer_with_dimensions(
    shared: &SharedFramebuffer,
    pixels: &[u8],
    dimensions: Option<(u32, u32)>,
) {
    let mut owned = FRAMEBUFFER_SPARE
        .with(|spare| spare.borrow_mut().take())
        .unwrap_or_default();
    owned.clear();
    owned.extend_from_slice(pixels);
    publish_owned(
        shared,
        PublishedFramebuffer {
            pixels: owned,
            dimensions,
        },
    );
}

pub(crate) fn publish_owned_framebuffer(shared: &SharedFramebuffer, pixels: Vec<u8>) {
    publish_owned(shared, pixels.into());
}

fn publish_owned(shared: &SharedFramebuffer, framebuffer: PublishedFramebuffer) {
    let previous = shared.swap(Some(Arc::new(framebuffer)));
    let Some(previous) = previous else {
        return;
    };
    let Ok(previous) = Arc::try_unwrap(previous) else {
        return;
    };
    let mut pixels = previous.pixels;
    if pixels.capacity() > MAX_CACHED_FRAMEBUFFER_BYTES {
        return;
    }
    pixels.clear();
    FRAMEBUFFER_SPARE.with(|spare| *spare.borrow_mut() = Some(pixels));
}

#[cfg(feature = "profile-cores")]
pub(crate) fn profile_frame_publication(framebuffer: &[u8], iterations: u32) {
    use std::hint::black_box;
    use std::time::Instant;

    let shared = new_shared_framebuffer();
    for _ in 0..10 {
        publish_framebuffer(&shared, framebuffer);
    }
    let start = Instant::now();
    for _ in 0..iterations {
        publish_framebuffer(&shared, black_box(framebuffer));
    }
    let elapsed = start.elapsed();
    let frames_per_second = f64::from(iterations) / elapsed.as_secs_f64();
    black_box(shared.load_full());
    println!(
        "frame publication                {iterations:5} frames  {elapsed:>9.2?}  {frames_per_second:>8.1} fps"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_keeps_pixels_and_dimensions_together() {
        let shared = new_shared_framebuffer();
        publish_framebuffer_with_dimensions(&shared, &[1; 8], Some((2, 1)));
        let old = shared.load_full().unwrap();
        publish_framebuffer_with_dimensions(&shared, &[2; 16], Some((1, 4)));
        let new = shared.load_full().unwrap();
        assert_eq!(old.as_slice(), &[1; 8]);
        assert_eq!(old.dimensions(), Some((2, 1)));
        assert_eq!(new.as_slice(), &[2; 16]);
        assert_eq!(new.dimensions(), Some((1, 4)));
        publish_framebuffer(&shared, &[3; 4]);
        assert_eq!(shared.load_full().unwrap().dimensions(), None);
    }
}
