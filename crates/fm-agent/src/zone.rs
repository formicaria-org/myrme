//! Which clock a meeting time in an email is on, and moving it onto the person's own — formicaria
//! `decisions.md` 2026-10-23, *a time written in another zone is moved onto yours*.
//!
//! An email that fixed a meeting "at 10am <city> time" put it on an agenda in another zone at 10:00,
//! hours away from when it was (2026-10-23). So once [`crate::meetings`] has found a time,
//! the words **right after it** are read for the clock it is on:
//!
//! - a place, then "time": `Berlin time`, `New York time`, `Italian time`, `ora di Berlino`,
//!   `ora italiana`, `heure de Londres`;
//! - an abbreviation: `CET`, `GMT`, `UTC`, `SGT`, `PT`, … (read as the place's clock, which is what
//!   a writer means by "EST" in July);
//! - an offset: `UTC+2`, `GMT-05:00`.
//!
//! Only right after the time — "at 10am (Berlin time)", "15:00 CET" — because that is where a writer
//! puts it; a place anywhere else in a sentence ("I'm in Berlin that week") says where someone is, not
//! which clock a time is on. **A time with no zone written stays as written**: the writer's clock
//! cannot be known, and guessing is how a meeting moves by hours.
//!
//! The conversion is the zone database's, on the meeting's own day, so a daylight-saving change
//! between the email and the meeting is counted: 10:00 in Berlin is 17:00 in Tokyo on 23 October 2026
//! and 18:00 on the 27th.

use std::collections::HashMap;
use std::sync::OnceLock;

use jiff::tz::{Offset, TimeZone};
use time::{Date, Month, Time};

/// The clock a time was written on, as found in the email.
#[derive(Debug, Clone)]
pub struct Zone {
    /// How the box names it: `Berlin time`, `CET`, `UTC+02:00`.
    pub label: String,
    tz: TimeZone,
}

/// The person's own clock — **formicaria's app clock**, never this process's own zone (2026-10-24,
/// later, *Myrme follows formicaria's clock*).
#[derive(Debug, Clone)]
pub struct Here(TimeZone);

/// **The clock formicaria says the person is on** (its `clock` command, contract 1.8): the zone, the
/// day it is there, and whether formicaria takes times as instants (RFC 3339 `start`/`due`).
#[derive(Debug, Clone)]
pub struct Clock {
    pub here: Here,
    pub today: Date,
    pub instants: bool,
}

impl Clock {
    /// A clock on `here`, today on it.
    pub fn on(here: Here, instants: bool) -> Clock {
        let today = here.today();
        Clock { here, today, instants }
    }

    /// A clock with its day given — for a test, or a day formicaria named.
    pub fn at(here: Here, today: Date, instants: bool) -> Clock {
        Clock { here, today, instants }
    }

    /// formicaria's `clock` answer; `None` when it names no zone (it cannot read one) or one this
    /// does not parse. Its `today` is taken when it is a day, so both sides agree on the day.
    pub fn from_answer(v: &serde_json::Value) -> Option<Clock> {
        let here = Here::parse(v["zone"].as_str()?)?;
        let today = v["today"]
            .as_str()
            .and_then(|d| {
                time::Date::parse(d, &time::format_description::well_known::Iso8601::DATE).ok()
            })
            .unwrap_or_else(|| here.today());
        Some(Clock { here, today, instants: v["instants"].as_bool().unwrap_or(false) })
    }
}

impl Here {
    /// `Asia/Tokyo`, `+09:00`, `-05:30`, `Z` or `UTC`. `None` for anything else: a wrong zone
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

    /// The moment `day` at `at` is on this clock, Unix seconds (`jiff`'s compatible rule in a gap or
    /// a repeated hour).
    pub fn instant(&self, day: Date, at: Time) -> Option<i64> {
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
        Some(dt.to_zoned(self.0.clone()).ok()?.timestamp().as_second())
    }

    /// A moment (Unix seconds) as a day and a time on this clock.
    pub fn local(&self, unix: i64) -> Option<(Date, Time)> {
        let z = jiff::Timestamp::from_second(unix).ok()?.to_zoned(self.0.clone());
        let d = civil_date(z.date())?;
        Some((d, Time::from_hms(z.hour() as u8, z.minute() as u8, 0).ok()?))
    }

    /// This clock's offset at a moment, in seconds east of UTC.
    pub fn offset_at(&self, unix: i64) -> i32 {
        jiff::Timestamp::from_second(unix).map(|t| self.0.to_offset(t).seconds()).unwrap_or(0)
    }

    /// The clock's name for a person: `Asia/Tokyo`, else its offset now (`+05:45`, `UTC`).
    pub fn label(&self) -> String {
        if let Some(n) = self.0.iana_name() {
            return n.to_string();
        }
        let s = self.offset_at(jiff::Timestamp::now().as_second());
        if s == 0 {
            return "UTC".into();
        }
        let (sign, a) = if s < 0 { ('-', -s) } else { ('+', s) };
        format!("{sign}{:02}:{:02}", a / 3600, a % 3600 / 60)
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
    ("cet", "Europe/Berlin"),
    ("cest", "Europe/Berlin"),
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
    ("central european", "Europe/Berlin"),
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
    // Italian: "ora di Berlino", "ora italiana".
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

/// Every city the zone database names, lower-cased with spaces: `europe/berlin` → `berlin`,
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

/// The zone named by `iana`, labelled by its city: `Europe/Berlin` → `Berlin time`.
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
    // "Berlin time", "New York time", "Italian time zone", "Melbourne standard time": one to three
    // words, then "time". A one-word place spelt one slip away is still read here (`near_place`),
    // where "time" says a clock is meant: "17:30 Melbuorne time" was taken for the person's own clock
    // and the meeting landed hours off (2026-10-29).
    for k in 1..=3 {
        if words.get(k).is_some_and(|w| w == "time" || w == "timezone") {
            let named = match &words[..k] {
                [rest @ .., season] if !rest.is_empty() && is_season(season) => rest,
                all => all,
            };
            if let Some(iana) = place_or_near(named) {
                return by_place(iana);
            }
        }
    }
    // "ora di Berlino", "ora italiana", "heure de Londres", "heure française".
    if matches!(first.as_str(), "ora" | "heure") {
        let from =
            if words.get(1).is_some_and(|w| matches!(w.as_str(), "di" | "de")) { 2 } else { 1 };
        for k in from + 1..=from + 2 {
            if let Some(iana) = words.get(from..k).and_then(place_or_near) {
                return by_place(iana);
            }
        }
    }
    None
}

/// The place named by the words **right after a time with nothing else** — `05:30 PM Melbourne`, as a
/// calendar invitation writes it — spelt exactly, one to three words. Weaker than [`after_time`]: "at
/// 17:30 London office" names a room, not a clock, so a caller trusts this only to confirm a time
/// already read elsewhere in the same message.
pub fn place_after_time(words: &[String]) -> Option<Zone> {
    (1..=3.min(words.len())).rev().find_map(|k| place(&words[..k])).and_then(by_place)
}

/// "standard time", "daylight time", "summer time": the season of a clock, not part of its place.
fn is_season(w: &str) -> bool {
    matches!(w, "standard" | "daylight" | "summer" | "winter")
}

/// A place spelt exactly, else the one it is a slip away from ([`near_place`]). Used only where the
/// words around it already say a clock is meant: before "time", after "ora di" or "heure de".
fn place_or_near(words: &[String]) -> Option<&'static str> {
    place(words).or_else(|| near_place(&words.join(" ")))
}

/// The one place a name is a slip away from — one letter added, dropped or changed, or two
/// neighbours swapped, anywhere in it, a space counting as a letter (`new yrok`, `newyork`) — when the
/// name has five letters or more and **exactly one** place is that close. Anything less certain names
/// no clock, as before.
fn near_place(phrase: &str) -> Option<&'static str> {
    if phrase.chars().count() < 5 {
        return None;
    }
    let names = PLACES
        .iter()
        .map(|(w, iana)| (*w, *iana))
        .chain(cities().iter().map(|(c, iana)| (c.as_str(), iana.as_str())));
    let mut found: Option<&'static str> = None;
    for (name, iana) in names {
        if name.chars().count() >= 5 && one_slip(phrase, name) {
            match found {
                Some(other) if other != iana => return None, // two places are that close: unsure
                _ => found = Some(iana),
            }
        }
    }
    found
}

/// Whether `a` and `b` differ by exactly one edit: an insertion, a deletion, a substitution, or two
/// neighbouring letters swapped.
fn one_slip(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a == b {
        return false;
    }
    let (n, m) = (a.len(), b.len());
    if n == m {
        let diff: Vec<usize> = (0..n).filter(|&i| a[i] != b[i]).collect();
        return diff.len() == 1
            || (diff.len() == 2
                && diff[1] == diff[0] + 1
                && a[diff[0]] == b[diff[1]]
                && a[diff[1]] == b[diff[0]]);
    }
    let (long, short) = if n > m { (&a, &b) } else { (&b, &a) };
    if long.len() != short.len() + 1 {
        return false;
    }
    let i = (0..short.len()).find(|&i| long[i] != short[i]).unwrap_or(short.len());
    long[i + 1..] == short[i..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, time};

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    fn tokyo() -> Here {
        Here::parse("Asia/Tokyo").unwrap()
    }

    #[test]
    fn a_place_and_time_is_read_and_converted_on_the_meetings_own_day() {
        let z = after_time(&words("berlin time works for me")).unwrap();
        assert_eq!(z.label, "Berlin time");
        // Berlin leaves summer time on 25 October 2026: +2 before, +1 after.
        assert_eq!(
            z.convert(date!(2026 - 10 - 23), time!(10:00), &tokyo()),
            Some((date!(2026 - 10 - 23), time!(17:00)))
        );
        assert_eq!(
            z.convert(date!(2026 - 10 - 27), time!(10:00), &tokyo()),
            Some((date!(2026 - 10 - 27), time!(18:00)))
        );
    }

    #[test]
    fn a_conversion_may_move_the_day() {
        let z = after_time(&words("new york time")).unwrap();
        assert_eq!(z.label, "New York time");
        assert_eq!(
            z.convert(date!(2026 - 11 - 02), time!(20:00), &tokyo()),
            Some((date!(2026 - 11 - 03), time!(10:00)))
        );
    }

    #[test]
    fn abbreviations_offsets_and_other_languages() {
        assert_eq!(after_time(&words("cet")).unwrap().label, "CET");
        assert_eq!(after_time(&words("gmt")).unwrap().label, "UTC");
        let z = after_time(&words("utc+2")).unwrap();
        assert_eq!(z.label, "UTC+02:00");
        assert_eq!(
            z.convert(date!(2026 - 10 - 27), time!(10:00), &tokyo()),
            Some((date!(2026 - 10 - 27), time!(17:00)))
        );
        assert_eq!(after_time(&words("gmt -5")).unwrap().label, "UTC-05:00");
        assert_eq!(after_time(&words("ora italiana")).unwrap().label, "Rome time");
        assert_eq!(after_time(&words("ora di berlino")).unwrap().label, "Berlin time");
        assert_eq!(after_time(&words("heure de londres")).unwrap().label, "London time");
        assert_eq!(after_time(&words("tokyo time")).unwrap().label, "Tokyo time");
        assert_eq!(after_time(&words("pacific time")).unwrap().label, "Los Angeles time");
    }

    /// **A place one slip away is read where a clock is meant** (2026-10-29: a misspelt "… time"
    /// landed seven hours off). Every kind of slip — a letter missing, extra, changed, or two swapped —
    /// at the start, the middle and the end; one word and several; cities, countries and adjectives;
    /// before "time", after "ora di" and "heure de".
    #[test]
    fn a_place_with_a_typo_is_still_read_where_a_clock_is_meant() {
        let cases = [
            // Two letters swapped in the middle.
            ("melbuorne time", "Melbourne time"),
            // Changed, missing, extra — at the start, the middle, the end.
            ("xelbourne time", "Melbourne time"),
            ("melbxurne time", "Melbourne time"),
            ("melbournx time", "Melbourne time"),
            ("elbourne time", "Melbourne time"),
            ("melbourn time", "Melbourne time"),
            ("mmelbourne time", "Melbourne time"),
            ("melboourne time", "Melbourne time"),
            ("melbournee time", "Melbourne time"),
            // Other cities, each kind of slip.
            ("lodnon time", "London time"),
            ("londno time", "London time"),
            ("tokio time", "Tokyo time"),
            ("tokyoo time", "Tokyo time"),
            ("berln time", "Berlin time"),
            ("bwrlin time", "Berlin time"),
            ("lisbno time", "Lisbon time"),
            ("sydeny time", "Sydney time"),
            // Several words: a slip inside one, or in the space between them.
            ("new yrok time", "New York time"),
            ("newyork time", "New York time"),
            ("new  york time", "New York time"),
            ("los angelse time", "Los Angeles time"),
            ("san fransisco time", "Los Angeles time"),
            // Countries and adjectives.
            ("italain time", "Rome time"),
            ("japnese time", "Tokyo time"),
            ("germna time", "Berlin time"),
            ("switzerlnd time", "Zurich time"),
            // A season after the place, and a typo before it.
            ("melbuorne standard time", "Melbourne time"),
            // Italian and French.
            ("ora di berlnio", "Berlin time"),
            ("ora di londar", "London time"),
            ("ora di madird", "Madrid time"),
            ("ora italaina", "Rome time"),
            ("heure de lodnres", "London time"),
        ];
        for (said, clock) in cases {
            assert_eq!(
                after_time(&words(said)).map(|z| z.label),
                Some(clock.to_string()),
                "{said:?}"
            );
        }
    }

    /// What a slip does **not** excuse: two slips, a slip as near one place as another, a name too
    /// short to tell, a place not followed by "time", and the everyday "… time" a city is not.
    #[test]
    fn a_typo_too_far_unsure_or_out_of_context_names_no_clock() {
        let none = [
            "melbuonre time", // two slips
            "lonodnn time",   // two slips
            "lawson time",    // one slip from both Dawson and Mawson
            "rme time",       // too short to tell
            "prais",          // no "time" after it
            "melbuorne",
            "lodnon calling",
            "lunch time",
            "dinner time",
            "break time",
            "local time",
            "meeting time",
            "start time",
            "office time",
            "working time",
            "spare time",
            "night time",
            "prime time",
            "screen time",
            "quality time",
            "summer time",
        ];
        for said in none {
            assert!(after_time(&words(said)).is_none(), "{said:?} named a clock");
        }
        assert!(one_slip("melbuorne", "melbourne") && one_slip("londn", "london"));
        assert!(one_slip("newyork", "new york"), "a missing space is one slip");
        assert!(!one_slip("london", "london") && !one_slip("malbuurne", "melbourne"));
    }

    #[test]
    fn a_clocks_season_is_not_part_of_its_place() {
        let z = after_time(&words("melbourne standard time")).unwrap();
        assert_eq!(z.label, "Melbourne time");
        assert_eq!(after_time(&words("british summer time")).unwrap().label, "London time");
    }

    /// An invitation's "05:30 PM Melbourne": a bare place, spelt exactly, read only by the caller
    /// that confirms it.
    #[test]
    fn a_bare_place_after_a_time_is_read_only_spelt_exactly() {
        assert_eq!(place_after_time(&words("melbourne")).unwrap().label, "Melbourne time");
        assert_eq!(place_after_time(&words("new york meeting")).unwrap().label, "New York time");
        assert!(place_after_time(&words("melbuorne")).is_none());
        assert!(place_after_time(&words("works for me")).is_none());
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
        let here = Here::parse("+09:00").unwrap();
        let z = after_time(&words("utc")).unwrap();
        assert_eq!(
            z.convert(date!(2026 - 10 - 27), time!(9:00), &here),
            Some((date!(2026 - 10 - 27), time!(18:00)))
        );
        assert!(Here::parse("Europe/Berlin").is_some());
        assert!(Here::parse("Z").is_some());
        assert!(Here::parse("Berlin").is_none());
        assert!(Here::parse("+25:00").is_none());
        assert!(Here::parse("").is_none());
    }

    #[test]
    fn a_clock_is_named_by_its_zone_else_its_offset() {
        assert_eq!(Here::parse("Asia/Tokyo").unwrap().label(), "Asia/Tokyo");
        assert_eq!(Here::parse("+05:45").unwrap().label(), "+05:45");
        assert_eq!(Here::parse("-03:30").unwrap().label(), "-03:30");
        assert_eq!(Here::parse("utc").unwrap().label(), "UTC");
    }
}
