//! The designer's texts in its languages: Fluent files in `assets/i18n` (one per language, English the
//! source), embedded so the desktop and the web build read the same. `tr("key")` gives a text in the current
//! language; a key a language lacks falls back to English, then to the key itself.

use std::cell::RefCell;
use std::collections::HashMap;

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};

/// The languages, as Settings lists them after "System": English (the source), then by their own names,
/// Latin script before Cyrillic.
pub const LANGUAGES: [(&str, &str); 6] = [("en", "English"), ("de", "Deutsch"), ("es", "Español"), ("fr", "Français"), ("sr", "Srpski"), ("ru", "Русский")];

fn source(lang: &str) -> &'static str {
    match lang {
        "de" => include_str!("../i18n/de.ftl"),
        "es" => include_str!("../i18n/es.ftl"),
        "fr" => include_str!("../i18n/fr.ftl"),
        "ru" => include_str!("../i18n/ru.ftl"),
        "sr" => include_str!("../i18n/sr.ftl"),
        _ => include_str!("../i18n/en.ftl"),
    }
}

thread_local! {
    static CURRENT: RefCell<&'static str> = const { RefCell::new("en") };
    static BUNDLES: RefCell<HashMap<&'static str, FluentBundle<FluentResource>>> = RefCell::new(HashMap::new());
}

/// A language of `LANGUAGES` by its code, else English.
fn known(lang: &str) -> &'static str {
    LANGUAGES.iter().find(|(code, _)| *code == lang).map_or("en", |(code, _)| code)
}

/// The language the next build draws with (the window is built again to show it).
pub fn set_language(lang: &str) {
    CURRENT.with(|c| *c.borrow_mut() = known(lang));
}

/// The current language's code.
pub fn language() -> &'static str {
    CURRENT.with(|c| *c.borrow())
}

/// A language's own name ("Deutsch").
pub fn native_name(lang: &str) -> &'static str {
    LANGUAGES.iter().find(|(code, _)| *code == lang).map_or("English", |(_, name)| name)
}

/// The system's language when the designer has it, else English. The web build has no system locale here.
pub fn system_language() -> &'static str {
    #[cfg(not(target_os = "emscripten"))]
    if let Some(locale) = sys_locale::get_locale() {
        return known(locale.split(['-', '_']).next().unwrap_or("en"));
    }
    "en"
}

fn with_bundle<R>(lang: &'static str, f: impl FnOnce(&FluentBundle<FluentResource>) -> R) -> R {
    BUNDLES.with(|bundles| {
        let mut bundles = bundles.borrow_mut();
        let bundle = bundles.entry(lang).or_insert_with(|| {
            let id = lang.parse().expect("a language code");
            let mut bundle = FluentBundle::new(vec![id]);
            // No Unicode isolation marks around the filled-in values: the fonts draw them as boxes.
            bundle.set_use_isolating(false);
            let resource = FluentResource::try_new(source(lang).to_owned()).unwrap_or_else(|(_, errors)| panic!("{lang}.ftl: {errors:?}"));
            bundle.add_resource(resource).unwrap_or_else(|errors| panic!("{lang}.ftl: {errors:?}"));
            bundle
        });
        f(bundle)
    })
}

fn format(lang: &'static str, id: &str, args: Option<&FluentArgs>) -> Option<String> {
    with_bundle(lang, |bundle| {
        let pattern = bundle.get_message(id)?.value()?;
        let mut errors = vec![];
        Some(bundle.format_pattern(pattern, args, &mut errors).into_owned())
    })
}

/// The text `id` in the current language.
pub fn tr(id: &str) -> String {
    tr_args(id, &[])
}

/// The text `id` with its `{ $name }` values filled in (numbers choose the plural form).
pub fn tr_args(id: &str, args: &[(&str, FluentValue<'static>)]) -> String {
    let mut fluent_args = FluentArgs::new();
    for (name, value) in args {
        fluent_args.set(*name, value.clone());
    }
    let args = (!args.is_empty()).then_some(&fluent_args);
    format(language(), id, args).or_else(|| format("en", id, args)).unwrap_or_else(|| id.to_owned())
}

/// The text `id` with string values, in the current language (messages that came from the hub).
pub fn tr_pairs(id: &str, args: &[(String, String)]) -> String {
    let values: Vec<(&str, FluentValue<'static>)> = args.iter().map(|(k, v)| (k.as_str(), FluentValue::from(v.clone()))).collect();
    tr_args(id, &values)
}

/// The text `id` with string values, in English, whatever the current language: what AI agents read.
pub fn english(id: &str, args: &[(String, String)]) -> String {
    let mut fluent_args = FluentArgs::new();
    for (name, value) in args {
        fluent_args.set(name.as_str(), FluentValue::from(value.clone()));
    }
    let args = (!args.is_empty()).then_some(&fluent_args);
    format("en", id, args).unwrap_or_else(|| id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every language has every English key and nothing else, and every file parses.
    #[test]
    fn every_language_has_every_key() {
        let keys = |lang: &'static str| -> Vec<String> {
            let resource = FluentResource::try_new(source(lang).to_owned()).unwrap_or_else(|(_, e)| panic!("{lang}: {e:?}"));
            let mut keys: Vec<String> = resource
                .entries()
                .filter_map(|e| match e {
                    fluent_syntax::ast::Entry::Message(m) => Some(m.id.name.to_owned()),
                    _ => None,
                })
                .collect();
            keys.sort();
            keys
        };
        let english = keys("en");
        for (lang, _) in LANGUAGES {
            assert_eq!(keys(lang), english, "{lang}.ftl has the same keys as en.ftl");
        }
    }

    #[test]
    fn plurals_and_values() {
        set_language("ru");
        assert_eq!(tr_args("controls-count", &[("count", 1.into())]), "1 элемент");
        assert_eq!(tr_args("controls-count", &[("count", 3.into())]), "3 элемента");
        assert_eq!(tr_args("controls-count", &[("count", 75.into())]), "75 элементов");
        set_language("sr");
        assert_eq!(tr_args("controls-count", &[("count", 3.into())]), "3 kontrole");
        set_language("en");
        assert_eq!(tr_args("controls-count", &[("count", 1.into())]), "1 control");
        assert_eq!(tr_args("app-run", &[("name", "HelloWpf".into())]), "Run HelloWpf");
        assert_eq!(tr("no-such-key"), "no-such-key");
    }
}
