//! Live meters of the insert plug-ins: written by the audio thread, read by anyone without locks.
//!
//! The control thread (whoever holds `&mut Engine`) gives each insert a cell with [`PluginMeters::assign`]; the
//! audio thread is the only writer of the values ([`PluginMeters::write`]) and stamps them with the plug-in id they
//! belong to, behind a sequence counter. A reader ([`PluginMeters::read`]) never waits: it retries a few times if a
//! write is in progress and only returns values stamped with the id it asked for, so a cell that changed hands
//! never shows one plug-in's meters as another's.
//!
//! [`PluginMeters::assign_project`] also returns a [`MeterCells`] map, which travels to the renderer with the project
//! snapshot: the audio thread finds an insert's cell there and never scans the table.

use std::collections::HashSet;
use std::sync::atomic::{fence, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use ondina_core::model::{Id, Project};

pub const PLUGIN_METER_CAPACITY: usize = 512;
/// Each write keeps this share of the previous peak (one write per audio callback), like the track meters.
const RELEASE: f32 = 0.97;
/// How many times a reader retries while the audio thread is writing the same cell.
const READ_ATTEMPTS: usize = 4;

/// What an insert did in the last audio blocks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PluginMeter {
    /// Peak entering the plug-in (linear, 1.0 = 0 dBFS), with a short release.
    pub input_peak: f32,
    /// Peak leaving the plug-in (linear), with a short release.
    pub output_peak: f32,
    /// Deepest gain reduction in dB (<= 0); None for plug-ins that do not reduce, and while bypassed.
    pub gain_reduction_db: Option<f32>,
}

/// Which meter cell each insert writes to, built by the control thread when it assigns the cells and handed to the
/// renderer with the project, so the audio thread looks a cell up without scanning the table.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MeterCells {
    /// Sorted by id, one entry per id.
    by_id: Box<[(Id, u32)]>,
}

impl MeterCells {
    /// The map of `pairs` (plug-in id, cell): sorted, without id 0 and keeping the first cell of a repeated id.
    pub fn new(pairs: impl IntoIterator<Item = (Id, usize)>) -> MeterCells {
        let mut v: Vec<(Id, u32)> = pairs.into_iter().filter(|(id, _)| *id != 0).map(|(id, c)| (id, c as u32)).collect();
        v.sort_by_key(|(id, _)| *id);
        v.dedup_by_key(|(id, _)| *id);
        MeterCells { by_id: v.into_boxed_slice() }
    }

    /// The cell of plug-in `id`: a binary search, with no allocation and no atomic load (audio thread safe).
    pub fn cell(&self, id: Id) -> Option<usize> {
        self.by_id.binary_search_by_key(&id, |(i, _)| *i).ok().map(|k| self.by_id[k].1 as usize)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

struct MeterCell {
    /// Plug-in id the control thread gave this cell to; 0 = free (plug-in ids start at 1).
    owner: AtomicU64,
    /// Sequence counter: odd while the audio thread writes the values below.
    seq: AtomicU32,
    /// Plug-in id the values belong to, stamped by the writer; 0 = never written.
    id: AtomicU64,
    /// f32 bits.
    input: AtomicU32,
    /// f32 bits.
    output: AtomicU32,
    /// f32 bits; NaN = no reduction to report.
    reduction: AtomicU32,
}

impl MeterCell {
    fn new() -> MeterCell {
        MeterCell {
            owner: AtomicU64::new(0),
            seq: AtomicU32::new(0),
            id: AtomicU64::new(0),
            input: AtomicU32::new(0.0f32.to_bits()),
            output: AtomicU32::new(0.0f32.to_bits()),
            reduction: AtomicU32::new(f32::NAN.to_bits()),
        }
    }
}

/// A fixed table of meter cells, allocated once.
pub struct PluginMeters {
    cells: Box<[MeterCell]>,
}

impl Default for PluginMeters {
    fn default() -> Self {
        PluginMeters::new(PLUGIN_METER_CAPACITY)
    }
}

impl PluginMeters {
    pub fn new(capacity: usize) -> PluginMeters {
        PluginMeters { cells: (0..capacity).map(|_| MeterCell::new()).collect() }
    }

    /// How many plug-ins can have a meter at the same time.
    pub fn capacity(&self) -> usize {
        self.cells.len()
    }

    /// Give a cell to every id of `ids` and free the cells of the plug-ins that are gone. Control thread only (the
    /// one that holds `&mut Engine`): it never touches the values, so the audio thread stays their only writer.
    /// Ids beyond the capacity get no cell.
    pub fn assign(&self, ids: &HashSet<Id>) {
        for c in self.cells.iter() {
            let owner = c.owner.load(Ordering::Acquire);
            if owner != 0 && !ids.contains(&owner) {
                c.owner.store(0, Ordering::Release);
            }
        }
        let mut sorted: Vec<Id> = ids.iter().copied().filter(|id| *id != 0).collect();
        sorted.sort_unstable();
        for id in sorted {
            if self.cell_of(id).is_some() {
                continue;
            }
            match self.cells.iter().find(|c| c.owner.load(Ordering::Acquire) == 0) {
                Some(c) => c.owner.store(id, Ordering::Release),
                None => break,
            }
        }
    }

    /// [`assign`](Self::assign) the inserts of every track and of the master, and return the map of their cells for
    /// the renderer. Control thread only (or before the stream starts).
    pub fn assign_project(&self, p: &Project) -> Arc<MeterCells> {
        let ids: HashSet<Id> = p.tracks.iter().flat_map(|t| t.channel.inserts.iter()).chain(p.master.inserts.iter()).map(|i| i.id).collect();
        self.assign(&ids);
        let owned = self.cells.iter().enumerate().map(|(k, c)| (c.owner.load(Ordering::Acquire), k)).filter(|(owner, _)| *owner != 0);
        Arc::new(MeterCells::new(owned))
    }

    /// The cell given to plug-in `id`, if any (a linear scan: not for the audio block).
    pub fn cell_of(&self, id: Id) -> Option<usize> {
        if id == 0 {
            return None;
        }
        self.cells.iter().position(|c| c.owner.load(Ordering::Acquire) == id)
    }

    /// Store the meters of plug-in `id` in `cell`. Audio thread only; real-time safe (atomic stores, no allocation,
    /// no lock). Peaks of the same plug-in fall with a short release; a cell out of range is ignored.
    pub fn write(&self, cell: usize, id: Id, input: f32, output: f32, reduction: Option<f32>) {
        let Some(c) = self.cells.get(cell) else { return };
        let (input, output, reduction) = if c.id.load(Ordering::Relaxed) == id {
            let prev_in = f32::from_bits(c.input.load(Ordering::Relaxed));
            let prev_out = f32::from_bits(c.output.load(Ordering::Relaxed));
            let prev_red = f32::from_bits(c.reduction.load(Ordering::Relaxed));
            let red = match reduction {
                Some(r) if !prev_red.is_nan() => Some(r.min(prev_red * RELEASE)),
                other => other,
            };
            (input.max(prev_in * RELEASE), output.max(prev_out * RELEASE), red)
        } else {
            (input, output, reduction)
        };
        let s = c.seq.load(Ordering::Relaxed);
        c.seq.store(s.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        c.input.store(input.to_bits(), Ordering::Relaxed);
        c.output.store(output.to_bits(), Ordering::Relaxed);
        c.reduction.store(reduction.unwrap_or(f32::NAN).to_bits(), Ordering::Relaxed);
        c.id.store(id, Ordering::Relaxed);
        c.seq.store(s.wrapping_add(2), Ordering::Release);
    }

    /// The meters of plug-in `id`, from any thread, without waiting: None when it has no cell, when the audio
    /// thread has not written it yet, or when a write kept overlapping the read.
    pub fn read(&self, id: Id) -> Option<PluginMeter> {
        let c = &self.cells[self.cell_of(id)?];
        for _ in 0..READ_ATTEMPTS {
            let s1 = c.seq.load(Ordering::Acquire);
            if s1 % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let input = f32::from_bits(c.input.load(Ordering::Relaxed));
            let output = f32::from_bits(c.output.load(Ordering::Relaxed));
            let reduction = f32::from_bits(c.reduction.load(Ordering::Relaxed));
            let stamp = c.id.load(Ordering::Relaxed);
            fence(Ordering::Acquire);
            let s2 = c.seq.load(Ordering::Relaxed);
            if s1 != s2 {
                continue;
            }
            if stamp != id || c.owner.load(Ordering::Acquire) != id {
                return None;
            }
            let gain_reduction_db = if reduction.is_nan() { None } else { Some(reduction) };
            return Some(PluginMeter { input_peak: input, output_peak: output, gain_reduction_db });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    fn ids(v: &[Id]) -> HashSet<Id> {
        v.iter().copied().collect()
    }

    #[test]
    fn a_written_meter_reads_back_by_id() {
        let m = PluginMeters::new(8);
        m.assign(&ids(&[5]));
        let cell = m.cell_of(5).expect("a cell for plug-in 5");
        m.write(cell, 5, 0.5, 0.25, Some(-6.0));
        assert_eq!(m.read(5), Some(PluginMeter { input_peak: 0.5, output_peak: 0.25, gain_reduction_db: Some(-6.0) }));
        m.write(cell, 5, 0.5, 0.25, None);
        assert_eq!(m.read(5).map(|x| x.gain_reduction_db), Some(None));
    }

    #[test]
    fn a_wrong_or_unknown_id_reads_nothing() {
        let m = PluginMeters::new(8);
        m.assign(&ids(&[5, 8]));
        assert_eq!(m.read(99), None);
        assert_eq!(m.read(8), None, "a cell that was never written");
        m.write(m.cell_of(5).unwrap(), 6, 0.5, 0.5, None);
        assert_eq!(m.read(5), None, "values stamped with another plug-in");
    }

    #[test]
    fn more_plug_ins_than_cells_never_panic() {
        let m = PluginMeters::new(4);
        m.assign(&(1..=10).collect());
        let with_cell: Vec<Id> = (1..=10).filter(|id| m.cell_of(*id).is_some()).collect();
        assert_eq!(with_cell, vec![1, 2, 3, 4]);
        m.write(100, 5, 1.0, 1.0, Some(-3.0));
        for id in 5..=10 {
            assert_eq!(m.read(id), None);
        }
        assert_eq!(m.capacity(), 4);
    }

    #[test]
    fn a_freed_cell_keeps_nothing_of_its_old_plug_in() {
        let m = PluginMeters::new(2);
        m.assign(&ids(&[1, 2]));
        m.write(m.cell_of(1).unwrap(), 1, 0.5, 0.5, Some(-3.0));
        m.write(m.cell_of(2).unwrap(), 2, 0.5, 0.5, Some(-3.0));
        m.assign(&ids(&[3]));
        assert_eq!(m.read(1), None);
        assert_eq!(m.read(2), None);
        let cell = m.cell_of(3).expect("a cell for plug-in 3");
        assert_eq!(m.read(3), None, "the old plug-in's values are not the new one's");
        m.write(cell, 3, 0.1, 0.1, None);
        assert_eq!(m.read(3), Some(PluginMeter { input_peak: 0.1, output_peak: 0.1, gain_reduction_db: None }));
    }

    #[test]
    fn values_fall_to_silence_with_a_short_release() {
        let m = PluginMeters::new(1);
        m.assign(&ids(&[1]));
        m.write(0, 1, 1.0, 1.0, Some(-12.0));
        m.write(0, 1, 0.0, 0.0, Some(0.0));
        let first = m.read(1).unwrap();
        assert!((first.input_peak - 0.97).abs() < 1e-6, "input {}", first.input_peak);
        for _ in 0..399 {
            m.write(0, 1, 0.0, 0.0, Some(0.0));
        }
        let last = m.read(1).unwrap();
        assert!(last.input_peak < 1e-4 && last.output_peak < 1e-4, "{last:?}");
        assert!(last.gain_reduction_db.unwrap() > -0.01, "{last:?}");
    }

    /// A project with a compressor and a Channel EQ on a track and a limiter on the master.
    fn three_inserts() -> (Project, [Id; 3]) {
        use ondina_core::command::Command;
        use ondina_core::model::{TrackKind, MASTER_ID};
        use ondina_core::ops::{self, Outcome};
        let mut p = Project::new("t");
        let run = |p: &mut Project, c: Command| match ops::apply(p, &c).unwrap() {
            Outcome::Done(v) => v,
            _ => panic!(),
        };
        let t = run(&mut p, Command::AddTrack { kind: TrackKind::Audio, name: None, patch: None, instrument: None, preset: None, index: None })["track_id"].as_u64().unwrap();
        let fx = |p: &mut Project, track_id: Id, kind: &str| run(p, Command::AddEffect { track_id, kind: kind.into(), preset: None, index: None })["plugin_id"].as_u64().unwrap();
        let comp = fx(&mut p, t, "compressor");
        let eq = fx(&mut p, t, "channel_eq");
        let lim = fx(&mut p, MASTER_ID, "limiter");
        (p, [comp, eq, lim])
    }

    #[test]
    fn meter_cells_map_every_assigned_insert() {
        let m = PluginMeters::new(8);
        let (p, inserts) = three_inserts();
        let cells = m.assign_project(&p);
        assert_eq!(cells.len(), 3);
        assert!(!cells.is_empty());
        for id in inserts {
            assert!(cells.cell(id).is_some(), "no cell for {id}");
            assert_eq!(cells.cell(id), m.cell_of(id));
        }
        assert_eq!(cells.cell(0), None);
        assert_eq!(cells.cell(9999), None);
    }

    #[test]
    fn meter_cells_new_sorts_and_skips_zero() {
        let cells = MeterCells::new([(9, 1), (0, 4), (3, 0)]);
        assert_eq!(cells.cell(3), Some(0));
        assert_eq!(cells.cell(9), Some(1));
        assert_eq!(cells.cell(0), None);
        assert_eq!(cells.len(), 2);
        assert!(MeterCells::default().is_empty());
        assert_eq!(MeterCells::new([(4, 2), (4, 3)]).len(), 1, "an id has one cell");
    }

    #[test]
    fn more_inserts_than_cells_map_only_those_with_a_cell() {
        let m = PluginMeters::new(4);
        m.assign(&(1..=10).collect());
        let cells = m.assign_project(&Project::new("empty"));
        assert!(cells.is_empty(), "an empty project frees every cell");
        let (mut p, _) = three_inserts();
        for _ in 0..7 {
            let mut extra = p.master.inserts[0].clone();
            extra.id = p.alloc_id();
            p.master.inserts.push(extra);
        }
        let cells = m.assign_project(&p);
        assert_eq!(cells.len(), 4);
        for i in p.tracks.iter().flat_map(|t| t.channel.inserts.iter()).chain(p.master.inserts.iter()) {
            assert_eq!(cells.cell(i.id), m.cell_of(i.id));
        }
    }

    #[test]
    fn concurrent_reads_never_mix_two_plug_ins() {
        let m = Arc::new(PluginMeters::new(1));
        m.assign(&ids(&[7]));
        let done = Arc::new(AtomicBool::new(false));
        let writer = {
            let (m, done) = (m.clone(), done.clone());
            std::thread::spawn(move || {
                for i in 0..200_000 {
                    if i % 2 == 0 {
                        m.write(0, 7, 0.7, 0.7, Some(-7.0));
                    } else {
                        m.write(0, 9, 0.9, 0.9, Some(-9.0));
                    }
                }
                done.store(true, Ordering::Release);
            })
        };
        let expected = PluginMeter { input_peak: 0.7, output_peak: 0.7, gain_reduction_db: Some(-7.0) };
        let mut seen = 0usize;
        while !done.load(Ordering::Acquire) {
            if let Some(v) = m.read(7) {
                assert_eq!(v, expected);
                seen += 1;
            }
        }
        writer.join().unwrap();
        if seen == 0 {
            m.write(0, 7, 0.7, 0.7, Some(-7.0));
            assert_eq!(m.read(7), Some(expected));
        }
    }
}
