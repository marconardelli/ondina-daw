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

    pub fn initial_bpm(&self) -> f64 {
        self.points[0].bpm
    }

    pub fn set_initial_bpm(&mut self, bpm: f64) {
        self.points[0].bpm = clamp_bpm(bpm);
    }

    pub fn add_change(&mut self, beat: f64, bpm: f64) {
        let bpm = clamp_bpm(bpm);
        if beat <= 0.0 {
            self.points[0].bpm = bpm;
            return;
        }
        self.points.retain(|p| (p.beat - beat).abs() > 1e-9);
        self.points.push(TempoPoint { beat, bpm });
        self.points.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap());
    }

    pub fn clear_changes(&mut self) {
        self.points.truncate(1);
    }

    pub fn bpm_at(&self, beat: f64) -> f64 {
        let idx = self.points.partition_point(|p| p.beat <= beat);
        self.points[idx.saturating_sub(1)].bpm
    }

    pub fn beat_to_sec(&self, beat: f64) -> f64 {
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

fn clamp_bpm(bpm: f64) -> f64 {
    bpm.clamp(5.0, 999.0)
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
