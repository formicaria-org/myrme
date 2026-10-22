//! Which clock a meeting time in an email is on, and moving it onto the person's own — formicaria
//! `decisions.md` 2026-10-23, *a time written in another zone is moved onto yours*.
//!
//! An email said "Tuesday 27th at 10am Paris time"; the meeting went on a Singapore agenda at 10:00,
//! seven hours early (the owner's mail, 2026-10-23). So once [`crate::meetings`] has found a time,
//! the words **right after it** are read for the clock it is on:
//!
//! - a place, then "time": `Paris time`, `New York time`, `Italian time`, `ora di Parigi`,
//!   `ora italiana`, `heure de Paris`;
//! - an abbreviation: `CET`, `GMT`, `UTC`, `SGT`, `PT`, … (read as the place's clock, which is what
//!   a writer means by "EST" in July);
//! - an offset: `UTC+2`, `GMT-05:00`.
//!
//! Only right after the time — "at 10am (Paris time)", "15:00 CET" — because that is where a writer
//! puts it; a place anywhere else in a sentence ("I'm in Paris that week") says where someone is, not
//! which clock a time is on. **A time with no zone written stays as written**: the writer's clock
//! cannot be known, and guessing is how a meeting moves by hours.
//!
//! The conversion is the zone database's, on the meeting's own day, so a daylight-saving change
//! between the email and the meeting is counted: 10:00 in Paris is 16:00 in Singapore on 23 October
//! 2026 and 17:00 on the 27th.

use std::collections::HashMap;
use std::sync::OnceLock;

use jiff::tz::{Offset, TimeZone};
use time::{Date, Month, Time};

/// The clock a time was written on, as found in the email.
#[derive(Debug, Clone)]
pub struct Zone {
    /// How the box names it: `Paris time`, `CET`, `UTC+02:00`.
    pub label: String,
    tz: TimeZone,
}

/// The person's own clock — formicaria's *your zone* setting, or the computer's.
#[derive(Debug, Clone)]
pub struct Here(TimeZone);

impl Here {
    /// `Asia/Singapore`, `+08:00`, `-05:30`, `Z` or `UTC`. `None` for anything else: a wrong zone
    /// moves every meeting by hours, so nothing is guessed.
    pub fn parse(name: &str) -> Option<Here> {
        let n = name.trim();
        if n.is_empty() {
            return None;
        }
        if n.eq_ignore_ascii_case("z") || n.eq_ignore_ascii_case("utc") {
            return Some(Here(TimeZone::UTC));
        }
        if n.starts_with(['+', '-']) {
            return signed_offset(n).and_then(fixed).map(Here);
        }
        TimeZone::get(n).ok().map(Here)
    }

    /// The computer's own zone, when it can be read.
    pub fn system() -> Option<Here> {
        TimeZone::try_system().ok().map(Here)
    }

    /// The day it is now on this clock — the day a meeting "today" is judged by.
    pub fn today(&self) -> Date {
        let z = jiff::Timestamp::now().to_zoned(self.0.clone());
        civil_date(z.date()).unwrap_or_else(|| time::OffsetDateTime::now_utc().date())
    }
}

impl Zone {
    /// `day` at `at` on this clock, as `(day, time)` on `here`'s. `None` only for a date the zone
    /// database cannot place.
    pub fn convert(&self, day: Date, at: Time, here: &Here) -> Option<(Date, Time)> {
        let dt = jiff::civil::DateTime::new(
            i16::try_from(day.year()).ok()?,
            u8::from(day.month()) as i8,
            day.day() as i8,
            at.hour() as i8,
            at.minute() as i8,
            0,
            0,
        )
        .ok()?;
        let there = dt.to_zoned(self.tz.clone()).ok()?.with_time_zone(here.0.clone());
        let t = there.time();
        Some((civil_date(there.date())?, Time::from_hms(t.hour() as u8, t.minute() as u8, 0).ok()?))
    }
}

fn civil_date(d: jiff::civil::Date) -> Option<Date> {
    Date::from_calendar_date(d.year().into(), Month::try_from(d.month() as u8).ok()?, d.day() as u8)
        .ok()
}

fn fixed(seconds: i32) -> Option<TimeZone> {
    Offset::from_seconds(seconds).ok().map(TimeZone::fixed)
}

/// `+2`, `+02`, `+0530`, `+05:30`, `-5` → seconds east of UTC.
fn signed_offset(s: &str) -> Option<i32> {
    let (sign, rest) = match s.as_bytes().first()? {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => return None,
    };
    let (h, m) = match rest.split_once(':') {
        Some((h, m)) => (h, m),
        None if rest.len() == 4 => (&rest[..2], &rest[2..]),
        None => (rest, "0"),
    };
    if h.is_empty() || h.len() > 2 || !h.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let (h, m): (i32, i32) = (h.parse().ok()?, m.parse().ok()?);
    (h <= 14 && m < 60).then_some(sign * (h * 3600 + m * 60))
}

/// Abbreviations a writer puts after a time. `ET` is left out: in French and Latin it is "and"
/// ("10h et demie"), and these words are read lower-cased.
const ABBREVIATIONS: [(&str, &str); 37] = [
    ("utc", "UTC"),
    ("gmt", "UTC"),
    ("cet", "Europe/Paris"),
    ("cest", "Europe/Paris"),
    ("bst", "Europe/London"),
    ("wet", "Europe/Lisbon"),
    ("west", "Europe/Lisbon"),
    ("eet", "Europe/Athens"),
    ("eest", "Europe/Athens"),
    ("msk", "Europe/Moscow"),
    ("sgt", "Asia/Singapore"),
    ("myt", "Asia/Kuala_Lumpur"),
    ("wib", "Asia/Jakarta"),
    ("ict", "Asia/Bangkok"),
    ("pht", "Asia/Manila"),
    ("hkt", "Asia/Hong_Kong"),
    ("jst", "Asia/Tokyo"),
    ("kst", "Asia/Seoul"),
    ("ist", "Asia/Kolkata"),
    ("aest", "Australia/Sydney"),
    ("aedt", "Australia/Sydney"),
    ("awst", "Australia/Perth"),
    ("nzst", "Pacific/Auckland"),
    ("nzdt", "Pacific/Auckland"),
    ("est", "America/New_York"),
    ("edt", "America/New_York"),
    ("cst", "America/Chicago"),
    ("cdt", "America/Chicago"),
    ("ct", "America/Chicago"),
    ("mst", "America/Denver"),
    ("mdt", "America/Denver"),
    ("mt", "America/Denver"),
    ("pst", "America/Los_Angeles"),
    ("pdt", "America/Los_Angeles"),
    ("pt", "America/Los_Angeles"),
    ("brt", "America/Sao_Paulo"),
    ("hst", "Pacific/Honolulu"),
];

/// Words for a clock that are not a city in the zone database: countries, adjectives, the US
/// regions, and the Italian and French names of a few cities. Checked before the database.
const PLACES: [(&str, &str); 52] = [
    ("uk", "Europe/London"),
    ("british", "Europe/London"),
    ("england", "Europe/London"),
    ("france", "Europe/Paris"),
    ("french", "Europe/Paris"),
    ("italy", "Europe/Rome"),
    ("italian", "Europe/Rome"),
    ("milan", "Europe/Rome"),
    ("germany", "Europe/Berlin"),
    ("german", "Europe/Berlin"),
    ("munich", "Europe/Berlin"),
    ("spain", "Europe/Madrid"),
    ("spanish", "Europe/Madrid"),
    ("swiss", "Europe/Zurich"),
    ("switzerland", "Europe/Zurich"),
    ("geneva", "Europe/Zurich"),
    ("dutch", "Europe/Amsterdam"),
    ("netherlands", "Europe/Amsterdam"),
    ("central european", "Europe/Paris"),
    ("japan", "Asia/Tokyo"),
    ("japanese", "Asia/Tokyo"),
    ("korea", "Asia/Seoul"),
    ("korean", "Asia/Seoul"),
    ("china", "Asia/Shanghai"),
    ("chinese", "Asia/Shanghai"),
    ("beijing", "Asia/Shanghai"),
    ("india", "Asia/Kolkata"),
    ("indian", "Asia/Kolkata"),
    ("delhi", "Asia/Kolkata"),
    ("mumbai", "Asia/Kolkata"),
    ("bangalore", "Asia/Kolkata"),
    ("eastern", "America/New_York"),
    ("boston", "America/New_York"),
    ("washington", "America/New_York"),
    ("central", "America/Chicago"),
    ("mountain", "America/Denver"),
    ("pacific", "America/Los_Angeles"),
    ("san francisco", "America/Los_Angeles"),
    ("seattle", "America/Los_Angeles"),
    // Italian: "ora di Parigi", "ora italiana".
    ("parigi", "Europe/Paris"),
    ("francese", "Europe/Paris"),
    ("londra", "Europe/London"),
    ("inglese", "Europe/London"),
    ("italiana", "Europe/Rome"),
    ("roma", "Europe/Rome"),
    ("milano", "Europe/Rome"),
    ("berlino", "Europe/Berlin"),
    ("tedesca", "Europe/Berlin"),
    ("giapponese", "Asia/Tokyo"),
    // French: "heure française", "heure de Londres".
    ("francaise", "Europe/Paris"),
    ("française", "Europe/Paris"),
    ("londres", "Europe/London"),
];

/// Every city the zone database names, lower-cased with spaces: `europe/paris` → `paris`,
/// `america/new_york` → `new york`. The first name for a city wins, in the database's sorted order.
fn cities() -> &'static HashMap<String, String> {
    static CITIES: OnceLock<HashMap<String, String>> = OnceLock::new();
    CITIES.get_or_init(|| {
        let mut names: Vec<String> =
            jiff::tz::db().available().map(|n| n.as_str().to_string()).collect();
        names.sort();
        let mut out = HashMap::new();
        for name in names {
            if !name.contains('/') || name.starts_with("Etc/") {
                continue;
            }
            let city = name.rsplit('/').next().unwrap_or_default().replace('_', " ").to_lowercase();
            out.entry(city).or_insert(name);
        }
        out
    })
}

fn place(words: &[String]) -> Option<&'static str> {
    let phrase = words.join(" ");
    if let Some((_, iana)) = PLACES.iter().find(|(w, _)| *w == phrase) {
        return Some(iana);
    }
    cities().get(&phrase).map(String::as_str)
}

/// The zone named by `iana`, labelled by its city: `Europe/Paris` → `Paris time`.
fn by_place(iana: &str) -> Option<Zone> {
    let tz = TimeZone::get(iana).ok()?;
    let city = iana.rsplit('/').next().unwrap_or(iana).replace('_', " ");
    Some(Zone { label: format!("{city} time"), tz })
}

/// `utc+2`, `gmt-05:00` as one word, or `utc` then `+2`.
fn offset_zone(words: &[String]) -> Option<Zone> {
    let w = words.first()?;
    let rest = w.strip_prefix("utc").or_else(|| w.strip_prefix("gmt"))?;
    let off = if rest.is_empty() {
        match words.get(1) {
            Some(n) if n.starts_with(['+', '-']) => n.as_str(),
            _ => return None,
        }
    } else {
        rest
    };
    let secs = signed_offset(off)?;
    let label = if secs == 0 {
        "UTC".to_string()
    } else {
        let (sign, a) = if secs < 0 { ('-', -secs) } else { ('+', secs) };
        format!("UTC{sign}{:02}:{:02}", a / 3600, a % 3600 / 60)
    };
    Some(Zone { label, tz: fixed(secs)? })
}

/// The clock named by the words **right after a time** (`words` starts just after it, lower-cased
/// and stripped of punctuation as [`crate::meetings`] tokenises), or `None` when they name none.
pub fn after_time(words: &[String]) -> Option<Zone> {
    if let Some(z) = offset_zone(words) {
        return Some(z);
    }
    let first = words.first()?;
    if let Some((w, iana)) = ABBREVIATIONS.iter().find(|(w, _)| w == first) {
        let tz = TimeZone::get(iana).ok()?;
        let label = if *iana == "UTC" { "UTC".to_string() } else { w.to_uppercase() };
        return Some(Zone { label, tz });
    }
    // "Paris time", "New York time", "Italian time zone": one to three words, then "time".
    for k in 1..=3 {
        if words.get(k).is_some_and(|w| w == "time" || w == "timezone") {
            if let Some(iana) = place(&words[..k]) {
                return by_place(iana);
            }
        }
    }
    // "ora di Parigi", "ora italiana", "heure de Paris", "heure française".
    if matches!(first.as_str(), "ora" | "heure") {
        let from =
            if words.get(1).is_some_and(|w| matches!(w.as_str(), "di" | "de")) { 2 } else { 1 };
        for k in from + 1..=from + 2 {
            if let Some(iana) = words.get(from..k).and_then(place) {
                return by_place(iana);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, time};

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    fn sg() -> Here {
        Here::parse("Asia/Singapore").unwrap()
    }

    #[test]
    fn a_place_and_time_is_read_and_converted_on_the_meetings_own_day() {
        let z = after_time(&words("paris time works for me")).unwrap();
        assert_eq!(z.label, "Paris time");
        // Paris leaves summer time on 25 October 2026: +2 before, +1 after.
        assert_eq!(
            z.convert(date!(2026 - 10 - 23), time!(10:00), &sg()),
            Some((date!(2026 - 10 - 23), time!(16:00)))
        );
        assert_eq!(
            z.convert(date!(2026 - 10 - 27), time!(10:00), &sg()),
            Some((date!(2026 - 10 - 27), time!(17:00)))
        );
    }

    #[test]
    fn a_conversion_may_move_the_day() {
        let z = after_time(&words("new york time")).unwrap();
        assert_eq!(z.label, "New York time");
        assert_eq!(
            z.convert(date!(2026 - 11 - 02), time!(20:00), &sg()),
            Some((date!(2026 - 11 - 03), time!(09:00)))
        );
    }

    #[test]
    fn abbreviations_offsets_and_other_languages() {
        assert_eq!(after_time(&words("cet")).unwrap().label, "CET");
        assert_eq!(after_time(&words("gmt")).unwrap().label, "UTC");
        let z = after_time(&words("utc+2")).unwrap();
        assert_eq!(z.label, "UTC+02:00");
        assert_eq!(
            z.convert(date!(2026 - 10 - 27), time!(10:00), &sg()),
            Some((date!(2026 - 10 - 27), time!(16:00)))
        );
        assert_eq!(after_time(&words("gmt -5")).unwrap().label, "UTC-05:00");
        assert_eq!(after_time(&words("ora italiana")).unwrap().label, "Rome time");
        assert_eq!(after_time(&words("ora di parigi")).unwrap().label, "Paris time");
        assert_eq!(after_time(&words("heure de paris")).unwrap().label, "Paris time");
        assert_eq!(after_time(&words("singapore time")).unwrap().label, "Singapore time");
        assert_eq!(after_time(&words("pacific time")).unwrap().label, "Los Angeles time");
    }

    #[test]
    fn words_that_name_no_clock_are_not_read_as_one() {
        for s in ["your time", "my time", "local time", "same time", "works for me", "et demie", ""]
        {
            assert!(after_time(&words(s)).is_none(), "{s:?}");
        }
    }

    #[test]
    fn your_zone_is_a_name_or_an_offset_and_nothing_else() {
        let here = Here::parse("+08:00").unwrap();
        let z = after_time(&words("utc")).unwrap();
        assert_eq!(
            z.convert(date!(2026 - 10 - 27), time!(9:00), &here),
            Some((date!(2026 - 10 - 27), time!(17:00)))
        );
        assert!(Here::parse("Europe/Paris").is_some());
        assert!(Here::parse("Z").is_some());
        assert!(Here::parse("Paris").is_none());
        assert!(Here::parse("+25:00").is_none());
        assert!(Here::parse("").is_none());
    }
}
