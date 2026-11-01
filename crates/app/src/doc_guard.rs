//! Guard against losing unsaved work: decides when to ask "Save changes?" before an action
//! replaces the current document, and what happens after the answer. Pure logic, tested without a GUI.

/// An action that throws away the current document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocAction {
    /// Close the main window (quit Ondina).
    Close,
    /// File > New Project.
    New,
    /// File > Open: `None` shows the file picker, `Some(path)` opens that file (recent projects).
    Open(Option<String>),
}

/// The user's answer to the prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Save,
    DontSave,
    Cancel,
}

/// What to do with a requested action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing would be lost: run it now.
    Proceed(DocAction),
    /// Ask first; the action waits for the answer.
    Ask(DocAction),
}

/// What to do when the window is asked to close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseStep {
    /// Let the window close.
    Allow,
    /// Cancel the close and show the prompt with `DocAction::Close`.
    CancelAndAsk,
}

pub fn on_request(action: DocAction, unsaved: bool) -> Step {
    if unsaved {
        Step::Ask(action)
    } else {
        Step::Proceed(action)
    }
}

/// `saved` is the outcome of the save attempt and only matters for `Answer::Save`: a cancelled
/// Save As dialog or a failed write must not lose the work, so the action is dropped.
pub fn on_answer(action: DocAction, answer: Answer, saved: bool) -> Option<DocAction> {
    match answer {
        Answer::Save if saved => Some(action),
        Answer::Save | Answer::Cancel => None,
        Answer::DontSave => Some(action),
    }
}

/// `allow_close` is set once the user already chose to quit (our own `ViewportCommand::Close`
/// comes back as a close request on the next frame). A close request while the prompt is open
/// replaces the waiting action: the window button is the latest thing the user asked for.
pub fn on_close_requested(allow_close: bool, prompt_open: bool, unsaved: bool) -> CloseStep {
    if allow_close {
        CloseStep::Allow
    } else if prompt_open || unsaved {
        CloseStep::CancelAndAsk
    } else {
        CloseStep::Allow
    }
}

/// The question shown in the prompt.
pub fn prompt_question(project_name: &str, action: &DocAction) -> String {
    let name = if project_name.trim().is_empty() { "Untitled" } else { project_name };
    let what = match action {
        DocAction::Close => "closing",
        DocAction::New => "creating a new project",
        DocAction::Open(_) => "opening another project",
    };
    format!("Save changes to '{name}' before {what}?")
}

/// The status bar line after a project was opened: what could not be loaded, so that a silent
/// track has an explanation.
pub fn open_status(path: &str, result: &serde_json::Value) -> String {
    let count = |key: &str| result[key].as_array().map_or(0, Vec::len);
    let mut s = format!("Opened {path}");
    let n = count("blocked_plugins");
    if n > 0 {
        s += &format!(" · {n} VST3 {} not loaded (path refused)", if n == 1 { "plug-in" } else { "plug-ins" });
    }
    let m = count("missing_audio_files");
    if m > 0 {
        s += &format!(" · {m} audio {} missing", if m == 1 { "file" } else { "files" });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_actions() -> Vec<DocAction> {
        vec![
            DocAction::Close,
            DocAction::New,
            DocAction::Open(None),
            DocAction::Open(Some("C:/songs/old.ondina".into())),
        ]
    }

    #[test]
    fn clean_project_proceeds_without_prompt() {
        for a in all_actions() {
            assert_eq!(on_request(a.clone(), false), Step::Proceed(a));
        }
    }

    #[test]
    fn unsaved_project_asks_first() {
        for a in all_actions() {
            assert_eq!(on_request(a.clone(), true), Step::Ask(a));
        }
    }

    #[test]
    fn save_proceeds_only_when_the_save_succeeded() {
        for a in all_actions() {
            assert_eq!(on_answer(a.clone(), Answer::Save, true), Some(a.clone()));
            // Save As dialog cancelled, or the file could not be written
            assert_eq!(on_answer(a, Answer::Save, false), None);
        }
    }

    #[test]
    fn dont_save_proceeds_and_cancel_drops() {
        for a in all_actions() {
            for saved in [false, true] {
                assert_eq!(on_answer(a.clone(), Answer::DontSave, saved), Some(a.clone()));
                assert_eq!(on_answer(a.clone(), Answer::Cancel, saved), None);
            }
        }
    }

    #[test]
    fn close_request_rules() {
        use CloseStep::*;
        for (allow, open, unsaved, want) in [
            (true, false, false, Allow),
            (true, true, true, Allow),
            (true, false, true, Allow),
            (false, false, false, Allow),
            (false, false, true, CancelAndAsk),
            (false, true, false, CancelAndAsk),
            (false, true, true, CancelAndAsk),
        ] {
            assert_eq!(on_close_requested(allow, open, unsaved), want, "allow={allow} open={open} unsaved={unsaved}");
        }
    }

    #[test]
    fn prompt_question_names_project_and_action() {
        assert_eq!(prompt_question("Lofi Beat", &DocAction::Close), "Save changes to 'Lofi Beat' before closing?");
        assert_eq!(
            prompt_question("Lofi Beat", &DocAction::New),
            "Save changes to 'Lofi Beat' before creating a new project?"
        );
        assert_eq!(
            prompt_question("Lofi Beat", &DocAction::Open(Some("x.ondina".into()))),
            "Save changes to 'Lofi Beat' before opening another project?"
        );
        assert_eq!(prompt_question("  ", &DocAction::Open(None)), "Save changes to 'Untitled' before opening another project?");
    }

    #[test]
    fn open_status_counts_blocked_plugins_and_missing_audio() {
        use serde_json::json;
        let p = "C:\\s.ondina";
        assert_eq!(open_status(p, &json!({"opened": p, "missing_audio_files": []})), "Opened C:\\s.ondina");
        assert_eq!(
            open_status(p, &json!({"missing_audio_files": [], "blocked_plugins": [{"plugin_id": 1}]})),
            "Opened C:\\s.ondina · 1 VST3 plug-in not loaded (path refused)"
        );
        assert_eq!(
            open_status(p, &json!({"missing_audio_files": ["a", "b", "c"], "blocked_plugins": [{}, {}]})),
            "Opened C:\\s.ondina · 2 VST3 plug-ins not loaded (path refused) · 3 audio files missing"
        );
        assert_eq!(open_status(p, &json!({"missing_audio_files": ["a"]})), "Opened C:\\s.ondina · 1 audio file missing");
    }
}
