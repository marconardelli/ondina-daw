//! Ondina DAW — desktop application entry point.
//!
//! `ondina [project.ondina] [--no-audio] [--mcp-port N] [--no-mcp]`

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod arrange;
mod arrange_view;
mod assistant;
mod cc_lane;
mod doc_guard;
mod editors;
mod extra_ui;
mod inspector;
mod mixer;
mod note_clipboard;
mod note_selection;
mod piano_roll_view;
mod theme;

use ondina_studio::{Session, Settings, Source};

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
            ondina_core::Command::SetAudioDevice {
                output: Some(out),
                input: None,
            },
            Source::Script,
        );
    }
    if let Some(project) = args.iter().find(|a| ondina_core::is_project_file(a)) {
        if let Err(e) = session.lock().execute(
            ondina_core::Command::OpenProject {
                path: project.clone(),
            },
            Source::Script,
        ) {
            eprintln!("[ondina] {e}");
        }
    }

    let port = value("--mcp-port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(settings.mcp_port);
    let (mcp, mcp_error) = if settings.mcp_enabled && !has("--no-mcp") {
        match ondina_studio::mcp::serve_http(session.clone(), port) {
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
            .with_title("Ondina DAW"),
        ..Default::default()
    };
    eframe::run_native(
        "Ondina DAW",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::OndinaApp::new(
                cc, session, settings, mcp, mcp_error,
            )))
        }),
    )
}
