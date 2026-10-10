//! How text is compared when a model **quotes** something: one place, two strengths.
//!
//! A model's quote is only worth anything if it is really in the source, so every check in this
//! crate is a substring test over text put into a common form first. There were two copies of that
//! form (one for research, one for meetings) with different rules; a variant one of them folded and
//! the other did not would have been a quote accepted in one place and refused in the other.
//!
//! - [`fold`] keeps the words exactly and only unifies what copying and extraction mangle: quote
//!   marks, dashes, odd spaces, runs of whitespace. Research uses it — a research quote must be
//!   verbatim.
//! - [`loose`] is `fold` plus lower case and accents off. Meetings use it, because the same text is
//!   then read for days and times, and "Giovedì" and "giovedi" are the same day.

/// Unify the punctuation and space variants that extraction introduces, drop soft hyphens, and make
/// every run of whitespace one space. **Case and letters are untouched.**
pub fn fold(s: &str) -> String {
    collapse(s.chars().filter_map(punct))
}

/// [`fold`], then lower case and the accents Italian and French put on vowels removed.
pub fn loose(s: &str) -> String {
    collapse(s.chars().flat_map(char::to_lowercase).map(unaccent).filter_map(punct))
}

fn punct(c: char) -> Option<char> {
    match c {
        '“' | '”' | '„' | '‟' | '«' | '»' => Some('"'),
        '‘' | '’' | '‚' | '‛' => Some('\''),
        '\u{00A0}' | '\u{2009}' | '\u{202F}' | '\u{2007}' => Some(' '), // nbsp / thin / narrow / figure
        '\u{00AD}' => None,                                             // soft hyphen
        '‐' | '‑' | '–' | '—' => Some('-'),
        other => Some(other),
    }
}

fn unaccent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' => 'a',
        'è' | 'é' | 'ê' => 'e',
        'ì' | 'í' | 'î' => 'i',
        'ò' | 'ó' | 'ô' => 'o',
        'ù' | 'ú' | 'û' => 'u',
        c => c,
    }
}

fn collapse(chars: impl Iterator<Item = char>) -> String {
    let s: String = chars.collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_unifies_marks_dashes_and_spaces_but_keeps_the_words() {
        assert_eq!(fold("“Già”\u{00A0}fatto —  ok"), "\"Già\" fatto - ok");
        assert_eq!(fold("co\u{00AD}operate"), "cooperate");
    }

    #[test]
    fn loose_is_fold_without_case_or_accents() {
        assert_eq!(loose("  Giovedì  alle «15» "), "giovedi alle \"15\"");
        // Whatever `fold` treats as the same, `loose` does too — the two can never disagree on
        // punctuation, which is the whole reason they live together.
        for s in ["a – b", "l’ora", "x\u{2009}y"] {
            assert_eq!(loose(s), fold(s).to_lowercase());
        }
    }
}
