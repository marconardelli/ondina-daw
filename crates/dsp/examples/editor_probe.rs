use std::sync::{Arc, Mutex};
fn main() {
    let mut host = vst3_host::Vst3Host::new().unwrap();
    let path = std::fs::canonicalize("tools/vst3/Dexed.vst3").unwrap();
    let p = host.load_plugin(&path).unwrap();
    println!("has editor: {}", p.has_editor());
    let plugin = Arc::new(Mutex::new(p));
    let mut win = vst3_host::PluginWindow::new(plugin.clone());
    win.open().unwrap();
    println!("editor open: {}", win.is_open());
    let t = std::time::Instant::now();
    while t.elapsed().as_secs() < 8 && !win.closed_by_user() {
        let _ = win.service_platform_events();
        let edits = plugin.lock().unwrap().take_parameter_edits();
        for e in edits { println!("edit {:?}", e); }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    win.close();
    println!("closed");
}
