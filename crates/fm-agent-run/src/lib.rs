//! Shared runner logic for the study agent: the fm-serve client and the "handle one discussion turn"
//! glue that both the chat REPL and the resident `@name` watcher use. All store I/O goes through a
//! running fm-serve (the single vault writer — no second FileStore), and retrieval is RAG over the
//! vault (FTS via fm-serve) plus optional web search through a local proxy.

/// In-app model downloader (resumable + checksum). Agent-only — behind the `download` feature so a
/// build that runs an already-provisioned model links no HTTPS/TLS stack.
#[cfg(feature = "download")]
pub mod fetch;
pub mod fmserve;
pub mod manifest;
/// Locate the Android native-library dir (where the bundled model runtime lives) in pure Rust.
pub mod nativelib;
pub mod watch;
/// In-process HTTPS web search for the phone (no local SearXNG proxy). Needs the TLS client the
/// `download` feature carries.
#[cfg(feature = "download")]
pub mod websearch;

use fm_agent::openai::OpenAiStep;
use fm_agent::search::{SearxngSearch, DEFAULT_ENGINES};
use fm_agent::{
    convo, AgentError, InputDoc, ResearchRequest, SearchHit, StudyAssistant, WebSearch,
};
use fmserve::{Origin, VaultAccess};

/// A configured agent bound to a vault (via any [`VaultAccess`]) + a warm model (+ optional
/// web-search proxy). Generic over the vault seam so the same runner works over HTTP on the desktop
/// and in-process on a phone.
pub struct Agent<V: VaultAccess> {
    pub fm: V,
    pub model_port: u16,
    /// The model/agent name — also what a user `@name`-mentions, and the git author of its proposals.
    pub model: String,
    /// The local web-search proxy port, or `None` to run without the web (desktop path).
    pub searxng_port: Option<u16>,
    /// Use the **in-process** HTTPS multi-source search ([`crate::websearch::DirectSearch`]) instead
    /// of a localhost proxy — the phone, which can run neither a Python proxy nor a local SearXNG.
    /// Ignored when `searxng_port` is set (an explicit proxy wins) or when the `download` feature that
    /// carries the HTTPS client is off.
    pub web_direct: bool,
    /// The local `whisper.cpp` server port for audio→transcript, or `None` where transcription is not
    /// available (mobile v1 — the phone audio path is a later spike). `None` ⇒ `/transcribe` degrades
    /// to a plain "not on this device" reply rather than failing.
    pub whisper_port: Option<u16>,
    /// The whisper model/weights label, used for transcript provenance (e.g. `"ggml-base.en"`).
    pub whisper_model: String,
    /// Whether the model server was started with a multimodal projector — i.e. whether it can
    /// actually see an image. `false` ⇒ `/describe` says so plainly instead of asking a text-only
    /// model to read a picture, which is the one failure mode that would produce a confident,
    /// fluent, entirely invented reading and file it in someone's notes.
    pub vision: bool,
    pub max_reply_chars: usize,
    /// How many related notes to retrieve as RAG context.
    pub retrieve: usize,
    /// History budget (chars). When the discussion overflows it, `history` pins both ends — the first
    /// turn (the original ask) and the latest — and fills the middle with the most recent turns that
    /// fit (plain truncation, not summarized). Cheap, predictable, and it never drops the standing
    /// constraints the opener carries.
    pub history_budget: usize,
}

impl<V: VaultAccess> Agent<V> {
    /// Handle one already-posted user message on `note`'s discussion end to end: gather context (host
    /// note + RAG + web when `/search`), truncate old history if it overflows, run the turn, and POST
    /// the agent's reply (and a proposal when `allow_propose` and `/propose`). Returns the reply text.
    /// The user's own message is assumed already in the discussion (the REPL or the app posted it).
    /// Returns the reply text and, when one was posted, the id of the reply message — so a caller
    /// watching the discussion can mark it seen and never answer the agent's own message.
    ///
    /// `on_stage` is called on entering each pipeline stage (`reading your notes`, `searching the
    /// web`, `thinking`) so a caller can surface *what the agent is doing* — because in an agent the
    /// slow part is often the tools (web search, retrieval), not the LLM. Pass `&|_| {}` to ignore it.
    pub fn handle(
        &self,
        note: &str,
        intent: &convo::Intent,
        allow_propose: bool,
        on_stage: &dyn Fn(&str),
    ) -> Result<(String, Option<String>), String> {
        // In a plain discussion there is nothing to propose an edit to, so propose/research are off
        // there (both land as a proposal to the host note).
        let intent = convo::Intent {
            propose: intent.propose && allow_propose,
            research: intent.research && allow_propose,
            // Transcribe, like propose/research, only lands as a proposal to the host note — so in
            // a plain discussion, which has no host note to propose against, it is off rather than
            // silently doing nothing.
            transcribe: intent.transcribe.clone().filter(|_| allow_propose),
            ..intent.clone()
        };

        // Grounded research is a distinct pipeline: its only output is a cited proposal to the host
        // note (insertion-only, human-merged), so it short-circuits the conversational turn.
        if intent.research {
            return self.research_turn(note, &intent, on_stage);
        }

        // Audio→transcript is likewise its own pipeline: read the selected audio blob's bytes, run the
        // whisper specialist, and propose the provenance-marked transcript into the host note.
        if let Some(reference) = intent.transcribe.clone() {
            return self.transcribe_turn(note, &reference, on_stage);
        }

        on_stage("reading the conversation");
        let history = self.history(note)?;
        // The note being discussed, plus the notes it explicitly **links** to (text only) — bounded,
        // relevant context the user chose by linking, not a vault-wide search (that fed a tiny model a
        // pile of unrelated fragments it parroted back). `/search` still adds the web.
        on_stage("reading this note");
        let mut context = self.host_note(note)?;
        context.extend(self.linked_notes(note)?);
        // Web search is best-effort: if the proxy is unreachable, don't fail the whole turn — answer
        // from notes/memory and *tell the user* the answer isn't web-grounded (a wrong answer that
        // looks researched is worse than a flagged one).
        let mut web_unavailable = false;
        if intent.search {
            on_stage("searching the web");
            match self.web(&intent.ask) {
                Ok(docs) => context.extend(docs),
                Err(e) => {
                    web_unavailable = true;
                    on_stage("web search unavailable");
                    eprintln!("web search unavailable: {e}");
                }
            }
        }

        on_stage("thinking");
        let agent = StudyAssistant::new(self.llm(), NoWeb);
        let turn = agent
            .turn(&history, &intent, &context, Some(self.max_reply_chars))
            .map_err(|e| e.to_string())?;

        on_stage("writing the reply");

        let reply = turn.reply.clone().unwrap_or_default();
        // A tiny model sometimes returns nothing usable, or the echo-stripper cleans it to empty.
        // Never post an empty message — the store rejects it (a reply "needs something in it"), which
        // would surface as a 500; say so plainly instead.
        let reply = if reply.trim().is_empty() {
            "(I couldn't get a usable answer from the model — try rephrasing, or ask something simpler.)"
                .to_string()
        } else if web_unavailable {
            format!(
                "_(Web search was unavailable — answering from your notes and the model's own \
                 knowledge, which may be unreliable.)_\n\n{reply}"
            )
        } else {
            reply
        };
        let email = format!("{}@fm-agents.local", self.model);
        let mut reply_id = None;
        if turn.reply.is_some() {
            // Attributed to the model, so the discussion labels who said it.
            let meta = self.fm.reply_as(note, &reply, &self.model, &email)?;
            reply_id = meta["id"].as_str().map(|s| s.to_string());
        }
        if let Some(body) = &turn.proposal {
            let origin = Origin {
                tool: "propose",
                query: Some(intent.ask.clone()),
                sources: Vec::new(),
            };
            self.fm
                .create_proposal(note, body, &self.model, &email, &origin)?;
        }
        Ok((reply, reply_id))
    }

    /// The grounded **research** pipeline as a live turn: search the configured sources, write a
    /// quote-first note, verify every claim's quote against its source (dropping the unsupported), and
    /// **append** the cited note to the host note as a proposal a human reviews and merges. Additive by
    /// design — it never rewrites away the note's existing content; the human sees the diff.
    fn research_turn(
        &self,
        note: &str,
        intent: &convo::Intent,
        on_stage: &dyn Fn(&str),
    ) -> Result<(String, Option<String>), String> {
        let email = format!("{}@fm-agents.local", self.model);
        // Grounded research needs the web; without a backend, say so plainly rather than guessing.
        let Some(web) = self.web_backend() else {
            let msg = "_(Grounded research needs web search, but it is off — enable it to use /research.)_".to_string();
            let meta = self.fm.reply_as(note, &msg, &self.model, &email)?;
            return Ok((msg, meta["id"].as_str().map(|s| s.to_string())));
        };

        on_stage("reading this note");
        // The host note and the notes it links to are offered as additional numbered sources, so the
        // model can ground in the user's own material too — not only the web.
        let mut inputs = self.host_note(note)?;
        inputs.extend(self.linked_notes(note)?);
        let req = ResearchRequest {
            host_note: note.to_string(),
            ask: intent.ask.clone(),
            inputs,
            search: Some(intent.ask.clone()),
        };

        on_stage("researching the web");
        let agent = StudyAssistant::new(self.llm(), web);
        let out = agent.research(&req).map_err(|e| e.to_string())?;

        on_stage("writing a grounded proposal");
        // Additive: append the cited note under the host note's existing body (the full body, not the
        // context-truncated copy). Preserves what's there; the proposal diff shows exactly what's added.
        let cur = self.fm.get(note)?;
        let existing = cur["body"].as_str().unwrap_or_default().trim().to_string();
        let body = if existing.is_empty() {
            out.draft.new_body.clone()
        } else {
            format!("{existing}\n\n{}", out.draft.new_body)
        };
        let origin = Origin {
            tool: "research",
            query: Some(intent.ask.clone()),
            sources: out.draft.sources.clone(),
        };
        self.fm
            .create_proposal(note, &body, &self.model, &email, &origin)?;

        let dropped = if out.grounded.dropped.is_empty() {
            String::new()
        } else {
            format!(
                " I dropped {} claim(s) the sources didn't support.",
                out.grounded.dropped.len()
            )
        };
        let reply = if out.grounded.verified.is_empty() {
            format!(
                "I researched \u{201c}{}\u{201d} but the sources found didn't support a grounded answer, \
                 so I proposed a note saying so.{dropped} Review it in Collaboration.",
                intent.ask.trim()
            )
        } else {
            format!(
                "I researched \u{201c}{}\u{201d} and proposed a grounded note — {} claim(s), each backed by \
                 a verbatim quote from {} source(s).{dropped} Review and merge it in Collaboration.",
                intent.ask.trim(),
                out.grounded.verified.len(),
                out.draft.sources.len(),
            )
        };
        let meta = self.fm.reply_as(note, &reply, &self.model, &email)?;
        Ok((reply, meta["id"].as_str().map(|s| s.to_string())))
    }

    /// `/transcribe` — turn this note's **media into text**: recordings into a transcript, and
    /// handwriting, boards and photographed pages into a digital one.
    ///
    /// **One verb, two specialists, chosen by what the blob is.** Transcribing a recording and
    /// transcribing a page are the same request; making a person know which command to type is
    /// making them do the dispatch. So a bare `/transcribe` does *everything* in the note that has
    /// not been done yet, and each blob routes by MIME to the specialist for it.
    ///
    /// Each capability is checked separately, because they fail separately: whisper is a second
    /// process that may not be staged, and vision is a projector that may not be fetched. A device
    /// with only one of them still does the half it can, and **says which half it skipped** rather
    /// than silently doing less than was asked.
    ///
    /// Insertion-only throughout: every reading lands as its own provenance-marked adjunct beside
    /// the source it came from, and the human sees the diff before anything is merged.
    fn transcribe_turn(
        &self,
        note: &str,
        reference: &str,
        on_stage: &dyn Fn(&str),
    ) -> Result<(String, Option<String>), String> {
        let email = format!("{}@fm-agents.local", self.model);
        let say = |msg: String| -> Result<(String, Option<String>), String> {
            let meta = self.fm.reply_as(note, &msg, &self.model, &email)?;
            Ok((msg, meta["id"].as_str().map(|s| s.to_string())))
        };

        on_stage("looking for what to transcribe");
        // The note's current, full body — both to find its embedded media and to append onto (not a
        // context-truncated copy), so the diff shows exactly what is added and a re-run supersedes
        // in place.
        let cur = self.fm.get(note)?;
        let existing = cur["body"].as_str().unwrap_or_default().to_string();
        let asked = reference.trim();

        let audio = match self.whisper_port {
            Some(_) => self.resolve_blobs(asked, &existing, "audio/", "fm:transcript")?,
            None => AudioWork::None,
        };
        let images = if self.vision {
            self.resolve_blobs(asked, &existing, "image/", "fm:image-text")?
        } else {
            AudioWork::None
        };
        let clips = match &audio {
            AudioWork::Clips(c) => c.clone(),
            _ => Vec::new(),
        };
        let pages = match &images {
            AudioWork::Clips(c) => c.clone(),
            _ => Vec::new(),
        };

        if clips.is_empty() && pages.is_empty() {
            // Nothing to do — but *why* is the useful part, and the answers are different.
            if matches!(audio, AudioWork::AllDone) || matches!(images, AudioWork::AllDone) {
                return say(
                    "_(Everything in this note is already transcribed — nothing new to do.)_"
                        .into(),
                );
            }
            // With NOTHING available, the missing capability is the whole answer. With one of the
            // two working, it is a footnote: "nothing here to transcribe" is what actually happened,
            // and leading with an unrelated missing projector would answer a question nobody asked.
            if self.whisper_port.is_none() && !self.vision {
                return say(
                    "_(Neither audio transcription nor image reading is available on this device, \
                     so there was nothing I could transcribe here.)_"
                        .into(),
                );
            }
            let aside = match (self.whisper_port.is_none(), !self.vision) {
                (true, _) => " (Audio transcription isn't available on this device.)",
                (_, true) => " (Image reading isn't available on this device — the vision projector isn't loaded.)",
                _ => "",
            };
            return if asked.is_empty() {
                say(format!(
                    "_(I don't see a recording or an image in this note to transcribe — attach one \
                     first.){aside}_"
                ))
            } else {
                say(format!(
                    "_(That doesn't look like something I can transcribe.){aside}_"
                ))
            };
        }

        let mut new_body = existing.clone();
        let mut sources: Vec<String> = Vec::new();

        if !clips.is_empty() {
            on_stage("transcribing the audio");
            let port = self.whisper_port.unwrap_or_default();
            let whisper =
                fm_agent::transcribe::WhisperServer::local(port, self.whisper_model.clone());
            for (reference, audio, mime) in &clips {
                let prov = fm_agent::adjunct::Provenance {
                    specialist: "whisper.cpp".into(),
                    model: whisper.model().to_string(),
                    blob_hash: fm_agent::adjunct::hash_of(reference),
                };
                new_body =
                    fm_agent::transcribe::transcribe_into(&whisper, &new_body, audio, mime, &prov)
                        .map_err(|e| e.to_string())?;
                sources.push(reference.to_string());
            }
        }

        if !pages.is_empty() {
            on_stage("reading the writing");
            let vision = fm_agent::openai::OpenAiStep::local(self.model_port, self.model.clone());
            for (reference, bytes, mime) in &pages {
                let prov = fm_agent::adjunct::Provenance {
                    specialist: "vision".into(),
                    model: self.model.clone(),
                    blob_hash: fm_agent::adjunct::hash_of(reference),
                };
                new_body = fm_agent::imagetext::read_into(&vision, &new_body, bytes, mime, &prov)
                    .map_err(|e| e.to_string())?;
                sources.push(reference.to_string());
            }
        }

        on_stage("proposing the transcript");
        let origin = Origin {
            tool: "transcribe",
            query: None,
            sources,
        };
        self.fm
            .create_proposal(note, &new_body, &self.model, &email, &origin)?;

        let what = match (clips.len(), pages.len()) {
            (a, 0) => format!("{a} recording{}", if a == 1 { "" } else { "s" }),
            (0, i) => format!("{i} image{}", if i == 1 { "" } else { "s" }),
            (a, i) => format!(
                "{a} recording{} and {i} image{}",
                if a == 1 { "" } else { "s" },
                if i == 1 { "" } else { "s" }
            ),
        };
        let caveat = if pages.is_empty() {
            ""
        } else {
            " A model reading handwriting can misread it, so the image stays beside the text — \
             check the equations."
        };
        say(format!(
            "I transcribed {what} and proposed the text as an addition to this note — review and \
             merge in Collaboration.{caveat}"
        ))
    }

    /// Resolve which audio to transcribe and read each clip's bytes ONCE (read-only, by value — the
    /// specialist never gets a path).
    ///
    /// An explicit `reference` returns exactly that clip (re-running is allowed — a redo supersedes its
    /// own block). A bare `/transcribe` returns EVERY audio clip the note embeds that is **not already
    /// transcribed**, in order, so one command does them all and never re-does or clobbers an accepted
    /// transcript. [`AudioWork::AllDone`] distinguishes "every clip is transcribed" from "no audio here"
    /// so the reply can say which.
    /// Which blobs of a given kind this note still needs read.
    ///
    /// Shared by both specialists and parameterised by `mime_prefix` and the specialist's fence
    /// `tag`, because the rule — an explicit reference, or else *every* embed of that kind that is
    /// not already done — is one rule. Two copies of it would drift, and the one that drifted would
    /// silently re-read work a human had already reviewed.
    fn resolve_blobs(
        &self,
        reference: &str,
        body: &str,
        mime_prefix: &str,
        tag: &str,
    ) -> Result<AudioWork, String> {
        if !reference.is_empty() {
            let (bytes, mime) = self.fm.blob_bytes(reference)?;
            return Ok(if mime.starts_with(mime_prefix) {
                AudioWork::Clips(vec![(reference.to_string(), bytes, mime)])
            } else {
                AudioWork::None
            });
        }
        let mut clips = Vec::new();
        let mut skipped_done = false;
        for r in asset_ref_candidates(body) {
            // A missing/unreadable embed just isn't audio — skip it, don't fail the turn.
            let Ok((bytes, mime)) = self.fm.blob_bytes(&r) else {
                continue;
            };
            if !mime.starts_with(mime_prefix) {
                continue;
            }
            if already_read(body, &r, tag) {
                skipped_done = true;
                continue;
            }
            clips.push((r, bytes, mime));
        }
        Ok(if !clips.is_empty() {
            AudioWork::Clips(clips)
        } else if skipped_done {
            AudioWork::AllDone
        } else {
            AudioWork::None
        })
    }

    fn llm(&self) -> OpenAiStep {
        OpenAiStep::local(self.model_port, &self.model)
    }

    /// The discussion so far as text, oldest first, within `history_budget`. When it overflows, the
    /// **first turn and the latest turn are both pinned** and the middle is filled with the most recent
    /// turns that fit — so the conversation never blows a tiny model's context *and* the original ask
    /// (the constraints every later round must still honour) is never the thing that drops. Plain,
    /// deterministic truncation — no summarize-model call (the owner cut that orchestration; a tiny
    /// model's summary of a chat is unreliable anyway).
    fn history(&self, note: &str) -> Result<String, String> {
        let view = self.fm.thread(note)?;
        let msgs: Vec<String> = view["messages"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["body"].as_str().map(|b| format!("- {}", b.trim())))
                    .collect()
            })
            .unwrap_or_default();
        let full = msgs.join("\n");
        if full.chars().count() <= self.history_budget || msgs.len() <= 1 {
            // Fits, or a lone over-long turn there is nothing to drop from — keep it whole (never
            // return empty, the old tail-only bound's edge case).
            return Ok(full);
        }
        // Over budget. Pin BOTH ends: the FIRST turn (the original ask + standing constraints) and the
        // LATEST turn (the current state), then fill the middle with the most recent turns that fit.
        // The old "keep the recent tail, drop the oldest" bound is what let a long refinement silently
        // lose an early requirement and re-break it (oscillation); pinning the opener fixes that.
        let last = msgs.len() - 1;
        let gap = "- […earlier turns omitted…]";
        // Reserve room for both ends and (if the kept turns aren't contiguous) the gap marker, so the
        // result stays within budget.
        let mut used =
            msgs[0].chars().count() + msgs[last].chars().count() + gap.chars().count() + 2;
        let mut middle: Vec<usize> = Vec::new();
        for i in (1..last).rev() {
            let cost = msgs[i].chars().count() + 1;
            if used + cost > self.history_budget {
                break;
            }
            used += cost;
            middle.push(i);
        }
        middle.reverse(); // back to chronological order
        let mut out = vec![msgs[0].clone()];
        // A gap marker only when a real hole was left between the opener and what follows it.
        let hole = match middle.first() {
            Some(&i) => i > 1,
            None => last > 1,
        };
        if hole {
            out.push(gap.to_string());
        }
        out.extend(middle.into_iter().map(|i| msgs[i].clone()));
        out.push(msgs[last].clone());
        Ok(out.join("\n"))
    }

    /// The host note itself — what the discussion is about, so always included.
    fn host_note(&self, note: &str) -> Result<Vec<InputDoc>, String> {
        let v = self.fm.get(note)?;
        let body = v["body"].as_str().unwrap_or_default();
        if body.is_empty() {
            return Ok(Vec::new());
        }
        let title = v["title"].as_str().unwrap_or("this note");
        Ok(vec![InputDoc {
            label: format!("{title} (this note)"),
            text: body.chars().take(1200).collect(),
        }])
    }

    /// The notes the host note **links to** (`[..](note:<id>)` in its body) — their *text only*, as
    /// context. Bounded (deduped, host excluded, capped) and deliberately shallow: only what the user
    /// chose to link, one hop, no vault-wide search — the safe half of RAG the owner asked for.
    fn linked_notes(&self, host: &str) -> Result<Vec<InputDoc>, String> {
        let v = self.fm.get(host)?;
        let body = v["body"].as_str().unwrap_or_default();
        let mut docs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for id in note_refs(body) {
            if id == host || !seen.insert(id.clone()) {
                continue;
            }
            let Ok(note) = self.fm.get(&id) else { continue };
            let text = note["body"].as_str().unwrap_or_default();
            if text.trim().is_empty() {
                continue;
            }
            let title = note["title"].as_str().unwrap_or("linked note");
            docs.push(InputDoc {
                label: format!("{title} (linked note)"),
                text: text.chars().take(1200).collect(),
            });
            if docs.len() >= 5 {
                break; // a note with many links must not flood a tiny model
            }
        }
        Ok(docs)
    }

    /// Web search (text-only), as context. No backend ⇒ no web.
    fn web(&self, ask: &str) -> Result<Vec<InputDoc>, String> {
        let Some(web) = self.web_backend() else {
            return Ok(Vec::new());
        };
        let hits = web.search(ask).map_err(|e| e.to_string())?;
        Ok(hits
            .into_iter()
            .map(|h| InputDoc {
                label: format!("web: {} ({})", h.title, h.url),
                text: h.text,
            })
            .collect())
    }

    /// The web-search backend for this agent: an explicit **local SearXNG proxy** (desktop) wins;
    /// otherwise the **in-process HTTPS** search when `web_direct` is set and the TLS client is
    /// compiled in (the phone); otherwise `None` — no web. Boxed so the two call sites don't go
    /// generic over the choice (see [`WebSearch for Box<dyn WebSearch>`](fm_agent::WebSearch)).
    fn web_backend(&self) -> Option<Box<dyn WebSearch>> {
        if let Some(port) = self.searxng_port {
            return Some(Box::new(
                SearxngSearch::local(port).with_engines(DEFAULT_ENGINES.iter().copied()),
            ));
        }
        if self.web_direct {
            #[cfg(feature = "download")]
            return Some(Box::new(crate::websearch::DirectSearch::new()));
        }
        None
    }
}

/// The note ids linked in a body — every `note:<ULID>` (from `[..](note:id)` or a bare ref). A ULID is
/// exactly 26 Crockford-base32 chars, so take the 26 after each `note:` and keep only alphanumeric
/// runs; a shorter/malformed match is skipped. Order-preserving; duplicates handled by the caller.
/// The outcome of scanning a note for audio to transcribe.
enum AudioWork {
    /// Un-transcribed clips to do, each `(reference, bytes, mime)`.
    Clips(Vec<(String, Vec<u8>, String)>),
    /// The note has audio, but every clip is already transcribed — nothing new to do.
    AllDone,
    /// No audio in the note (or an explicit reference that isn't audio).
    None,
}

/// Has this audio already been transcribed into `body`? True when a transcript block keyed to its hash
/// is present — i.e. a prior transcription was accepted and merged — so a bare `/transcribe` skips it
/// and moves to the next clip instead of re-doing or clobbering it.
fn already_read(body: &str, reference: &str, tag: &str) -> bool {
    let hash = fm_agent::adjunct::hash_of(reference);
    body.contains(&format!("{tag} key=\"{hash}|"))
}

/// The `sha256:<hash>` reference of every asset a note body embeds — both the body form
/// (`asset:sha256-<hex>`) and the frontmatter form (`sha256:<hex>`) — in order, deduped. Used to find
/// a note's own audio for a bare `/transcribe`, so the user never types a content hash.
fn asset_ref_candidates(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pat in ["asset:sha256-", "sha256:"] {
        for (i, _) in body.match_indices(pat) {
            let hex: String = body[i + pat.len()..]
                .chars()
                .take_while(char::is_ascii_hexdigit)
                .collect();
            // A sha-256 is 64 hex chars; be lenient but reject stray short runs.
            if hex.len() >= 8 && seen.insert(hex.clone()) {
                out.push(format!("sha256:{hex}"));
            }
        }
    }
    out
}

fn note_refs(body: &str) -> Vec<String> {
    body.match_indices("note:")
        .filter_map(|(i, _)| {
            let id: String = body[i + 5..].chars().take(26).collect();
            (id.len() == 26 && id.chars().all(|c| c.is_ascii_alphanumeric())).then_some(id)
        })
        .collect()
}

#[cfg(test)]
mod ref_tests {
    #[test]
    fn note_refs_extracts_linked_note_ids() {
        let body = "See [Bayes](note:01KY42HKAM9EZNFMGCS4A99V4C) and an embed \
                    ![x](note:01KY3ZBV913R3AA06FW7DBWQB0). A bare note:tooShort is ignored.";
        let ids = super::note_refs(body);
        assert_eq!(
            ids,
            vec![
                "01KY42HKAM9EZNFMGCS4A99V4C".to_string(),
                "01KY3ZBV913R3AA06FW7DBWQB0".to_string(),
            ]
        );
        assert!(super::note_refs("no links here").is_empty());
    }
}

/// `turn`/`summarize` never call web (they take pre-assembled context), so this stub satisfies the
/// generic; the runner does web search itself and folds it into the context.
struct NoWeb;
impl WebSearch for NoWeb {
    fn search(&self, _query: &str) -> Result<Vec<SearchHit>, AgentError> {
        Err(AgentError::new("unused"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// A `VaultAccess` with no HTTP and no fm-serve — proof the runner is decoupled from the desktop
    /// transport. The mobile in-process impl is just another one of these.
    #[derive(Default)]
    struct FakeVault {
        thread: Value,
        alive: bool,
    }
    impl VaultAccess for FakeVault {
        fn get(&self, _: &str) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn thread(&self, _: &str) -> Result<Value, String> {
            Ok(self.thread.clone())
        }
        fn discussions(&self) -> Result<Value, String> {
            Ok(json!([]))
        }
        fn alive(&self) -> bool {
            self.alive
        }
        fn search(&self, _: &str) -> Result<Value, String> {
            Ok(json!([]))
        }
        fn reply(&self, _: &str, _: &str) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn reply_as(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn create_proposal(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &Origin,
        ) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn blob_bytes(&self, _: &str) -> Result<(Vec<u8>, String), String> {
            Err("no blobs in the fake vault".into())
        }
        fn activity(&self, _: &str, _: &str, _: &str) {}
        fn activity_done(&self, _: &str) {}
        fn present(&self, _: &str) {}
    }

    fn agent(fm: FakeVault) -> Agent<FakeVault> {
        Agent {
            fm,
            model_port: 0,
            model: "test".into(),
            searxng_port: None,
            web_direct: false,
            whisper_port: None,
            whisper_model: "ggml-base.en".into(),
            vision: false,
            max_reply_chars: 600,
            retrieve: 3,
            history_budget: 4000,
        }
    }

    #[test]
    fn the_runner_reads_the_thread_through_the_vault_seam_not_fm_serve() {
        // No fm-serve, no socket — Agent runs against a plain in-memory VaultAccess, which is exactly
        // what makes the Android in-process impl a drop-in.
        let fm = FakeVault {
            thread: json!({ "messages": [{ "body": "hi" }, { "body": "there" }] }),
            alive: true,
        };
        let history = agent(fm).history("note").unwrap();
        assert!(
            history.contains("hi") && history.contains("there"),
            "got: {history:?}"
        );
    }

    // --- history() budgeting: the multi-round refinement fix (pin both ends, trim the middle). ---

    #[test]
    fn history_returns_the_whole_thread_when_it_fits_the_budget() {
        // The common case: under budget, nothing is dropped and the order is oldest-first.
        let thread =
            json!({ "messages": [{ "body": "one" }, { "body": "two" }, { "body": "three" }] });
        let h = agent(FakeVault {
            thread,
            alive: true,
        })
        .history("note")
        .unwrap();
        assert_eq!(h, "- one\n- two\n- three");
    }

    #[test]
    fn history_pins_the_original_ask_and_the_latest_turn_when_it_overflows() {
        // The refinement fix: over budget, the FIRST turn (the standing constraints an early round set)
        // and the LATEST turn (the current state) are both kept, a dropped middle is marked, and it is
        // a *middle* turn that gives way — not the opener. Without this, a long back-and-forth loses its
        // earliest requirement and the model re-breaks it (oscillation).
        let thread = json!({ "messages": [
            { "body": "FIRST: always keep the action items" },
            { "body": "second turn — some middle padding here" },
            { "body": "third turn — more middle padding here" },
            { "body": "LATEST: and fix the title" },
        ]});
        let mut a = agent(FakeVault {
            thread,
            alive: true,
        });
        a.history_budget = 120; // room for both ends + the gap marker, but not every middle turn
        let h = a.history("note").unwrap();
        assert!(
            h.contains("FIRST: always keep"),
            "the original ask is pinned, never dropped: {h:?}"
        );
        assert!(
            h.contains("LATEST: and fix"),
            "the most recent turn is always kept: {h:?}"
        );
        assert!(
            h.contains("omitted"),
            "a dropped middle is marked as a gap: {h:?}"
        );
        assert!(
            !h.contains("second turn"),
            "a middle turn is what gives way to fit the budget: {h:?}"
        );
        // The opener leads and the latest closes — order preserved.
        assert!(
            h.find("FIRST").unwrap() < h.find("LATEST").unwrap(),
            "chronological: {h:?}"
        );
    }

    #[test]
    fn history_keeps_both_short_turns_without_a_marker_or_duplication() {
        // Two turns are both ends, so even under an absurdly tight budget both survive, in order, with
        // no gap marker and no duplicated opener.
        let thread = json!({ "messages": [{ "body": "the ask" }, { "body": "the answer" }] });
        let mut a = agent(FakeVault {
            thread,
            alive: true,
        });
        a.history_budget = 5;
        let h = a.history("note").unwrap();
        assert_eq!(h, "- the ask\n- the answer");
    }

    #[test]
    fn history_keeps_a_lone_over_long_turn_rather_than_returning_nothing() {
        // One turn longer than the whole budget: there is nothing to drop, so it is kept whole — never
        // the empty string the old tail-only loop could return.
        let thread =
            json!({ "messages": [{ "body": "a single very long turn that exceeds the budget" }] });
        let mut a = agent(FakeVault {
            thread,
            alive: true,
        });
        a.history_budget = 5;
        let h = a.history("note").unwrap();
        assert!(
            h.contains("single very long turn"),
            "a lone turn is kept whole, not dropped to empty: {h:?}"
        );
    }

    #[test]
    fn serve_loop_stops_the_model_when_formicaria_is_gone() {
        // The "no orphaned agent" guarantee, checked with NO real model or network so it holds on both
        // the desktop and the phone: when the vault — fm-serve, i.e. formicaria — is gone (`alive()` is
        // false), the watch loop must trip `stop_model` and RETURN, rather than keep the model (and its
        // memory / GPU VRAM) loaded. This is the seam that failed when a restarted fm-serve was mistaken
        // for the original: the agent must go out after formicaria does.
        let agent = agent(FakeVault {
            alive: false,
            ..Default::default()
        });
        let stopped = std::cell::Cell::new(false);
        // `finished` stays false (the model itself is fine); only the missing vault should end the loop.
        crate::watch::serve_loop(
            &agent,
            "test",
            std::path::Path::new("/tmp"),
            1,
            &|| false,
            &|| stopped.set(true),
        );
        assert!(
            stopped.get(),
            "serve_loop must call stop_model when the vault (formicaria) is not alive"
        );
    }

    #[test]
    fn transcribe_reports_a_missing_runtime_and_a_missing_clip() {
        let intent = |src: &str| convo::Intent {
            ask: String::new(),
            search: false,
            propose: false,
            research: false,
            transcribe: Some(src.to_string()),
        };
        // No whisper runtime on this device → say so, propose nothing (regardless of any asset).
        let off = agent(FakeVault {
            alive: true,
            ..Default::default()
        }); // whisper_port None
        let (reply, _) = off.handle("note", &intent(""), true, &|_| {}).unwrap();
        // The wording now names *which* capability is missing, because `/transcribe` covers two
        // and a device may have either, both or neither.
        assert!(
            reply.contains("available on this device"),
            "no runtime: {reply}"
        );
        assert!(
            reply.contains("audio transcription"),
            "it must name the missing one: {reply}"
        );
        // Runtime on, but the note embeds no audio → asks to record/attach — and NEVER demands a hash.
        // (whisper is never contacted: resolution fails first on the empty note body.)
        let mut on = agent(FakeVault {
            alive: true,
            ..Default::default()
        });
        on.whisper_port = Some(1);
        let (reply, _) = on.handle("note", &intent(""), true, &|_| {}).unwrap();
        // `/transcribe` now covers recordings *and* writing, so it asks for either — and still
        // never demands a content hash.
        assert!(
            reply.contains("don't see a recording or an image"),
            "no media in note: {reply}"
        );
        assert!(
            !reply.contains("sha256"),
            "must never demand a hash: {reply}"
        );
    }

    #[test]
    fn transcribe_reads_the_blob_and_proposes_a_provenance_marked_transcript() {
        use std::cell::RefCell;
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        // A fake whisper-server returning a fixed transcript.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 8192];
            let _ = sock.read(&mut buf);
            let json = r#"{"text":"the recorded lecture words"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = sock.write_all(resp.as_bytes());
        });

        // A vault that hands back audio bytes and captures the proposed body.
        struct CapVault {
            proposed: RefCell<Option<String>>,
        }
        impl VaultAccess for CapVault {
            fn get(&self, _: &str) -> Result<Value, String> {
                // A regular note that embeds an audio clip — the bare-command resolver finds it by MIME.
                Ok(json!({ "body": "existing note body\n\n![clip](asset:sha256-abc123)\n" }))
            }
            fn blob_bytes(&self, _: &str) -> Result<(Vec<u8>, String), String> {
                Ok((b"RIFFaudiobytes".to_vec(), "audio/wav".into()))
            }
            fn create_proposal(
                &self,
                _: &str,
                body: &str,
                _: &str,
                _: &str,
                _: &Origin,
            ) -> Result<Value, String> {
                *self.proposed.borrow_mut() = Some(body.to_string());
                Ok(json!({ "id": "prop1" }))
            }
            fn reply_as(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Value, String> {
                Ok(json!({ "id": "r1" }))
            }
            fn thread(&self, _: &str) -> Result<Value, String> {
                Ok(Value::Null)
            }
            fn discussions(&self) -> Result<Value, String> {
                Ok(json!([]))
            }
            fn alive(&self) -> bool {
                true
            }
            fn search(&self, _: &str) -> Result<Value, String> {
                Ok(json!([]))
            }
            fn reply(&self, _: &str, _: &str) -> Result<Value, String> {
                Ok(Value::Null)
            }
            fn activity(&self, _: &str, _: &str, _: &str) {}
            fn activity_done(&self, _: &str) {}
            fn present(&self, _: &str) {}
        }

        let a = Agent {
            fm: CapVault {
                proposed: RefCell::new(None),
            },
            model_port: 0,
            model: "test".into(),
            searxng_port: None,
            web_direct: false,
            whisper_port: Some(port),
            whisper_model: "ggml-base.en".into(),
            vision: false,
            max_reply_chars: 600,
            retrieve: 3,
            history_budget: 4000,
        };
        let intent = convo::Intent {
            ask: String::new(),
            search: false,
            propose: false,
            research: false,
            transcribe: Some("asset:sha256-abc123".into()),
        };
        let (reply, _) = a.handle("note", &intent, true, &|_| {}).unwrap();
        assert!(reply.contains("transcribed"), "user is told, got: {reply}");
        let body =
            a.fm.proposed
                .borrow()
                .clone()
                .expect("a proposal was created");
        assert!(
            body.contains("existing note body"),
            "insertion-only: host body kept: {body}"
        );
        assert!(
            body.contains("the recorded lecture words"),
            "transcript inserted: {body}"
        );
        assert!(
            body.contains("asset:sha256-abc123"),
            "source referenced, not replaced: {body}"
        );
        assert!(body.contains("whisper.cpp"), "provenance present: {body}");
    }

    #[test]
    fn transcribe_does_the_untranscribed_clips_and_skips_the_accepted_one() {
        use std::cell::RefCell;
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        // A fake whisper-server that answers every request (two clips ⇒ two connections).
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for _ in 0..8 {
                let Ok((mut sock, _)) = listener.accept() else {
                    break;
                };
                let mut buf = vec![0u8; 8192];
                let _ = sock.read(&mut buf);
                let json = r#"{"text":"fresh clip words"}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    json.len(),
                    json
                );
                let _ = sock.write_all(resp.as_bytes());
            }
        });

        // A note with TWO audio clips: `aaaaaaaa` already transcribed (its block is in the body), and
        // `bbbbbbbb` not yet. One bare `/transcribe` must do only `bbbbbbbb` and leave A's block intact.
        struct TwoClips {
            proposed: RefCell<Option<String>>,
        }
        impl VaultAccess for TwoClips {
            fn get(&self, _: &str) -> Result<Value, String> {
                Ok(json!({ "body": "notes\n\n![a](asset:sha256-aaaaaaaa)\n\
                    <!-- fm:transcript key=\"aaaaaaaa|whisper.cpp|ggml-base.en\" -->\n\
                    > [!note] Transcript\n> OLD A TRANSCRIPT\n<!-- fm:transcript:end -->\n\n\
                    ![b](asset:sha256-bbbbbbbb)\n" }))
            }
            fn blob_bytes(&self, _: &str) -> Result<(Vec<u8>, String), String> {
                Ok((b"RIFFaudio".to_vec(), "audio/wav".into()))
            }
            fn create_proposal(
                &self,
                _: &str,
                body: &str,
                _: &str,
                _: &str,
                _: &Origin,
            ) -> Result<Value, String> {
                *self.proposed.borrow_mut() = Some(body.to_string());
                Ok(json!({ "id": "p" }))
            }
            fn reply_as(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Value, String> {
                Ok(json!({ "id": "r" }))
            }
            fn thread(&self, _: &str) -> Result<Value, String> {
                Ok(Value::Null)
            }
            fn discussions(&self) -> Result<Value, String> {
                Ok(json!([]))
            }
            fn alive(&self) -> bool {
                true
            }
            fn search(&self, _: &str) -> Result<Value, String> {
                Ok(json!([]))
            }
            fn reply(&self, _: &str, _: &str) -> Result<Value, String> {
                Ok(Value::Null)
            }
            fn activity(&self, _: &str, _: &str, _: &str) {}
            fn activity_done(&self, _: &str) {}
            fn present(&self, _: &str) {}
        }

        let a = Agent {
            fm: TwoClips {
                proposed: RefCell::new(None),
            },
            model_port: 0,
            model: "test".into(),
            searxng_port: None,
            web_direct: false,
            whisper_port: Some(port),
            whisper_model: "ggml-base.en".into(),
            vision: false,
            max_reply_chars: 600,
            retrieve: 3,
            history_budget: 4000,
        };
        let intent = convo::Intent {
            ask: String::new(),
            search: false,
            propose: false,
            research: false,
            transcribe: Some(String::new()), // bare /transcribe
        };
        let (reply, _) = a.handle("note", &intent, true, &|_| {}).unwrap();
        assert!(
            reply.contains("1 recording"),
            "only the one untranscribed clip: {reply}"
        );
        let body =
            a.fm.proposed
                .borrow()
                .clone()
                .expect("a proposal was created");
        assert!(
            body.contains("OLD A TRANSCRIPT"),
            "the accepted transcript is preserved: {body}"
        );
        assert!(
            body.contains("fresh clip words"),
            "the untranscribed clip is transcribed: {body}"
        );
        assert!(
            body.contains("asset:sha256-bbbbbbbb"),
            "the new block references clip B: {body}"
        );
        // Exactly two transcript blocks now: A's (kept) + B's (new).
        assert_eq!(
            body.matches("fm:transcript:end").count(),
            2,
            "expected A + B blocks: {body}"
        );
    }

    #[test]
    fn research_without_a_search_proxy_tells_the_user_and_proposes_nothing() {
        // /research needs the web; with no proxy configured, the turn must short-circuit to a plain
        // notice (no model call, no network, no proposal) — hermetically checkable.
        let a = agent(FakeVault {
            alive: true,
            ..Default::default()
        });
        let intent = convo::Intent {
            ask: "how do mRNA vaccines work".into(),
            search: false,
            propose: false,
            research: true,
            transcribe: None,
        };
        let (reply, _id) = a.handle("note", &intent, true, &|_| {}).unwrap();
        assert!(
            reply.contains("web search") && reply.contains("is off"),
            "should explain the web is off, got: {reply}"
        );
    }
    /// `/transcribe` on a device that can do neither modality says **which** it cannot do, and
    /// proposes nothing.
    ///
    /// This is the failure that must never be silent: a text-only model handed an image does not
    /// refuse — it produces a fluent, confident, entirely invented reading, and this pipeline would
    /// then file it in someone's notes as a provenance-marked block. The `create_proposal` below
    /// panics if that ever happens.
    #[test]
    fn transcribing_without_the_capability_says_which_one_is_missing() {
        struct Fm(std::cell::RefCell<Vec<String>>);
        impl VaultAccess for Fm {
            fn alive(&self) -> bool {
                true
            }
            fn search(&self, _: &str) -> Result<Value, String> {
                Ok(json!([]))
            }
            fn get(&self, _: &str) -> Result<Value, String> {
                Ok(json!({ "body": "![](asset:sha256-abc)" }))
            }
            fn blob_bytes(&self, _: &str) -> Result<(Vec<u8>, String), String> {
                Ok((vec![1, 2, 3], "image/png".into()))
            }
            fn reply(&self, _: &str, _: &str) -> Result<Value, String> {
                Ok(json!({ "id": "r" }))
            }
            fn reply_as(&self, _: &str, body: &str, _: &str, _: &str) -> Result<Value, String> {
                self.0.borrow_mut().push(body.to_string());
                Ok(json!({ "id": "r" }))
            }
            fn create_proposal(
                &self,
                _: &str,
                _: &str,
                _: &str,
                _: &str,
                _: &Origin,
            ) -> Result<Value, String> {
                panic!("a model that cannot read the media must never propose a transcript");
            }
            fn thread(&self, _: &str) -> Result<Value, String> {
                Ok(json!([]))
            }
            fn discussions(&self) -> Result<Value, String> {
                Ok(json!([]))
            }
            fn activity(&self, _: &str, _: &str, _: &str) {}
            fn activity_done(&self, _: &str) {}
            fn present(&self, _: &str) {}
        }

        let fm = Fm(std::cell::RefCell::new(Vec::new()));
        let agent = Agent {
            fm,
            model_port: 1,
            model: "text-only".into(),
            searxng_port: None,
            web_direct: false,
            whisper_port: None,
            whisper_model: String::new(),
            vision: false,
            max_reply_chars: 2000,
            retrieve: 0,
            history_budget: 4000,
        };
        let intent = convo::Intent {
            ask: String::new(),
            search: false,
            propose: false,
            research: false,
            transcribe: Some(String::new()),
        };
        let (reply, _) = agent.handle("n1", &intent, true, &|_| {}).unwrap();
        assert!(
            reply.contains("Neither audio transcription nor image reading"),
            "it must name what is missing, got: {reply}"
        );
    }
}
