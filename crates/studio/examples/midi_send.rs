//! Send a short note sequence to a MIDI output port (used to test MIDI input through loopMIDI).
fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "loopMIDI".into());
    let out = midir::MidiOutput::new("ondina-test-sender").unwrap();
    let port = out.ports().into_iter().find(|p| out.port_name(p).map(|n| n.contains(&name)).unwrap_or(false)).expect("port not found");
    println!("sending to {}", out.port_name(&port).unwrap());
    let mut conn = out.connect(&port, "send").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    for (note, ch) in [(48u8, 0u8), (60, 0), (64, 0), (67, 0), (72, 1)] {
        conn.send(&[0x90 | ch, note, 100]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(350));
        conn.send(&[0x80 | ch, note, 0]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    println!("done");
}
