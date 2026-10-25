//! The methods — Myrme's dedicated tasks, as one closed list (`docs/context/plan.md`, Part 2).
//!
//! A method is one task end to end: what starts it, what it needs from the device, and where its
//! description and golden cases live (`methods/<name>/`). The list is an `enum` on purpose: adding a
//! method is a new variant the compiler makes every `match` handle, and a `decisions.md` entry — the
//! same discipline as [`crate::convo::Command`], so there is still no registry.
//!
//! This module describes; it does not run anything. The pipelines stay where they are (the meeting
//! pass, [`crate::StudyAssistant`], the readers) and [`Method::of`] says which method a discussion
//! command is.

use crate::convo::Command;

/// One dedicated task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    /// M1 — meetings from mail.
    Meetings,
    /// M2 — grounded research, proposed as a cited note.
    Research,
    /// M3 — images to text.
    ImageText,
    /// M4 — speech to text, and dictation.
    AudioText,
    /// M5 — a proposed edit to the host note.
    Edits,
    /// M7 — a recipe from a recording or a note.
    Recipe,
    /// A reply in the discussion. Not a dedicated task, and the one method that may draw on the
    /// model's own knowledge; kept on the list so every command maps to something.
    Chat,
}

/// What starts a method. Never a model choosing it (`decisions.md`, inherited 2026-10-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// A command typed in a discussion (`@Myrme /research …`).
    Command,
    /// A file's type: audio goes to the speech reader, an image to the image reader.
    FileType,
    /// A schedule or an event: new mail wakes the meeting pass.
    Schedule,
}

/// A capability a method needs the device to have running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// A text model.
    Llm,
    /// A model that can see, or a dedicated image reader.
    Vision,
    /// A speech reader.
    Speech,
    /// The web, which only ever runs when the person asked for it.
    Web,
}

impl Method {
    pub const ALL: [Method; 7] = [
        Method::Meetings,
        Method::Research,
        Method::ImageText,
        Method::AudioText,
        Method::Edits,
        Method::Recipe,
        Method::Chat,
    ];

    /// The folder under `methods/` that describes it (`METHOD.md`) and holds its cases (`eval/`).
    pub fn name(self) -> &'static str {
        match self {
            Method::Meetings => "meetings",
            Method::Research => "research",
            Method::ImageText => "image-text",
            Method::AudioText => "audio-text",
            Method::Edits => "edits",
            Method::Recipe => "recipe",
            Method::Chat => "chat",
        }
    }

    pub fn trigger(self) -> Trigger {
        match self {
            Method::Meetings => Trigger::Schedule,
            Method::ImageText | Method::AudioText => Trigger::FileType,
            Method::Research | Method::Edits | Method::Recipe | Method::Chat => Trigger::Command,
        }
    }

    /// What must be running for it. `Research` always needs the web; `Chat` and `Edits` may add it
    /// when the message asks (`/search`), which [`Method::of`] does not record.
    pub fn needs(self) -> &'static [Need] {
        match self {
            Method::Meetings | Method::Edits | Method::Chat => &[Need::Llm],
            Method::Research => &[Need::Llm, Need::Web],
            Method::ImageText => &[Need::Vision],
            Method::AudioText => &[Need::Speech],
            Method::Recipe => &[Need::Llm, Need::Speech],
        }
    }

    /// The method a discussion command runs. `Read` is image or audio by each file's type, which only
    /// the runner knows, so it maps to neither here.
    pub fn of(command: &Command) -> Option<Method> {
        match command {
            Command::Chat => Some(Method::Chat),
            Command::Propose => Some(Method::Edits),
            Command::Research => Some(Method::Research),
            Command::Recipe => Some(Method::Recipe),
            Command::Read(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn methods_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../methods")
    }

    /// Every method is described, and every folder under `methods/` is a method: a folder nobody
    /// listed is a method the code does not know about.
    #[test]
    fn every_method_has_its_folder_and_every_folder_is_a_method() {
        for m in Method::ALL {
            let md = methods_dir().join(m.name()).join("METHOD.md");
            assert!(md.is_file(), "{} has no {}", m.name(), md.display());
            let text = std::fs::read_to_string(&md).unwrap();
            assert!(
                text.starts_with("---\nname: ") && text.contains("\ndescription: "),
                "{} is not in the Agent Skills format (name, description)",
                md.display()
            );
        }
        for entry in std::fs::read_dir(methods_dir()).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            assert!(
                Method::ALL.iter().any(|m| m.name() == name),
                "methods/{name} is not on Method::ALL"
            );
        }
    }

    #[test]
    fn every_command_but_reading_maps_to_one_method() {
        assert_eq!(Method::of(&Command::Propose), Some(Method::Edits));
        assert_eq!(Method::of(&Command::Research), Some(Method::Research));
        assert_eq!(Method::of(&Command::Read(String::new())), None);
    }
}
