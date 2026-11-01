//! Piecewise-constant tempo map: converts between beats and seconds.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct TempoPoint {
    pub beat: f64,
    pub bpm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoMap {
    /// Always sorted, first point always at beat 0.
    pub points: Vec<TempoPoint>,
}

impl TempoMap {
    pub fn new(bpm: f64) -> Self {
        Self { points: vec![TempoPoint { beat: 0.0, bpm: clamp_bpm(bpm) }] }
    }

    /// The tempo at beat 0. A map without points (only a file built by hand has one: `load_project_str` refuses
    /// it) answers `DEFAULT_BPM` instead of panicking, because the audio thread asks too.
    pub fn initial_bpm(&self) -> f64 {
        self.points.first().map_or(DEFAULT_BPM, |p| p.bpm)
    }

    pub fn set_initial_bpm(&mut self, bpm: f64) {
        self.ensure_first_point();
        self.points[0].bpm = clamp_bpm(bpm);
    }

    pub fn add_change(&mut self, beat: f64, bpm: f64) {
        let bpm = clamp_bpm(bpm);
        self.ensure_first_point();
        if beat <= 0.0 {
            self.points[0].bpm = bpm;
            return;
        }
        self.points.retain(|p| (p.beat - beat).abs() > 1e-9);
        self.points.push(TempoPoint { beat, bpm });
        self.points.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap());
    }

    pub fn clear_changes(&mut self) {
        self.ensure_first_point();
        self.points.truncate(1);
    }

    /// A map always has a point at beat 0; one without points gets the default tempo there.
    fn ensure_first_point(&mut self) {
        if self.points.is_empty() {
            self.points.push(TempoPoint { beat: 0.0, bpm: DEFAULT_BPM });
        }
    }

    pub fn bpm_at(&self, beat: f64) -> f64 {
        let idx = self.points.partition_point(|p| p.beat <= beat);
        self.points.get(idx.saturating_sub(1)).map_or(DEFAULT_BPM, |p| p.bpm)
    }

    pub fn beat_to_sec(&self, beat: f64) -> f64 {
        if self.points.is_empty() {
            return beat * 60.0 / DEFAULT_BPM;
        }
        let mut sec = 0.0;
        for (i, p) in self.points.iter().enumerate() {
            let next = self.points.get(i + 1).map(|n| n.beat).unwrap_or(f64::INFINITY);
            if beat <= next {
                return sec + (beat - p.beat) * 60.0 / p.bpm;
            }
            sec += (next - p.beat) * 60.0 / p.bpm;
        }
        sec
    }

    pub fn sec_to_beat(&self, sec: f64) -> f64 {
        if self.points.is_empty() {
            return sec * DEFAULT_BPM / 60.0;
        }
        let mut acc = 0.0;
        for (i, p) in self.points.iter().enumerate() {
            let next = self.points.get(i + 1).map(|n| n.beat);
            let seg = match next {
                Some(nb) => (nb - p.beat) * 60.0 / p.bpm,
                None => f64::INFINITY,
            };
            if sec <= acc + seg {
                return p.beat + (sec - acc) * p.bpm / 60.0;
            }
            acc += seg;
        }
        0.0
    }
}

/// Tempo of a new project, and the answer of a tempo map without points.
pub const DEFAULT_BPM: f64 = 120.0;
/// Range every tempo is kept in.
pub const MIN_BPM: f64 = 5.0;
pub const MAX_BPM: f64 = 999.0;

pub(crate) fn clamp_bpm(bpm: f64) -> f64 {
    bpm.clamp(MIN_BPM, MAX_BPM)
}

impl TempoMap {
    /// Puts a tempo map read from a file back in the shape the rest of the code expects: sorted, the first point
    /// at beat 0, every tempo in range. A map without points, or with a position or tempo that is not a number,
    /// is refused.
    pub(crate) fn repair_loaded(&mut self) -> Result<(), String> {
        if self.points.is_empty() {
            return Err("the tempo map has no points".into());
        }
        if self.points.iter().any(|p| !p.beat.is_finite() || !p.bpm.is_finite()) {
            return Err("the tempo map has a position or a tempo that is not a number".into());
        }
        for p in &mut self.points {
            p.beat = p.beat.max(0.0);
        }
        self.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        self.points[0].beat = 0.0;
        self.points.dedup_by(|b, a| (a.beat - b.beat).abs() <= 1e-9);
        for p in &mut self.points {
            p.bpm = clamp_bpm(p.bpm);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut m = TempoMap::new(120.0);
        m.add_change(8.0, 60.0);
        assert!((m.beat_to_sec(8.0) - 4.0).abs() < 1e-9);
        assert!((m.beat_to_sec(10.0) - 6.0).abs() < 1e-9);
        for b in [0.0, 3.3, 8.0, 9.5, 100.0] {
            assert!((m.sec_to_beat(m.beat_to_sec(b)) - b).abs() < 1e-9);
        }
    }
}
