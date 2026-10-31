//! End-to-end test of the offline AI assistant with a real local model.
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn main() {
    let prompt = std::env::args().nth(1).unwrap_or_else(|| "Set the tempo to 90 bpm, add a drum track with the Pop Drum Kit patch and generate 4 bars of rock drums on it starting at beat 0.".into());
    let session = aurora_studio::Session::shared(false);
    let settings = aurora_studio::Settings { provider: "local".into(), ..Default::default() };
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut a = aurora_studio::local_ai::LocalAssistant::default();
    let t = std::time::Instant::now();
    a.run_turn(&session, &settings, &prompt, "", &tx, &Arc::new(AtomicBool::new(false)));
    for ev in rx.try_iter() {
        match ev {
            aurora_studio::ai::AiEvent::ToolCall { name, input, ok, .. } => println!("TOOL {} {} {}", if ok { "ok " } else { "ERR" }, name, input),
            aurora_studio::ai::AiEvent::Text(t) => println!("TEXT {t}"),
            aurora_studio::ai::AiEvent::Error(e) => println!("ERROR {e}"),
            _ => {}
        }
    }
    let s = session.lock();
    println!("--- after {:.0}s: tempo {} bpm", t.elapsed().as_secs_f32(), s.project.tempo.initial_bpm());
    for tr in &s.project.tracks {
        let notes: usize = tr.regions.iter().map(|r| match &r.content { aurora_core::RegionContent::Midi(m) => m.notes.len(), _ => 0 }).sum();
        println!("track '{}' instrument={:?} regions={} notes={}", tr.name, tr.instrument.as_ref().map(|i| i.kind.clone()), tr.regions.len(), notes);
    }
}
