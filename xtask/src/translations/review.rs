use super::{Result, catalog};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use toml_edit::{DocumentMut, Table, value};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Record {
    source: String,
    translation: String,
}

#[derive(Default)]
struct Review {
    messages: BTreeMap<String, Record>,
    readme: Record,
}

fn path(root: &Path, locale: &str) -> PathBuf {
    root.join("localization/reviewed").join(format!("{locale}.toml"))
}

fn text(path: &Path) -> Result<String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?.replace("\r\n", "\n");
    if text.trim().is_empty() {
        return Err(format!("{}: empty source or translation", path.display()).into());
    }
    Ok(text)
}

fn current(root: &Path, locale: &str) -> Result<Review> {
    let source_text = text(&root.join("locales/en/clawback.ftl"))?;
    let translation_text = text(&root.join(format!("locales/{locale}/clawback.ftl")))?;
    let source = catalog::parse(&source_text)?;
    let translated = catalog::parse(&translation_text)?;
    for (id, args) in &source {
        let actual = translated.get(id).ok_or_else(|| format!("{locale}: missing translation for {id}"))?;
        if !actual.is_subset(args) {
            return Err(format!("{locale}: unexpected variables for {id}: {actual:?}").into());
        }
    }
    for id in translated.keys() {
        if !source.contains_key(id) {
            return Err(format!("{locale}: unknown message {id}").into());
        }
    }
    let source = catalog::revisions(&source_text)?;
    let mut translated = catalog::revisions(&translation_text)?;
    let messages = source
        .into_iter()
        .map(|(id, source)| {
            let translation = translated.remove(&id).expect("validated translation key");
            (id, Record { source, translation })
        })
        .collect();
    Ok(Review {
        messages,
        readme: Record {
            source: format!("{:x}", Sha256::digest(text(&root.join("README.md"))?.as_bytes())),
            translation: format!(
                "{:x}",
                Sha256::digest(text(&root.join(format!("docs/i18n/README.{locale}.md")))?.as_bytes())
            ),
        },
    })
}

fn parse(text: &str) -> Result<Review> {
    let doc = text.parse::<DocumentMut>()?;
    if doc.get("version").and_then(toml_edit::Item::as_integer) != Some(2) {
        return Err("Unsupported translation review format (expected version 2)".into());
    }
    let read = |table: &Table| -> Result<Record> {
        Ok(Record {
            source: table.get("source").and_then(|v| v.as_str()).ok_or("Missing review source")?.to_owned(),
            translation: table
                .get("translation")
                .and_then(|v| v.as_str())
                .ok_or("Missing review translation")?
                .to_owned(),
        })
    };
    let mut messages = BTreeMap::new();
    if let Some(table) = doc.get("messages") {
        for (id, item) in table.as_table().ok_or("Invalid messages review table")? {
            messages.insert(id.to_owned(), read(item.as_table().ok_or("Invalid message review record")?)?);
        }
    }
    let readme = doc
        .get("readme")
        .map(|item| read(item.as_table().ok_or("Invalid README review")?))
        .transpose()?
        .unwrap_or_default();
    Ok(Review { messages, readme })
}

fn load(root: &Path, locale: &str) -> Result<Review> {
    match fs::read_to_string(path(root, locale)) {
        Ok(text) => parse(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Review::default()),
        Err(error) => Err(error.into()),
    }
}

fn serialize(review: &Review) -> String {
    fn record(record: &Record) -> Table {
        let mut table = Table::new();
        table["source"] = value(&record.source);
        table["translation"] = value(&record.translation);
        table
    }
    let mut doc = DocumentMut::new();
    doc["version"] = value(2);
    doc["readme"] = toml_edit::Item::Table(record(&review.readme));
    let mut messages = Table::new();
    messages.set_implicit(true);
    for (id, entry) in &review.messages {
        messages.insert(id, toml_edit::Item::Table(record(entry)));
    }
    doc["messages"] = toml_edit::Item::Table(messages);
    doc.to_string()
}

fn differences(locale: &str, expected: &Review, reviewed: &Review) -> Vec<String> {
    let mut errors = Vec::new();
    for (id, now) in &expected.messages {
        match reviewed.messages.get(id) {
            None => errors.push(format!("{locale}: unreviewed string: {id}\n  Current English: {}", now.source)),
            Some(old) if old != now => {
                if old.source != now.source {
                    errors.push(format!(
                        "{locale}: stale translation: {id}\n  Previous English: {}\n  Current English: {}",
                        old.source, now.source
                    ));
                }
                if old.translation != now.translation {
                    errors.push(format!(
                        "{locale}: changed translation: {id}\n  Previous translation: {}\n  Current translation: {}",
                        old.translation, now.translation
                    ));
                }
            }
            _ => {}
        }
    }
    for id in reviewed.messages.keys().filter(|id| !expected.messages.contains_key(*id)) {
        errors.push(format!("{locale}: removed string: {id}; review this ID to remove its obsolete record"));
    }
    if expected.readme != reviewed.readme {
        errors.push(format!(
            "{locale}: stale README review; review with cargo xtask translations review {locale} --readme"
        ));
    }
    errors
}

pub fn check(root: &Path, locale: &str) -> Result<Vec<String>> {
    Ok(differences(locale, &current(root, locale)?, &load(root, locale)?))
}

fn update(reviewed: &mut Review, current: Review, ids: &[String], all: bool, readme: bool) -> Result<()> {
    if usize::from(all) + usize::from(readme) + usize::from(!ids.is_empty()) != 1 {
        return Err("Choose message IDs, --readme, or --all explicitly".into());
    }
    if all {
        *reviewed = current;
    } else if readme {
        reviewed.readme = current.readme;
    } else {
        for id in ids {
            if let Some(record) = current.messages.get(id) {
                reviewed.messages.insert(id.clone(), record.clone());
            } else if reviewed.messages.remove(id).is_none() {
                return Err(format!("Unknown message ID: {id}").into());
            }
        }
    }
    Ok(())
}

pub fn run(root: &Path, locale: &str, ids: &[String], all: bool, readme: bool) -> Result<()> {
    if locale == "en" || !catalog::files(&root.join("locales"))?.iter().any(|(name, _)| name == locale) {
        return Err(format!("Not a translation locale: {locale}").into());
    }
    let mut reviewed = load(root, locale)?;
    update(&mut reviewed, current(root, locale)?, ids, all, readme)?;
    fs::create_dir_all(root.join("localization/reviewed"))?;
    fs::write(path(root, locale), serialize(&reviewed))?;
    println!("Recorded selected reviews for {locale}; commit the review file with the translations.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(source: &str, translation: &str) -> Review {
        let translated = catalog::revisions(translation).expect("translation");
        Review {
            messages: catalog::revisions(source)
                .expect("source")
                .into_iter()
                .map(|(id, source)| (id.clone(), Record { source, translation: translated[&id].clone() }))
                .collect(),
            readme: Record { source: "english readme hash".into(), translation: "French readme hash".into() },
        }
    }

    #[test]
    fn only_changed_strings_are_invalidated_and_selective_review_leaves_other_work_stale() {
        let mut old = fixture("a = One\nb = Two", "a = Un\nb = Deux");
        let now = fixture("a = First\nb = Second", "a = Premier\nb = Deux");
        let errors = differences("fr", &now, &old).join("\n");
        assert!(errors.contains("Previous English: One"));
        assert!(errors.contains("Current English: First"));
        update(&mut old, now, &["a".into()], false, false).expect("review a");
        let now = fixture("a = First\nb = Second", "a = Premier\nb = Deux");
        let errors = differences("fr", &now, &old);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("stale translation: b"));
        let restored = parse(&serialize(&old)).expect("review roundtrip");
        assert_eq!(restored.messages, old.messages);
        assert_eq!(restored.readme, old.readme);
    }

    #[test]
    fn added_removed_translation_only_and_readme_changes_are_independent() {
        let mut old = fixture("a = One\nb = Two", "a = Un\nb = Deux");
        let mut now = fixture("a = One\nc = Three", "a = Une\nc = Trois");
        now.readme.source = "new README".into();
        let errors = differences("fr", &now, &old).join("\n");
        for expected in ["changed translation: a", "unreviewed string: c", "removed string: b", "stale README"] {
            assert!(errors.contains(expected), "{errors}");
        }
        update(&mut old, now, &["b".into(), "c".into()], false, false).expect("review added and removed");
        assert!(!old.messages.contains_key("b"));
        assert!(old.messages.contains_key("c"));
        assert_eq!(old.messages["a"].translation, "Un");
        assert_eq!(old.readme.source, "english readme hash");
        let mut now = fixture("a = One\nc = Three", "a = Une\nc = Trois");
        now.readme.source = "new README".into();
        update(&mut old, now, &[], false, true).expect("review README only");
        assert_eq!(old.readme.source, "new README");
        assert_eq!(old.messages["a"].translation, "Un");
        assert!(update(&mut old, Review::default(), &[], false, false).is_err());
        assert!(parse("version = 1").is_err());
        assert!(parse("").is_err());
    }
}
