//! Aurora DAW — desktop application entry point.
//!
//! `aurora [project.aurora] [--no-audio] [--mcp-port N] [--no-mcp]`

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod arrange;
mod assistant;
mod doc_guard;
mod editors;
mod extra_ui;
mod inspector;
mod mixer;
mod theme;

use aurora_studio::{Session, Settings, Source};

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let value = |f: &str| {
        args.iter()
            .position(|a| a == f)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let settings = Settings::load();

    let session = Session::shared(!has("--no-audio"));
    if let Some(out) = settings.audio_output.clone() {
        let _ = session.lock().execute(
            aurora_core::Command::SetAudioDevice {
                output: Some(out),
                input: None,
            },
            Source::Script,
        );
    }
    if let Some(project) = args.iter().find(|a| a.ends_with(".aurora")) {
        if let Err(e) = session.lock().execute(
            aurora_core::Command::OpenProject {
                path: project.clone(),
            },
            Source::Script,
        ) {
            eprintln!("[aurora] {e}");
        }
    }

    let port = value("--mcp-port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(settings.mcp_port);
    let (mcp, mcp_error) = if settings.mcp_enabled && !has("--no-mcp") {
        match aurora_studio::mcp::serve_http(session.clone(), port) {
            Ok(h) => (Some(h), None),
            Err(e) => (None, Some(e)),
        }
    } else {
        (None, None)
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1500.0, 920.0])
            .with_min_inner_size([960.0, 600.0])
            .with_title("Aurora DAW"),
        ..Default::default()
    };
    eframe::run_native(
        "Aurora DAW",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::AuroraApp::new(
                cc, session, settings, mcp, mcp_error,
            )))
        }),
    )
}
