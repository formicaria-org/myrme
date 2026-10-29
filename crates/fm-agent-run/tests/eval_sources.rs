//! **The sources eval** (R7 of `docs/research/2026-10-29-reputable-sources.md`): exact match on what
//! code decides before any model runs — which sources a question goes to, which body's page the
//! sources pack offers for it, and the cases where the right answer is *no page*. Offline: the pack is
//! the one built into this crate. The live run behind the research doc's table is
//! `websearch::tests::live_sources`.
#![cfg(feature = "download")]

use fm_agent_run::sources::{pack_search, Need};

/// (question, its need). Invented questions in the five languages the app is used in.
const ROUTES: &[(&str, Need)] = &[
    ("what does vitamin K do", Need::Health),
    ("symptoms of iron deficiency", Need::Health),
    ("how much vitamin D should an adult take a day", Need::Health),
    ("is measles dangerous for children", Need::Health),
    ("à quoi sert la vitamine K", Need::Health),
    ("quels sont les symptômes de la carence en fer", Need::Health),
    ("Wie äußert sich Eisenmangel?", Need::Health),
    ("Kopfschmerzen und Schwindel", Need::Health),
    ("sintomi della carenza di vitamina D", Need::Health),
    ("维生素K有什么作用", Need::Health),
    ("which foods are rich in iron", Need::Food),
    ("how much vitamin K in spinach", Need::Food),
    ("combien de vitamine K dans les épinards", Need::Food),
    ("aliments riches en calcium", Need::Food),
    ("Wie viel Vitamin K ist in Spinat?", Need::Food),
    ("Lebensmittel mit viel Eisen", Need::Food),
    ("quanta vitamina K negli spinaci", Need::Food),
    ("菠菜中有多少维生素K", Need::Food),
    ("studies on vitamin K and bone density", Need::Papers),
    ("what does 10.1038/nature14539 show", Need::Papers),
    ("summarise arXiv:2401.01234", Need::Papers),
    ("études sur la vitamine D et la grippe", Need::Papers),
    ("Studien zu Eisenmangel bei Sportlern", Need::Papers),
    ("how do I fix a borrow checker error in a Rust closure", Need::Code),
    ("python regex to match a date", Need::Code),
    ("who built the Analytical Engine", Need::General),
    ("history of the Roman aqueducts", Need::General),
    ("which amino acids are essential", Need::General),
    ("la tour Eiffel a été construite quand", Need::General),
];

#[test]
fn a_question_goes_to_the_sources_its_need_names() {
    let wrong: Vec<String> = ROUTES
        .iter()
        .filter(|(q, want)| Need::of(q) != *want)
        .map(|(q, want)| format!("{q}: {:?}, wanted {want:?}", Need::of(q)))
        .collect();
    println!("routing: {}/{} exact", ROUTES.len() - wrong.len(), ROUTES.len());
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// (search words as the question's keywords, its language, the hosts any of whose pages is a right
/// first answer — empty where no body's page is about it).
const PAGES: &[(&str, &str, &[&str])] = &[
    ("vitamin K", "en", &["www.nhs.uk"]),
    ("vitamin A", "en", &["www.nhs.uk"]),
    ("vitamin E", "en", &["www.nhs.uk"]),
    ("iron deficiency anaemia", "en", &["www.nhs.uk", "www.who.int"]),
    ("measles", "en", &["www.nhs.uk", "www.who.int"]),
    ("malaria", "en", &["www.nhs.uk", "www.who.int"]),
    ("Eisenmangel", "de", &["gesund.bund.de", "www.dge.de"]),
    ("Masern", "de", &["gesund.bund.de"]),
    ("Vitamin D", "de", &["www.dge.de", "gesund.bund.de", "www.bfr.bund.de", "www.blv.admin.ch"]),
    ("rougeole", "fr", &["www.who.int", "www.santepubliquefrance.fr"]),
    // No French body has a page on vitamin D alone; the NHS's, in English, is the right one.
    ("vitamine D", "fr", &["www.anses.fr", "www.blv.admin.ch", "www.nhs.uk"]),
    ("morbillo", "it", &["www.epicentro.iss.it"]),
    ("麻疹", "zh", &["www.who.int"]),
    ("Analytical Engine", "en", &[]),
    ("Roman aqueducts", "en", &[]),
    ("borrow checker", "en", &[]),
    ("Eiffel tour construite", "fr", &[]),
];

#[test]
fn the_pack_offers_a_body_s_page_or_nothing() {
    let host = |url: &str| url.split('/').nth(2).unwrap_or("").to_string();
    let mut wrong = Vec::new();
    for (words, lang, want) in PAGES {
        let got = pack_search(words, lang, 1).first().map(|p| (host(p.url), p.title));
        let right = match &got {
            Some((h, _)) => want.contains(&h.as_str()),
            None => want.is_empty(),
        };
        if !right {
            wrong.push(format!("{words} ({lang}): {got:?}, wanted one of {want:?}"));
        }
    }
    println!("pack: {}/{} exact", PAGES.len() - wrong.len(), PAGES.len());
    assert!(wrong.is_empty(), "{wrong:#?}");
}
