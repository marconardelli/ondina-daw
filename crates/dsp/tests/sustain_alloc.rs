//! The sustain pedal and note-off run on the audio thread: they must not allocate.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use ondina_dsp::{create_instrument, AudioData, ProcessContext};

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
fn pedal_and_note_off_never_allocate() {
    for kind in ["analog_synth", "fm_synth", "organ", "pluck_synth", "sampler"] {
        let mut inst = create_instrument(kind, 48000.0).expect("built-in instrument");
        if kind == "sampler" {
            let sine = (0..4800).map(|i| (i as f32 * 0.05).sin()).collect();
            inst.set_sample(Some(Arc::new(AudioData { sample_rate: 48000, channels: vec![sine] })));
        }
        for p in [60, 64, 67] {
            inst.note_on(p, 100);
        }
        let (mut l, mut r) = (vec![0.0f32; 480], vec![0.0f32; 480]);
        inst.render(&mut l, &mut r, &ProcessContext::idle(48000.0));
        let n = allocations(|| {
            inst.control_change(64, 127);
            for p in [60, 64, 67] {
                inst.note_off(p);
            }
            for _ in 0..200 {
                inst.note_off(60);
            }
            inst.control_change(64, 0);
            inst.control_change(64, 127);
            inst.note_off(72);
            inst.control_change(1, 50);
            inst.control_change(64, 0);
            inst.control_change(64, 0);
            inst.all_notes_off();
            inst.reset();
        });
        assert_eq!(n, 0, "{kind} allocated {n} times in the pedal path");
    }
}
