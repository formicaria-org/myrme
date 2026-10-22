//! A packet's nutrition label, read off a photograph (`decisions.md#agent`, 2026-10-13, *a label
//! is read from a photograph, and the person checks it*).
//!
//! **The model transcribes; it decides no number.** What comes back is the table as text. The
//! figures are read out of that text by code the page carries (`ui/src/lib/food/labelText.ts`),
//! put into the label form, and saved only when a person has looked at them beside the picture.
//!
//! Not a turn in a discussion and not a proposal: nothing is written anywhere. The picture
//! arrives by value, goes to the reader over loopback, and is kept nowhere.
//!
//! **The OCR reads it first** (Myrme 0.8.0, contract 1.7; `docs/research/2026-10-20-packet-labels.md`).
//! On thirty packets, the OCR's text read by the page's `labelText.ts` got 148 of 210 figures
//! right and 13 wrong in half a second, against 85–88 right for either vision model in 28–40 s.
//! So [`read_with`] has the OCR read the label — turned, when upright it holds too few figures —
//! and a vision model fit for labels reads it only when the OCR found too little. That is also
//! what lets a phone read a label at all: its own vision model was never fit to.

use crate::ocr::{Page, ReadPage};
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

/// **How many figures make a reading of a label**: a figure is a number with a unit after it
/// (`30.9 g`, `539kcal`), or one with decimals (`0,107`: many tables print the unit once, in its
/// own column, "grassi (g) 30,9"), and never a percentage. Seven lines carry one each; four is most
/// of a table, and a picture of something else (a list of ingredients, a barcode) has fewer.
pub const ENOUGH: usize = 4;

/// The figures in `text`: numbers followed, perhaps after a space, by g, mg, µg, kcal, kJ or ml,
/// and numbers with decimals, unless a % follows.
pub fn figures(text: &str) -> usize {
    let t = text.to_lowercase();
    let b = t.as_bytes();
    let mut n = 0;
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() || (i > 0 && (b[i - 1].is_ascii_alphanumeric())) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.' || b[j] == b',') {
            j += 1;
        }
        let mut k = j;
        if k < b.len() && b[k] == b' ' {
            k += 1;
        }
        let rest = &t[k..];
        let unit =
            ["kcal", "kj", "mg", "ml", "µg", "mcg", "g"].iter().find(|u| rest.starts_with(**u));
        let with_unit = unit
            .is_some_and(|u| !t[k + u.len()..].chars().next().is_some_and(|c| c.is_alphanumeric()));
        let num = &t[i..j];
        let decimal = num.trim_end_matches(['.', ',']).contains(['.', ','])
            && !rest.starts_with('%')
            && !t[j..].starts_with('%');
        if with_unit || decimal {
            n += 1;
        }
        i = j;
    }
    n
}

/// Whether a label can be read here (contract 1.7): by the OCR, which every device that has the
/// assistant's tools has, or else a vision model fit for labels. Named by what would read it.
pub fn ready_with<'a>(
    ocr: Option<&'a dyn ReadPage>,
    reader: Option<&'a ImageReader>,
) -> Result<&'a str, String> {
    match ocr {
        Some(o) => Ok(o.model()),
        None => ready(reader),
    }
}

/// [`ready_with`], as the page is told it.
pub fn ready_json_with(
    ocr: Option<&dyn ReadPage>,
    reader: Option<&ImageReader>,
) -> serde_json::Value {
    match ready_with(ocr, reader) {
        Ok(model) => serde_json::json!({ "ready": true, "model": model, "why": "" }),
        Err(why) => serde_json::json!({ "ready": false, "model": "", "why": why }),
    }
}

/// Said when the OCR found no label in the picture and nothing else here can read one.
pub const NOT_MADE_OUT: &str = "The label could not be made out. Take the picture closer, with \
the table of figures filling it and in good light, or type the label in.";

/// **Read a label, the OCR first** (contract 1.7). The OCR reads the picture upright and, when that
/// holds fewer than [`ENOUGH`] figures, turned a quarter, a half and three quarters; the reading
/// with the most figures is kept. With enough, it is the answer. With too few, a vision model fit
/// for labels reads the picture instead, where there is one; where there is none, what the OCR
/// read is still given, if it holds any figure at all, for the person to check and complete.
pub fn read_with(
    ocr: Option<&dyn ReadPage>,
    reader: Option<&ImageReader>,
    image: &[u8],
) -> Result<Reading, String> {
    kind(image).ok_or("That is not a picture this can read. Take it again.")?;
    let Some(o) = ocr else { return read(reader, image) };
    let mut best: Option<Page> = o.read_page(image).ok();
    let count = |p: &Option<Page>| p.as_ref().map_or(0, |p| figures(&p.text));
    for quarter in [90, 180, 270] {
        if count(&best) >= ENOUGH {
            break;
        }
        if let Ok(p) = o.read_turned(image, quarter) {
            if figures(&p.text) > count(&best) {
                best = Some(p);
            }
        }
    }
    if count(&best) >= ENOUGH || !reader.is_some_and(|r| r.labels) {
        return match best {
            Some(p) if figures(&p.text) > 0 => {
                Ok(Reading { text: p.text, model: o.model().into() })
            }
            _ => Err(NOT_MADE_OUT.into()),
        };
    }
    read(reader, image)
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

    /// A stand-in OCR: what it reads upright, and turned by each quarter.
    struct Fake {
        upright: &'static str,
        turned: [(u16, &'static str); 3],
        asked: std::sync::Mutex<Vec<u16>>,
    }
    impl ReadPage for Fake {
        fn read_page(&self, _: &[u8]) -> Result<Page, String> {
            Ok(Page { text: self.upright.into(), confidence: 0.9 })
        }
        fn model(&self) -> &str {
            "PP-OCRv6 small"
        }
        fn read_turned(&self, _: &[u8], quarter: u16) -> Result<Page, String> {
            self.asked.lock().unwrap().push(quarter);
            let text = self.turned.iter().find(|(q, _)| *q == quarter).unwrap().1;
            Ok(Page { text: text.into(), confidence: 0.9 })
        }
    }
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0];
    const TABLE: &str =
        "Energy 2252kJ 539kcal\nFat 30.9 g\nCarbohydrate 57,5g\nProtein 6.3 g\nSalt 0.107g";

    #[test]
    fn figures_are_numbers_with_a_unit() {
        assert_eq!(figures(TABLE), 6);
        assert_eq!(figures("Ingredients: sugar, palm oil 20%, hazelnuts 13%, cocoa 7.4%"), 0);
        // A table that prints its unit once, in a column of its own.
        assert_eq!(figures("grassi g 30,9 4,6\nsale g 0,107 0,016"), 4);
        // A unit glued to a word is not a figure's ("100grams" is), nor a number inside a word.
        assert_eq!(figures("E150d 3gx 12 g"), 1);
    }

    #[test]
    fn the_ocr_reads_a_label_upright_and_is_not_turned_when_that_is_enough() {
        let o = Fake {
            upright: TABLE,
            turned: [(90, ""), (180, ""), (270, "")],
            asked: Default::default(),
        };
        let got = read_with(Some(&o), None, JPEG).unwrap();
        assert_eq!((got.text.as_str(), got.model.as_str()), (TABLE, "PP-OCRv6 small"));
        assert!(o.asked.lock().unwrap().is_empty(), "not turned");
    }

    #[test]
    fn a_label_photographed_on_its_side_is_read_turned() {
        let o = Fake {
            upright: "rt 9 ll",
            turned: [(90, TABLE), (180, "x"), (270, "y")],
            asked: Default::default(),
        };
        assert_eq!(read_with(Some(&o), None, JPEG).unwrap().text, TABLE);
        assert_eq!(*o.asked.lock().unwrap(), vec![90], "stops at the first turn that is enough");
    }

    #[test]
    fn too_little_goes_to_a_fit_vision_model_and_otherwise_is_given_as_it_is() {
        let few = "Fat 3 g\nsome words";
        let o = Fake {
            upright: few,
            turned: [(90, ""), (180, ""), (270, "")],
            asked: Default::default(),
        };
        // No vision model fit for labels: what was read is given, for the person to complete.
        assert_eq!(read_with(Some(&o), None, JPEG).unwrap().text, few);
        // A fit one is asked instead (nobody is on port 1, so asking it is what fails here).
        let fit = ImageReader { port: 1, model: "vl".into(), prompt: None, labels: true };
        let said = read_with(Some(&o), Some(&fit), JPEG).err().unwrap();
        assert!(said.contains("could not be read"), "{said}");
        // Nothing at all: said plainly.
        let none = Fake {
            upright: "",
            turned: [(90, ""), (180, ""), (270, "")],
            asked: Default::default(),
        };
        assert_eq!(read_with(Some(&none), None, JPEG).err().unwrap(), NOT_MADE_OUT);
    }

    #[test]
    fn with_the_ocr_a_phone_can_read_labels() {
        let o = Fake {
            upright: "",
            turned: [(90, ""), (180, ""), (270, "")],
            asked: Default::default(),
        };
        let small = ImageReader { port: 1, model: "tiny".into(), prompt: None, labels: false };
        assert_eq!(ready_json_with(Some(&o), Some(&small))["model"], "PP-OCRv6 small");
        assert_eq!(ready_json_with(None, Some(&small))["ready"], false);
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
