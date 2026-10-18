//! Every sentence the assistant itself says in a discussion, in one file.
//!
//! These are the fixed replies — what it posts when it has done something, or cannot — as opposed to
//! what a model wrote. They were inline in the turn code, so reviewing the assistant's wording meant
//! reading its control flow. (The one fixed reply the orchestrator owns, `fm_agent::PROPOSAL_ACK`,
//! stays beside the call that needs it.)
//!
//! A reply in `_( … )_` is an aside about the assistant itself, shown in italics.

/// The model answered with nothing usable. An empty message cannot be posted, so say so.
pub const NO_USABLE_ANSWER: &str =
    "(I couldn't get a usable answer from the model — try rephrasing, or ask something simpler.)";

/// `/search` was asked but the web could not be reached: the answer still goes out, flagged.
pub fn web_unavailable(reply: &str) -> String {
    format!(
        "_(Web search was unavailable — answering from your notes and the model's own \
         knowledge, which may be unreliable.)_\n\n{reply}"
    )
}

/// `/propose`: none of the model's edits could be placed — each pointed at words not in the note, or
/// at words that occur more than once. Nothing is proposed rather than a guess.
pub const NO_EDIT_PLACED: &str =
    "I couldn't find where in the note to make that change, so I haven't \
proposed anything — try naming the words to change.";

/// `/propose`: the proposal was made, but some edits could not be placed and were left out.
pub fn edits_dropped(dropped: usize) -> String {
    let what = if dropped == 1 { "one change" } else { "some changes" };
    format!(
        "{} _({what} I couldn't place in the note — {dropped} — {} left out.)_",
        fm_agent::PROPOSAL_ACK,
        if dropped == 1 { "is" } else { "are" }
    )
}

pub const RESEARCH_NEEDS_WEB: &str =
    "_(Grounded research needs web search, but it is off — enable it to use /research.)_";

/// What `/research` did: how many claims survived the quote check, from how many sources, and how
/// many were dropped.
pub fn researched(ask: &str, verified: usize, sources: usize, dropped: usize) -> String {
    let dropped = if dropped == 0 {
        String::new()
    } else {
        format!(" I dropped {dropped} claim(s) the sources didn't support.")
    };
    let ask = ask.trim();
    if verified == 0 {
        format!(
            "I researched \u{201c}{ask}\u{201d} but the sources found didn't support a grounded answer, \
             so I proposed a note saying so.{dropped} Review it in Collaboration."
        )
    } else {
        format!(
            "I researched \u{201c}{ask}\u{201d} and proposed a grounded note — {verified} claim(s), each backed by \
             a verbatim quote from {sources} source(s).{dropped} Review and merge it in Collaboration."
        )
    }
}

pub const ALL_TRANSCRIBED: &str =
    "_(Everything in this note is already transcribed — nothing new to do.)_";

/// Neither reader exists on this device, so the missing capability is the whole answer.
pub const NOTHING_CAN_TRANSCRIBE: &str =
    "_(Neither audio transcription nor image reading is available on this device, \
     so there was nothing I could transcribe here.)_";

/// `/transcribe` found nothing it could work on. With one reader missing, that is a footnote, not
/// the headline: "nothing here to transcribe" is what actually happened.
pub fn nothing_to_transcribe(named_a_file: bool, no_audio: bool, no_vision: bool) -> String {
    let aside = match (no_audio, no_vision) {
        (true, _) => " (Audio transcription isn't available on this device.)",
        (_, true) => {
            " (Image reading isn't available on this device — the vision projector isn't loaded.)"
        }
        _ => "",
    };
    if named_a_file {
        format!("_(That doesn't look like something I can transcribe.){aside}_")
    } else {
        format!(
            "_(I don't see a recording or an image in this note to transcribe — attach one \
             first.){aside}_"
        )
    }
}

/// What `/transcribe` did. Reading handwriting carries a caveat that a recording does not.
pub fn transcribed(recordings: usize, images: usize) -> String {
    let s = |n: usize| if n == 1 { "" } else { "s" };
    let what = match (recordings, images) {
        (a, 0) => format!("{a} recording{}", s(a)),
        (0, i) => format!("{i} image{}", s(i)),
        (a, i) => format!("{a} recording{} and {i} image{}", s(a), s(i)),
    };
    let caveat = if images == 0 {
        ""
    } else {
        " A model reading handwriting can misread it, so the image stays beside the text — \
         check the equations."
    };
    format!(
        "I transcribed {what} and proposed the text as an addition to this note — review and \
         merge in Collaboration.{caveat}"
    )
}

/// `/recipe` on a note with a recording, where this device has nothing to hear it with.
pub const RECIPE_CANNOT_HEAR: &str = "I can't listen to recordings on this device — turn on audio \
    transcription in Settings → Study assistant, or ask from the device that can.";
/// `/recipe` on a note with too little in it to be a recipe.
pub const RECIPE_NOTHING_TO_READ: &str = "There isn't a recipe in this note for me to work from — \
    record it, or write it, and ask again.";
/// The words did not make a recipe.
pub const RECIPE_NOT_FOUND: &str =
    "I couldn't find a recipe in what this note says, so I proposed \
    nothing. Say the ingredients and the steps, and ask again.";

/// A recipe was proposed: what it is called, what it was made from, and anything left out.
/// `unheard`: recordings in the note that were **not** listened to, because transcription is off
/// here — said, so a recipe made from the text alone is never mistaken for one made from the voice.
pub fn recipe_proposed(title: &str, recordings: usize, dropped: usize, unheard: usize) -> String {
    let from = match recordings {
        0 => "from what this note says".to_string(),
        1 => "from the recording".to_string(),
        n => format!("from the {n} recordings"),
    };
    let left = match dropped {
        0 => String::new(),
        1 => " I left out one line that wasn't in what was said.".to_string(),
        n => format!(" I left out {n} lines that weren't in what was said."),
    };
    let deaf = match unheard {
        0 => String::new(),
        1 => " I didn't listen to the recording: audio transcription is off on this device \
              (Settings → Study assistant)."
            .to_string(),
        n => format!(
            " I didn't listen to the {n} recordings: audio transcription is off on this device \
             (Settings → Study assistant)."
        ),
    };
    format!(
        "I've proposed the recipe “{title}”, {from}. Accept it in Collaboration, or change it first; \
         then in Food its ingredients can be matched to foods so it counts.{left}{deaf}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recipe_from_text_says_which_recordings_it_did_not_hear() {
        let r = recipe_proposed("Tiramisù", 0, 0, 1);
        assert!(r.contains("didn't listen to the recording"), "{r}");
        assert!(!recipe_proposed("Tiramisù", 1, 0, 0).contains("didn't listen"));
    }
}
