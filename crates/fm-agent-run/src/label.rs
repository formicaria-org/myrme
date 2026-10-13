//! A packet's nutrition label, read off a photograph (`decisions.md#agent`, 2026-10-13, *a label
//! is read from a photograph, and the person checks it*).
//!
//! **The model transcribes; it decides no number.** What comes back is the table as text. The
//! figures are read out of that text by code the page carries (`ui/src/lib/food/labelText.ts`),
//! put into the label form, and saved only when a person has looked at them beside the picture.
//!
//! Not a turn in a discussion and not a proposal: nothing is written anywhere. The picture
//! arrives by value, goes to the reader over loopback, and is kept nowhere.

use crate::ImageReader;
use fm_agent::imagetext::ReadImage;
use fm_agent::openai::OpenAiStep;
use std::time::Duration;

/// What a reader made of the picture, and which model it was: the person is told who read it.
pub struct Reading {
    pub text: String,
    pub model: String,
}

/// Said when nothing on this device can read a picture. The remedy is one that can succeed
/// wherever the assistant runs at all; where it cannot run, Settings already says why.
pub const NO_READER: &str = "Nothing on this device is reading pictures right now. Turn the \
assistant on in Settings, with a model that can read pictures, wait until it is ready, and try \
again. Or type the label in.";

/// Said when the model that can see here was not found fit to read a label. Measured
/// 2026-10-13 on nine packets from Open Food Facts (`agents/bench/results.md`): the phone's
/// small model got two right and put wrong figures in most of the others, in a table that looked
/// like a reading. So it is not asked, and the person is told why.
pub fn not_fit(model: &str) -> String {
    format!(
        "The assistant's model on this device ({model}) misreads nutrition labels too often to be \
         asked: measured on nine packets, it got two right. Type the label in, or scan the barcode. \
         A computer running the larger model reads a label well."
    )
}

/// Whether a label can be read here, before anybody takes a picture: the model that would read
/// it, or why none will. The page asks when the food sheet opens, so a phone whose model is not
/// fit says so on the press and never opens the camera for nothing.
pub fn ready(reader: Option<&ImageReader>) -> Result<&str, String> {
    match reader {
        None => Err(NO_READER.to_string()),
        Some(r) if !r.labels => Err(not_fit(&r.model)),
        Some(r) => Ok(&r.model),
    }
}

/// [`ready`], as the page is told it.
pub fn ready_json(reader: Option<&ImageReader>) -> serde_json::Value {
    match ready(reader) {
        Ok(model) => serde_json::json!({ "ready": true, "model": model, "why": "" }),
        Err(why) => serde_json::json!({ "ready": false, "model": "", "why": why }),
    }
}

/// A table is a few hundred tokens. A reader that runs on past this is repeating itself, which
/// the small models do on a picture they cannot make out, and is stopped.
const MOST_TOKENS: u32 = 900;

/// The picture's type, told from its first bytes. The page sends a JPEG; anything else that is
/// not a picture is refused here and never reaches a model.
pub fn kind(image: &[u8]) -> Option<&'static str> {
    match image {
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        _ => None,
    }
}

/// Have `reader` transcribe the nutrition table in `image`.
///
/// Always under the label's own instruction. A reader's instruction from the catalogue is for a
/// page of writing; given a photograph of a packet, the small model it was written for answered
/// with a description of the packet.
pub fn read(reader: Option<&ImageReader>, image: &[u8]) -> Result<Reading, String> {
    ready(reader)?;
    let reader = reader.ok_or(NO_READER)?;
    let mime = kind(image).ok_or("That is not a picture this can read. Take it again.")?;
    let text = OpenAiStep::local(reader.port, &reader.model)
        .with_image_instruction(Some(fm_agent::prompts::LABEL_INSTRUCTION.to_string()))
        .with_max_tokens(MOST_TOKENS)
        .with_timeout(Duration::from_secs(240))
        .read_image(image, mime)
        .map_err(|e| {
            let e = e.to_string();
            if e.contains("cut off") {
                "The reader could not make the label out. Take the picture closer, upright and in \
                 good light, with the table filling it."
                    .to_string()
            } else {
                format!("The label could not be read: {e}")
            }
        })?;
    Ok(Reading { text, model: reader.model.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_picture_reaches_a_reader() {
        assert_eq!(kind(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]), Some("image/jpeg"));
        assert_eq!(kind(b"\x89PNG\r\n\x1a\n"), Some("image/png"));
        assert_eq!(kind(b"%PDF-1.7"), None);
        assert_eq!(kind(b""), None);
        // Refused before any request: the port below has nobody on it, and is never tried.
        let nobody = ImageReader { port: 1, model: "m".into(), prompt: None, labels: true };
        let said = read(Some(&nobody), b"not a picture").err().unwrap();
        assert!(said.contains("not a picture"), "{said}");
    }

    #[test]
    fn a_model_not_found_fit_is_not_asked() {
        // Refused before any request, with the model's name and what to do instead.
        let small = ImageReader { port: 1, model: "tiny".into(), prompt: None, labels: false };
        let said = read(Some(&small), &[0xFF, 0xD8, 0xFF, 0xE0]).err().unwrap();
        assert!(said.contains("(tiny)") && said.contains("Type the label in"), "{said}");
        // And the page hears the same before a picture is taken.
        assert_eq!(ready_json(Some(&small))["ready"], false);
        assert_eq!(ready_json(Some(&small))["why"], said);
        assert_eq!(ready_json(None)["why"], NO_READER);
        assert_eq!(read(None, &[0xFF, 0xD8, 0xFF]).err().unwrap(), NO_READER);
        let fit = ImageReader { labels: true, ..small };
        assert_eq!(ready_json(Some(&fit))["model"], "tiny");
    }
}
