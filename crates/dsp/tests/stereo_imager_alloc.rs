//! The stereo imager runs on the audio thread: processing and parameter changes must not allocate.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use ondina_dsp::{configure, create_effect, ProcessContext};

struct CountingAlloc;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.try_with(Cell::get).unwrap_or(false) {
            COUNT.with(|c| c.set(c.get() + 1));
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

/// Allocations made by `f` on this thread.
fn allocations(f: impl FnOnce()) -> usize {
    let before = COUNT.with(Cell::get);
    COUNTING.with(|c| c.set(true));
    f();
    COUNTING.with(|c| c.set(false));
    COUNT.with(Cell::get) - before
}

#[test]
fn stereo_imager_never_allocates_while_processing() {
    let info = ondina_core::plugins::find("stereo_imager").expect("stereo_imager in the registry");
    let mut fx = create_effect("stereo_imager", 48000.0).expect("built-in effect");
    let mut params = std::collections::BTreeMap::new();
    params.insert("mono_bass".to_string(), 1.0);
    params.insert("high_width".to_string(), 1.8);
    configure("stereo_imager", &params, |k, v| fx.set_param(k, v));
    let left: Vec<f32> = (0..512).map(|i| (i as f32 * 0.05).sin() * 0.8).collect();
    let right: Vec<f32> = (0..512).map(|i| (i as f32 * 0.031).sin() * 0.6).collect();
    let (mut l, mut r) = (left.clone(), right.clone());
    let ctx = ProcessContext::idle(48000.0);
    let n = allocations(|| {
        for block in 0..200 {
            l.copy_from_slice(&left);
            r.copy_from_slice(&right);
            // the engine sets automated parameters every block, with new and with unchanged values
            for p in info.params {
                let t = (block % 7) as f32 / 6.0;
                fx.set_param(p.id, p.min + (p.max - p.min) * t);
            }
            fx.process(&mut l, &mut r, &ctx);
        }
        fx.reset();
    });
    assert_eq!(n, 0, "the stereo imager allocated {n} times");
}
