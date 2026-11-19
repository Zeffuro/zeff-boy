use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Allocator;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[derive(Default)]
pub struct Windows {
    pub count: usize,
}
impl Windows {
    pub fn measure<T>(&mut self, call: impl FnOnce() -> T) -> T {
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let result = black_box(call());
        let after = ALLOCATIONS.load(Ordering::Relaxed);
        self.count += 1;
        assert_eq!(after, before, "allocation window {}", self.count);
        result
    }
}

pub fn positive_control() {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let control = black_box(Vec::<u8>::with_capacity(black_box(31)));
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert!(after > before);
    black_box(control);
}
