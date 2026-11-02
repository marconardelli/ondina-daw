//! The dynamic EQ runs on the audio thread: processing and parameter changes must not allocate.

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
fn dynamic_eq_never_allocates_while_processing() {
    let info = ondina_core::plugins::find("dynamic_eq").expect("dynamic_eq in the registry");
    let mut fx = create_effect("dynamic_eq", 48000.0).expect("built-in effect");
    let mut params = std::collections::BTreeMap::new();
    for n in 1..=4 {
        params.insert(format!("b{n}_threshold"), -40.0);
        params.insert(format!("b{n}_range"), if n % 2 == 0 { 6.0 } else { -12.0 });
    }
    configure("dynamic_eq", &params, |k, v| fx.set_param(k, v));
    let input: Vec<f32> = (0..512).map(|i| (i as f32 * 0.05).sin() * 0.8).collect();
    let (mut l, mut r) = (input.clone(), input.clone());
    let ctx = ProcessContext::idle(48000.0);
    let n = allocations(|| {
        for block in 0..200 {
            l.copy_from_slice(&input);
            r.copy_from_slice(&input);
            // the engine sets automated parameters every block, with new and with unchanged values
            for p in info.params {
                let t = (block % 7) as f32 / 6.0;
                fx.set_param(p.id, p.min + (p.max - p.min) * t);
            }
            fx.process(&mut l, &mut r, &ctx);
        }
        fx.reset();
    });
    assert_eq!(n, 0, "the dynamic EQ allocated {n} times");
}
