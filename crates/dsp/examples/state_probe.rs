use ondina_dsp::Instrument;
fn main() {
    let path = "tools/vst3/Dexed.vst3";
    let (_, _, params) = ondina_dsp::vst3::probe(path).unwrap();
    let cutoff = params.iter().find(|p| p.name == "Cutoff").unwrap().id;
    let algo = params.iter().find(|p| p.name.to_uppercase() == "ALGORITHM").map(|p| p.id);
    let mut a = ondina_dsp::vst3::Vst3Processor::load(path, 48000.0, true).unwrap();
    a.set_param(&format!("p{cutoff}"), 0.31);
    if let Some(al) = algo { a.set_param(&format!("p{al}"), 0.7); }
    let ctx = ondina_dsp::ProcessContext::idle(48000.0);
    for _ in 0..10 { let mut l = vec![0.0; 512]; let mut r = vec![0.0; 512]; a.render(&mut l, &mut r, &ctx); }
    let blob = a.save_state().unwrap();
    let encoded = ondina_core::model::base64_encode(&blob);
    let decoded = ondina_core::model::base64_decode(&encoded).unwrap();
    let mut b = ondina_dsp::vst3::Vst3Processor::load(path, 48000.0, true).unwrap();
    println!("B before: cutoff={:?}", b.param_value(cutoff));
    b.load_state(&decoded);
    println!("state {} bytes, B after: cutoff={:?} algorithm={:?} (A: {:?} {:?})", blob.len(), b.param_value(cutoff), algo.and_then(|x| b.param_value(x)), a.param_value(cutoff), algo.and_then(|x| a.param_value(x)));
}
