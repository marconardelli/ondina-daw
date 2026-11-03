//! The plug-in meters run on the audio thread: writing and reading them must not allocate, and measuring the
//! inserts must not add allocations to the render.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use ondina_core::command::Command;
use ondina_core::model::{Id, Note, Project, TrackKind, MASTER_ID};
use ondina_core::ops::{self, Outcome};
use ondina_engine::{AudioPool, MeterCells, PluginMeters, Renderer};

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

fn run(p: &mut Project, c: Command) -> serde_json::Value {
    match ops::apply(p, &c).expect("command applies") {
        Outcome::Done(v) => v,
        _ => panic!("unexpected outcome"),
    }
}

fn effect(p: &mut Project, track_id: Id, kind: &str) -> Id {
    run(p, Command::AddEffect { track_id, kind: kind.into(), preset: None, index: None })["plugin_id"].as_u64().unwrap()
}

/// A synth holding a loud chord through every dynamics plug-in and a Channel EQ, with a limiter on the master.
fn project() -> Project {
    let mut p = Project::new("t");
    let t = run(&mut p, Command::AddTrack { kind: TrackKind::Instrument, name: None, patch: None, instrument: Some("analog_synth".into()), preset: None, index: None })["track_id"].as_u64().unwrap();
    let notes = [48u8, 55, 60, 64].iter().map(|&pitch| Note { pitch, velocity: 127, start: 0.0, length: 64.0 }).collect();
    run(&mut p, Command::CreateMidiRegion { track_id: t, start: 0.0, length: 64.0, name: None, notes: Some(notes) });
    let comp = effect(&mut p, t, "compressor");
    let params: BTreeMap<String, f32> = [("threshold", -40.0), ("ratio", 8.0)].iter().map(|(k, v)| (k.to_string(), *v)).collect();
    run(&mut p, Command::SetPluginParams { plugin_id: comp, params });
    for kind in ["noise_gate", "de_esser", "dynamic_eq", "limiter", "channel_eq"] {
        effect(&mut p, t, kind);
    }
    effect(&mut p, MASTER_ID, "limiter");
    p
}

#[test]
fn meter_writes_and_reads_never_allocate() {
    let m = PluginMeters::default();
    let ids: HashSet<Id> = (1..=16).collect();
    m.assign(&ids);
    let cells: Vec<usize> = (1..=16).map(|id| m.cell_of(id).unwrap()).collect();
    let n = allocations(|| {
        for i in 0..10_000usize {
            let id = (i % 16) as Id + 1;
            m.write(cells[i % 16], id, 0.5, 0.25, if i % 2 == 0 { Some(-3.0) } else { None });
            assert!(m.read(id).is_some());
        }
    });
    assert_eq!(n, 0, "the meter table allocated {n} times");
}

#[test]
fn meters_add_no_allocations_to_the_render() {
    let p = project();
    let meters = Arc::new(PluginMeters::default());
    let cells = meters.assign_project(&p);
    let inserts: Vec<Id> = p.tracks.iter().flat_map(|t| t.channel.inserts.iter()).chain(p.master.inserts.iter()).map(|i| i.id).collect();
    assert_eq!(inserts.len(), 7);
    let p = Arc::new(p);
    let mut with = Renderer::new(48000.0, p.clone(), Arc::new(AudioPool::new()));
    with.set_plugin_meters(Some(meters.clone()), cells);
    let mut without = Renderer::new(48000.0, p, Arc::new(AudioPool::new()));
    with.play();
    without.play();
    let (mut l, mut r) = (vec![0.0f32; 480], vec![0.0f32; 480]);
    // warm up out of the count (the limiter allocates in its first blocks)
    for _ in 0..100 {
        with.render(&mut l, &mut r);
        without.render(&mut l, &mut r);
    }
    let with_meters = allocations(|| {
        for _ in 0..200 {
            with.render(&mut l, &mut r);
        }
    });
    let without_meters = allocations(|| {
        for _ in 0..200 {
            without.render(&mut l, &mut r);
        }
    });
    assert_eq!(with_meters, without_meters, "the meters added allocations to the render");
    let measured = inserts.iter().filter(|id| meters.read(**id).is_some()).count();
    assert_eq!(measured, inserts.len(), "every insert is measured");
}

#[test]
fn meter_cell_lookups_never_allocate() {
    let cells = MeterCells::new((1..=16).map(|id: Id| (id * 3, (16 - id) as usize)));
    let n = allocations(|| {
        for i in 0..10_000u64 {
            let id = i % 50;
            let cell = cells.cell(id);
            assert_eq!(cell.is_some(), id % 3 == 0 && (3..=48).contains(&id));
        }
    });
    assert_eq!(n, 0, "the cell lookups allocated {n} times");
}
