//! The limiter runs on the audio thread: from the first block after new, processing, switching True Peak and
//! parameter changes must not allocate.

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
fn limiter_never_allocates_while_processing() {
    let info = ondina_core::plugins::find("limiter").expect("limiter in the registry");
    for true_peak in [0.0, 1.0] {
        let mut fx = create_effect("limiter", 48000.0).expect("built-in effect");
        let mut params = std::collections::BTreeMap::new();
        params.insert("gain".to_string(), 12.0);
        params.insert("ceiling".to_string(), -1.0);
        params.insert("true_peak".to_string(), true_peak);
        let input: Vec<f32> = (0..512).map(|i| (i as f32 * 0.05).sin() * 0.8 + (i as f32 * 1.3).sin() * 0.3).collect();
        let (mut l, mut r) = (input.clone(), input.clone());
        let ctx = ProcessContext::idle(48000.0);
        let n = allocations(|| {
            configure("limiter", &params, |k, v| fx.set_param(k, v));
            for block in 0..200 {
                l.copy_from_slice(&input);
                r.copy_from_slice(&input);
                // the engine sets automated parameters every block: True Peak goes Off and On along the way
                for p in info.params {
                    let t = (block % 7) as f32 / 6.0;
                    fx.set_param(p.id, p.min + (p.max - p.min) * t);
                }
                fx.process(&mut l, &mut r, &ctx);
            }
            fx.reset();
        });
        assert_eq!(n, 0, "the limiter allocated {n} times (true_peak {true_peak})");
    }
}
