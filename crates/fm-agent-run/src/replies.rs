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
