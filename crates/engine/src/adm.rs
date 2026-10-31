//! ADM BWF writer: ITU-R BS.2076 Audio Definition Model metadata (axml chunk) with the
//! BS.2088 channel allocation (chna chunk) in a RIFF/WAVE container, 24-bit PCM.
//! Layout: a DirectSpeakers bed (e.g. 7.1.2) followed by one mono Objects channel per object.

use std::io::Write;
use std::path::Path;

pub struct BedChannel {
    pub label: &'static str,
    /// ADM speaker label, e.g. "M+030", "LFE1", "U+090"
    pub speaker: &'static str,
    /// ADM convention: positive azimuth = left
    pub azimuth: f32,
    pub elevation: f32,
    pub audio: Vec<f32>,
}

pub struct ObjectBlock {
    pub start_sec: f64,
    pub duration_sec: f64,
    /// Ondina convention (positive = right); converted to ADM on write.
    pub azimuth: f32,
    pub elevation: f32,
    pub width: f32,
}

pub struct AdmObject {
    pub name: String,
    pub audio: Vec<f32>,
    pub blocks: Vec<ObjectBlock>,
}

fn tc(sec: f64) -> String {
    let sec = sec.max(0.0);
    let h = (sec / 3600.0).floor();
    let m = ((sec - h * 3600.0) / 60.0).floor();
    let s = sec - h * 3600.0 - m * 60.0;
    format!("{:02}:{:02}:{:08.5}", h as u32, m as u32, s)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Standard bed definitions (BS.2051 speaker labels).
pub fn bed_712() -> Vec<(&'static str, &'static str, f32, f32)> {
    vec![
        ("L", "M+030", 30.0, 0.0),
        ("R", "M-030", -30.0, 0.0),
        ("C", "M+000", 0.0, 0.0),
        ("LFE", "LFE1", 0.0, 0.0),
        ("Lss", "M+090", 90.0, 0.0),
        ("Rss", "M-090", -90.0, 0.0),
        ("Lrs", "M+135", 135.0, 0.0),
        ("Rrs", "M-135", -135.0, 0.0),
        ("Ltm", "U+090", 90.0, 45.0),
        ("Rtm", "U-090", -90.0, 45.0),
    ]
}

pub fn build_axml(programme: &str, sr: u32, duration: f64, bed: &[BedChannel], objects: &[AdmObject]) -> String {
    let mut x = String::new();
    let end = tc(duration);
    x += "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
    x += "<ebuCoreMain xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns=\"urn:ebu:metadata-schema:ebuCore_2016\" schema=\"EBU_CORE_20161111.xsd\" version=\"1.6\" xml:lang=\"en\">\n";
    x += "<coreMetadata><format><audioFormatExtended version=\"ITU-R_BS.2076-2\">\n";
    x += &format!("<audioProgramme audioProgrammeID=\"APR_1001\" audioProgrammeName=\"{}\" audioProgrammeLanguage=\"en\" start=\"00:00:00.00000\" end=\"{end}\">\n", esc(programme));
    x += "<audioContentIDRef>ACO_1001</audioContentIDRef>\n";
    for i in 0..objects.len() {
        x += &format!("<audioContentIDRef>ACO_{:04X}</audioContentIDRef>\n", 0x1002 + i);
    }
    x += "</audioProgramme>\n";
    // contents
    x += "<audioContent audioContentID=\"ACO_1001\" audioContentName=\"Bed\"><audioObjectIDRef>AO_1001</audioObjectIDRef></audioContent>\n";
    for (i, o) in objects.iter().enumerate() {
        x += &format!("<audioContent audioContentID=\"ACO_{:04X}\" audioContentName=\"{}\"><audioObjectIDRef>AO_{:04X}</audioObjectIDRef></audioContent>\n", 0x1002 + i, esc(&o.name), 0x1002 + i);
    }
    // bed object
    x += &format!("<audioObject audioObjectID=\"AO_1001\" audioObjectName=\"Bed\" start=\"00:00:00.00000\" duration=\"{end}\">\n<audioPackFormatIDRef>AP_00011001</audioPackFormatIDRef>\n");
    for i in 0..bed.len() {
        x += &format!("<audioTrackUIDRef>ATU_{:08X}</audioTrackUIDRef>\n", i + 1);
    }
    x += "</audioObject>\n";
    for (i, o) in objects.iter().enumerate() {
        x += &format!(
            "<audioObject audioObjectID=\"AO_{:04X}\" audioObjectName=\"{}\" start=\"00:00:00.00000\" duration=\"{end}\"><audioPackFormatIDRef>AP_{:08X}</audioPackFormatIDRef><audioTrackUIDRef>ATU_{:08X}</audioTrackUIDRef></audioObject>\n",
            0x1002 + i,
            esc(&o.name),
            0x0003_1001 + i,
            bed.len() + i + 1
        );
    }
    // bed pack + channels
    x += "<audioPackFormat audioPackFormatID=\"AP_00011001\" audioPackFormatName=\"bed\" typeLabel=\"0001\" typeDefinition=\"DirectSpeakers\">\n";
    for i in 0..bed.len() {
        x += &format!("<audioChannelFormatIDRef>AC_{:08X}</audioChannelFormatIDRef>\n", 0x0001_1001 + i);
    }
    x += "</audioPackFormat>\n";
    for (i, b) in bed.iter().enumerate() {
        let id = 0x0001_1001 + i;
        let lfe = if b.speaker.starts_with("LFE") { "<frequency typeDefinition=\"lowPass\">120</frequency>" } else { "" };
        x += &format!(
            "<audioChannelFormat audioChannelFormatID=\"AC_{id:08X}\" audioChannelFormatName=\"{}\" typeLabel=\"0001\" typeDefinition=\"DirectSpeakers\">{lfe}<audioBlockFormat audioBlockFormatID=\"AB_{id:08X}_00000001\"><speakerLabel>{}</speakerLabel><position coordinate=\"azimuth\">{:.1}</position><position coordinate=\"elevation\">{:.1}</position><position coordinate=\"distance\">1.0</position></audioBlockFormat></audioChannelFormat>\n",
            b.label, b.speaker, b.azimuth, b.elevation
        );
    }
    // object packs + channels
    for (i, o) in objects.iter().enumerate() {
        let pid = 0x0003_1001 + i;
        x += &format!("<audioPackFormat audioPackFormatID=\"AP_{pid:08X}\" audioPackFormatName=\"{}\" typeLabel=\"0003\" typeDefinition=\"Objects\"><audioChannelFormatIDRef>AC_{pid:08X}</audioChannelFormatIDRef></audioPackFormat>\n", esc(&o.name));
        x += &format!("<audioChannelFormat audioChannelFormatID=\"AC_{pid:08X}\" audioChannelFormatName=\"{}\" typeLabel=\"0003\" typeDefinition=\"Objects\">\n", esc(&o.name));
        for (k, b) in o.blocks.iter().enumerate() {
            x += &format!(
                "<audioBlockFormat audioBlockFormatID=\"AB_{pid:08X}_{:08X}\" rtime=\"{}\" duration=\"{}\"><position coordinate=\"azimuth\">{:.2}</position><position coordinate=\"elevation\">{:.2}</position><position coordinate=\"distance\">1.0</position><width>{:.1}</width></audioBlockFormat>\n",
                k + 1,
                tc(b.start_sec),
                tc(b.duration_sec),
                -b.azimuth,
                b.elevation,
                b.width * 90.0
            );
        }
        x += "</audioChannelFormat>\n";
    }
    // stream / track formats and track UIDs
    let all: Vec<(usize, u32, &str)> = (0..bed.len()).map(|i| (0x0001_1001 + i, 1u32, bed[i].label)).chain((0..objects.len()).map(|i| (0x0003_1001 + i, 3u32, objects[i].name.as_str()))).collect();
    for (k, (cid, tl, name)) in all.iter().enumerate() {
        let tl_s = format!("{:04}", tl);
        let pack = if *tl == 1 { 0x0001_1001 } else { *cid };
        x += &format!("<audioStreamFormat audioStreamFormatID=\"AS_{cid:08X}\" audioStreamFormatName=\"PCM_{}\" formatLabel=\"0001\" formatDefinition=\"PCM\"><audioChannelFormatIDRef>AC_{cid:08X}</audioChannelFormatIDRef><audioTrackFormatIDRef>AT_{cid:08X}_01</audioTrackFormatIDRef></audioStreamFormat>\n", esc(name));
        x += &format!("<audioTrackFormat audioTrackFormatID=\"AT_{cid:08X}_01\" audioTrackFormatName=\"PCM_{}\" formatLabel=\"0001\" formatDefinition=\"PCM\"><audioStreamFormatIDRef>AS_{cid:08X}</audioStreamFormatIDRef></audioTrackFormat>\n", esc(name));
        x += &format!("<audioTrackUID UID=\"ATU_{:08X}\" sampleRate=\"{sr}\" bitDepth=\"24\"><audioTrackFormatIDRef>AT_{cid:08X}_01</audioTrackFormatIDRef><audioPackFormatIDRef>AP_{pack:08X}</audioPackFormatIDRef></audioTrackUID>\n", k + 1);
        let _ = tl_s;
    }
    x += "</audioFormatExtended></format></coreMetadata></ebuCoreMain>\n";
    x
}

fn chna(bed_len: usize, objects: usize) -> Vec<u8> {
    let n = bed_len + objects;
    let mut c = vec![];
    c.extend_from_slice(&(n as u16).to_le_bytes());
    c.extend_from_slice(&(n as u16).to_le_bytes());
    for k in 0..n {
        let cid = if k < bed_len { 0x0001_1001 + k } else { 0x0003_1001 + (k - bed_len) };
        let pack = if k < bed_len { 0x0001_1001 } else { cid };
        c.extend_from_slice(&((k + 1) as u16).to_le_bytes());
        c.extend_from_slice(format!("ATU_{:08X}", k + 1).as_bytes()); // 12
        c.extend_from_slice(format!("AT_{cid:08X}_01").as_bytes()); // 14
        c.extend_from_slice(format!("AP_{pack:08X}").as_bytes()); // 11
        c.push(0);
    }
    c
}

/// Write the ADM BWF file. All channels must have the same length.
pub fn write_adm_bwf(path: &Path, programme: &str, sr: u32, bed: &[BedChannel], objects: &[AdmObject]) -> Result<(), String> {
    let n_ch = bed.len() + objects.len();
    let frames = bed.iter().map(|b| b.audio.len()).chain(objects.iter().map(|o| o.audio.len())).min().unwrap_or(0);
    let duration = frames as f64 / sr as f64;
    let mut axml = build_axml(programme, sr, duration, bed, objects);
    if axml.len() % 2 == 1 {
        // even-sized chunks avoid pad bytes, which some readers mishandle
        axml.push(' ');
    }
    let chna = chna(bed.len(), objects.len());
    let block_align = n_ch * 3;
    let data_len = frames * block_align;
    let mut fmt = vec![];
    fmt.extend_from_slice(&1u16.to_le_bytes()); // PCM
    fmt.extend_from_slice(&(n_ch as u16).to_le_bytes());
    fmt.extend_from_slice(&sr.to_le_bytes());
    fmt.extend_from_slice(&((sr as usize * block_align) as u32).to_le_bytes());
    fmt.extend_from_slice(&(block_align as u16).to_le_bytes());
    fmt.extend_from_slice(&24u16.to_le_bytes());
    let pad = |n: usize| n % 2;
    let chunk_total = |len: usize| 8 + len + pad(len);
    let riff_len = 4 + chunk_total(fmt.len()) + chunk_total(chna.len()) + chunk_total(axml.len()) + chunk_total(data_len);
    if riff_len > u32::MAX as usize {
        return Err("export longer than the 4 GB RIFF limit".into());
    }
    if let Some(p) = path.parent() {
        if !p.as_os_str().is_empty() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
    }
    let f = std::fs::File::create(path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let mut w = std::io::BufWriter::new(f);
    let put = |id: &[u8], body: &[u8], w: &mut std::io::BufWriter<std::fs::File>| -> std::io::Result<()> {
        w.write_all(id)?;
        w.write_all(&(body.len() as u32).to_le_bytes())?;
        w.write_all(body)?;
        if body.len() % 2 == 1 {
            w.write_all(&[0])?;
        }
        Ok(())
    };
    (|| -> std::io::Result<()> {
        w.write_all(b"RIFF")?;
        w.write_all(&(riff_len as u32).to_le_bytes())?;
        w.write_all(b"WAVE")?;
        put(b"fmt ", &fmt, &mut w)?;
        put(b"chna", &chna, &mut w)?;
        put(b"axml", axml.as_bytes(), &mut w)?;
        w.write_all(b"data")?;
        w.write_all(&(data_len as u32).to_le_bytes())?;
        let chans: Vec<&Vec<f32>> = bed.iter().map(|b| &b.audio).chain(objects.iter().map(|o| &o.audio)).collect();
        let mut frame = Vec::with_capacity(block_align);
        for i in 0..frames {
            frame.clear();
            for c in &chans {
                let v = (c[i].clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
                frame.extend_from_slice(&v.to_le_bytes()[..3]);
            }
            w.write_all(&frame)?;
        }
        if data_len % 2 == 1 {
            w.write_all(&[0])?;
        }
        w.flush()
    })()
    .map_err(|e| e.to_string())
}
