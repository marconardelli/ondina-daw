fn main() {
    let env = ort::init().build().unwrap();
    for name in ["vocals", "accompaniment"] {
        let path = format!("tools/models/sherpa-onnx-spleeter-2stems-fp16/{name}.fp16.onnx");
        let s = ort::session::Session::builder(&env).unwrap().commit_from_file(&path).unwrap();
        for i in s.inputs() { println!("{name} input: {} {:?}", i.name(), i.dtype()); }
        for o in s.outputs() { println!("{name} output: {} {:?}", o.name(), o.dtype()); }
        if let Ok(m) = s.metadata() { if let Ok(keys) = m.custom_keys() { for k in keys { println!("  meta {k} = {:?}", m.custom(&k)); } } };
    }
}
