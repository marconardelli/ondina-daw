//! The repeats the built-in delays play after one hit, for drawing: when each repeat comes, on which side and how
//! loud. Plain math the delay windows draw and the tests compare with the delays themselves; nothing here runs on
//! the audio thread.

use std::collections::BTreeMap;

pub const FLOOR_DB: f32 = -60.0;
/// -60 dB.
pub const FLOOR_GAIN: f32 = 0.001;
/// Most repeats one call returns: enough for 0.95 feedback on a whole window, small enough for every frame.
pub const MAX_TAPS: usize = 256;
/// The counts below stop here, for a feedback that would never let the repeats fade.
const MAX_COUNT: u32 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Mono,
}

/// One repeat: when it comes after the hit, on which side, at what gain (1 = the hit's level), and on which trip
/// through the delay (1 = the first repeat).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tap {
    pub ms: f32,
    pub side: Side,
    pub gain: f32,
    pub pass: u32,
}

/// The Stereo Delay's repeats of a hit in the middle (both channels), up to `window_ms`, louder than FLOOR_GAIN,
/// sorted by time. Each repeat feeds its own side with feedback × (1 - crossfeed) and the other with
/// feedback × crossfeed, as StereoDelay::process does; the Low and High Cut are left out.
pub fn stereo_echoes(left_ms: f32, right_ms: f32, feedback: f32, crossfeed: f32, window_ms: f32) -> Vec<Tap> {
    // times in whole microseconds, so a repeat reached along two paths lands on one key and its gains add up
    let us = |ms: f32| (ms.max(1.0) as f64 * 1000.0).round() as u64;
    let step = [us(left_ms), us(right_ms)];
    let feedback = if feedback.is_finite() { feedback.max(0.0) } else { 0.0 };
    let crossfeed = if crossfeed.is_finite() { crossfeed.clamp(0.0, 1.0) } else { 0.0 };
    let window_us = if window_ms.is_finite() { (window_ms.max(0.0) as f64 * 1000.0) as u64 } else { 0 };
    let mut front: BTreeMap<(u64, u8), (f32, u32)> = BTreeMap::new();
    front.insert((step[0], 0), (1.0, 1));
    front.insert((step[1], 1), (1.0, 1));
    let mut taps = Vec::new();
    while let Some(((t, side), (gain, pass))) = front.pop_first() {
        if t > window_us || taps.len() >= MAX_TAPS {
            break;
        }
        if gain < FLOOR_GAIN {
            // its children are quieter still
            continue;
        }
        taps.push(Tap { ms: t as f32 / 1000.0, side: if side == 0 { Side::Left } else { Side::Right }, gain, pass });
        let other = 1 - side;
        for (to, g) in [(side, gain * feedback * (1.0 - crossfeed)), (other, gain * feedback * crossfeed)] {
            // two tiny contributions never add up to a repeat above -60 dB that anyone could see
            if g < FLOOR_GAIN / 4.0 {
                continue;
            }
            let e = front.entry((t + step[to as usize], to)).or_insert((0.0, pass + 1));
            e.0 += g;
            e.1 = e.1.min(pass + 1);
        }
    }
    taps
}

/// The gain of the next tape repeat after one of gain `a`: feedback × tanh(a × drive) / tanh(drive) with
/// drive = 1 + 4 × saturation, the saturating feedback of TapeDelay::process.
pub fn tape_next_gain(a: f32, feedback: f32, saturation: f32) -> f32 {
    let drive = 1.0 + saturation * 4.0;
    feedback.min(1.05) * (a * drive).tanh() / drive.tanh()
}

/// The Tape Delay's repeats of a full-level hit (first repeat at gain 1), up to `window_ms`, louder than FLOOR_GAIN.
pub fn tape_echoes(time_ms: f32, feedback: f32, saturation: f32, window_ms: f32) -> Vec<Tap> {
    let time_ms = time_ms.max(1.0);
    let mut taps = Vec::new();
    let mut a = 1.0f32;
    let mut pass = 1u32;
    while a >= FLOOR_GAIN && taps.len() < MAX_TAPS {
        let ms = time_ms * pass as f32;
        if ms > window_ms {
            break;
        }
        taps.push(Tap { ms, side: Side::Mono, gain: a, pass });
        a = tape_next_gain(a, feedback, saturation);
        pass += 1;
    }
    taps
}

/// How many repeats per side the Stereo Delay plays above -60 dB with no ping-pong (1 with feedback 0). Counted by
/// multiplying, as stereo_echoes does, so the two never disagree at the edge of -60 dB.
pub fn stereo_repeats_to_floor(feedback: f32) -> u32 {
    let mut g = 1.0f32;
    let mut n = 0;
    while g >= FLOOR_GAIN && n < MAX_COUNT {
        n += 1;
        // the same product as stereo_echoes with no ping-pong: gain × feedback × 1
        g *= feedback;
    }
    n
}

/// How many tape repeats stay above -60 dB, or None when they never fade (see tape_sustain_db).
pub fn tape_repeats_to_floor(feedback: f32, saturation: f32) -> Option<u32> {
    if tape_sustain_db(feedback, saturation).is_some() {
        return None;
    }
    let mut a = 1.0f32;
    let mut n = 0;
    while a >= FLOOR_GAIN && n < MAX_COUNT {
        n += 1;
        a = tape_next_gain(a, feedback, saturation);
    }
    Some(n)
}

/// The level (dB) tape repeats settle at when the saturating feedback holds them up instead of letting them fade:
/// Some when feedback × drive / tanh(drive) ≥ 1, None otherwise.
pub fn tape_sustain_db(feedback: f32, saturation: f32) -> Option<f32> {
    let drive = 1.0 + saturation * 4.0;
    if feedback.min(1.05) * drive / drive.tanh() < 1.0 {
        return None;
    }
    let mut a = 1.0f32;
    for _ in 0..500 {
        a = tape_next_gain(a, feedback, saturation);
    }
    Some(20.0 * a.max(1e-9).log10())
}

/// The cutoff of the Tape Delay's Tone filter (800 + 9000 × tone Hz, as TapeDelay::process sets it).
pub fn tape_tone_hz(tone: f32) -> f32 {
    800.0 + tone * 9000.0
}

/// How much of the brightness (at 5 kHz) is left after `pass` trips through the Tone filter, 0..=1:
/// (1 / sqrt(1 + (5000 / cutoff)^2))^(pass - 1); the first repeat is not filtered yet.
pub fn tape_brightness(tone: f32, pass: u32) -> f32 {
    let ratio = 5000.0 / tape_tone_hz(tone.clamp(0.0, 1.0));
    let per_pass = 1.0 / (1.0 + ratio * ratio).sqrt();
    per_pass.powi(pass.saturating_sub(1).min(i32::MAX as u32) as i32).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    fn side(taps: &[Tap], s: Side) -> Vec<Tap> {
        taps.iter().copied().filter(|t| t.side == s).collect()
    }

    #[test]
    fn stereo_echoes_fall_by_the_feedback_and_stop_under_minus_60_db() {
        let taps = stereo_echoes(375.0, 375.0, 0.5, 0.0, 60000.0);
        for s in [Side::Left, Side::Right] {
            let one = side(&taps, s);
            assert_eq!(one.len(), 10, "{s:?}: {one:?}");
            for (k, t) in one.iter().enumerate() {
                assert!(near(t.ms, 375.0 * (k + 1) as f32, 1e-3), "{t:?}");
                assert!(near(t.gain, 0.5f32.powi(k as i32), 1e-6), "{t:?}");
                // each repeat is 6.02 dB under the one before
                assert!(near(20.0 * t.gain.log10(), -6.0206 * k as f32, 1e-2), "{t:?}");
                assert!(t.gain >= FLOOR_GAIN);
                assert_eq!(t.pass, k as u32 + 1);
            }
        }
        assert!(taps.iter().all(|t| t.gain > 0.5f32.powi(10)));
    }

    #[test]
    fn crossfeed_alternates_the_sides() {
        let taps = stereo_echoes(300.0, 200.0, 0.5, 1.0, 60000.0);
        let l = side(&taps, Side::Left);
        let r = side(&taps, Side::Right);
        assert!(near(l[0].ms, 300.0, 1e-3) && near(l[0].gain, 1.0, 1e-6), "{l:?}");
        assert!(near(l[1].ms, 500.0, 1e-3) && near(l[1].gain, 0.5, 1e-6), "{l:?}");
        assert!(near(r[0].ms, 200.0, 1e-3) && near(r[0].gain, 1.0, 1e-6), "{r:?}");
        assert!(near(r[1].ms, 500.0, 1e-3) && near(r[1].gain, 0.5, 1e-6), "{r:?}");
        assert!(!l.iter().any(|t| near(t.ms, 600.0, 1e-3)), "{l:?}");
    }

    #[test]
    fn coincident_repeats_add_up() {
        let fb = 0.6;
        let taps = stereo_echoes(250.0, 250.0, fb, 0.5, 60000.0);
        let l = side(&taps, Side::Left);
        assert!(near(l[1].ms, 500.0, 1e-3), "{l:?}");
        assert!(near(l[1].gain, fb, 1e-6), "{l:?}");
        assert_eq!(l[1].pass, 2);
    }

    #[test]
    fn echoes_stay_inside_the_window_and_the_cap() {
        let taps = stereo_echoes(1.0, 1.0, 0.95, 0.0, 60000.0);
        assert!(taps.len() <= MAX_TAPS);
        assert!(taps.windows(2).all(|w| w[0].ms <= w[1].ms));
        let taps = stereo_echoes(375.0, 250.0, 0.9, 0.3, 1000.0);
        assert!(!taps.is_empty() && taps.iter().all(|t| t.ms <= 1000.0));
        assert!(taps.windows(2).all(|w| w[0].ms <= w[1].ms));
        let taps = stereo_echoes(375.0, 250.0, 0.0, 0.3, 60000.0);
        assert_eq!(taps.len(), 2, "{taps:?}");
        assert_eq!(side(&taps, Side::Left).len(), 1);
        assert!(tape_echoes(1.0, 1.0, 1.0, 60000.0).len() <= MAX_TAPS);
        assert!(tape_echoes(250.0, 0.5, 0.0, 600.0).iter().all(|t| t.ms <= 600.0));
    }

    #[test]
    fn tape_echoes_follow_the_saturating_feedback() {
        let taps = tape_echoes(250.0, 0.5, 0.0, 1e9);
        let gains: Vec<f32> = taps.iter().map(|t| t.gain).collect();
        for (g, want) in gains.iter().zip([1.0, 0.5, 0.30339, 0.19329]) {
            assert!(near(*g, want, 1e-4), "{gains:?}");
        }
        assert_eq!(taps.len(), 16);
        assert!(taps.iter().all(|t| t.side == Side::Mono));
        assert!(near(taps[2].ms, 750.0, 1e-3));
        let taps = tape_echoes(250.0, 0.4, 0.3, 1e9);
        for (t, want) in taps.iter().zip([1.0, 0.4, 0.28959, 0.23077]) {
            assert!(near(t.gain, want, 1e-4), "{taps:?}");
        }
        assert_eq!(taps.len(), 53);
    }

    #[test]
    fn tape_sustain_when_the_tape_holds_the_repeats() {
        let held = tape_sustain_db(0.5, 0.3).unwrap();
        assert!(near(held, -10.92, 0.05), "{held}");
        assert_eq!(tape_sustain_db(0.4, 0.3), None);
        assert_eq!(tape_repeats_to_floor(0.5, 0.3), None);
        assert_eq!(tape_repeats_to_floor(0.4, 0.3), Some(53));
        assert_eq!(tape_repeats_to_floor(0.5, 0.0), Some(16));
    }

    #[test]
    fn repeats_to_floor_counts_like_the_echoes() {
        assert_eq!(stereo_repeats_to_floor(0.5), 10);
        assert_eq!(stereo_repeats_to_floor(0.35), 7);
        assert_eq!(stereo_repeats_to_floor(0.95), 135);
        assert_eq!(stereo_repeats_to_floor(0.0), 1);
        let taps = stereo_echoes(100.0, 100.0, 0.35, 0.0, 1e9);
        assert_eq!(side(&taps, Side::Left).len(), 7);
        let taps = stereo_echoes(100.0, 100.0, 0.95, 0.0, 1e9);
        assert_eq!(side(&taps, Side::Left).len(), 128, "the cap splits evenly between the sides");
    }

    #[test]
    fn tape_brightness_darkens_with_tone() {
        assert_eq!(tape_brightness(0.0, 1), 1.0);
        assert_eq!(tape_brightness(1.0, 1), 1.0);
        assert!(tape_brightness(0.0, 4) < tape_brightness(1.0, 4));
        for tone in [0.0, 0.3, 1.0] {
            for pass in 1..20 {
                let b = tape_brightness(tone, pass);
                assert!((0.0..=1.0).contains(&b), "{tone} {pass}: {b}");
                assert!(b <= tape_brightness(tone, pass.max(2) - 1) + 1e-6);
            }
        }
        assert_eq!(tape_tone_hz(0.4), 4400.0);
    }
}
