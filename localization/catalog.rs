//! Shared build/xtask validation for Fluent syntax, IDs, and message arguments.
use fluent_syntax::ast::{self, Expression, InlineExpression, PatternElement};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    path::{Path, PathBuf},
};

pub type Messages = BTreeMap<String, BTreeSet<String>>;
type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Default)]
struct PatternInfo {
    variables: BTreeSet<String>,
    references: BTreeSet<String>,
    terms: BTreeSet<String>,
}

/// Canonical text per message value/attribute, including referenced values and terms.
/// Comments, entry ordering and Fluent formatting do not affect these revisions.
#[allow(dead_code)] // Used by xtask; build.rs shares catalog validation only.
pub fn revisions(text: &str) -> Result<BTreeMap<String, String>> {
    let text = text.replace("\r\n", "\n");
    parse(&text)?; // Reject duplicates, unresolved references and cycles first.
    let resource =
        fluent_syntax::parser::parse(text.as_str()).map_err(|(_, errors)| format!("Invalid Fluent: {errors:?}"))?;
    let mut values = BTreeMap::new();
    let mut dependencies = BTreeMap::new();
    for entry in &resource.body {
        let (id, value, attributes) = match entry {
            ast::Entry::Message(message) => (message.id.name.to_owned(), message.value.as_ref(), &message.attributes),
            ast::Entry::Term(term) => (format!("-{}", term.id.name), Some(&term.value), &term.attributes),
            _ => continue,
        };
        let patterns = value
            .map(|value| (id.clone(), value))
            .into_iter()
            .chain(attributes.iter().map(|attribute| (format!("{id}.{}", attribute.id.name), &attribute.value)));
        for (key, value) in patterns {
            let mut info = PatternInfo::default();
            pattern(value, &mut info);
            info.references.extend(info.terms);
            dependencies.insert(key.clone(), info.references);
            let single = ast::Resource {
                body: vec![ast::Entry::Message(ast::Message {
                    id: ast::Identifier { name: "value" },
                    value: Some(value.clone()),
                    attributes: Vec::new(),
                    comment: None,
                })],
            };
            let serialized = fluent_syntax::serializer::serialize(&single);
            values.insert(key, serialized.trim_start_matches("value =").trim().to_owned());
        }
    }
    fn collect(id: &str, dependencies: &BTreeMap<String, BTreeSet<String>>, seen: &mut BTreeSet<String>) {
        for dependency in &dependencies[id] {
            if seen.insert(dependency.clone()) {
                collect(dependency, dependencies, seen);
            }
        }
    }
    let mut revisions = BTreeMap::new();
    for (id, value) in values.iter().filter(|(id, _)| !id.starts_with('-')) {
        let mut references = BTreeSet::new();
        collect(id, &dependencies, &mut references);
        let mut revision = value.clone();
        for reference in references {
            use std::fmt::Write;
            write!(revision, "\n\n# reference {reference}\n{}", values[&reference])?;
        }
        revisions.insert(id.clone(), revision);
    }
    Ok(revisions)
}

fn pattern(value: &ast::Pattern<&str>, info: &mut PatternInfo) {
    for element in &value.elements {
        if let PatternElement::Placeable { expression: value } = element {
            expression(value, info);
        }
    }
}

fn expression(value: &Expression<&str>, info: &mut PatternInfo) {
    match value {
        Expression::Inline(value) => inline(value, info),
        Expression::Select { selector, variants } => {
            inline(selector, info);
            for variant in variants {
                pattern(&variant.value, info);
            }
        }
    }
}

fn arguments(args: &ast::CallArguments<&str>, info: &mut PatternInfo) {
    for value in &args.positional {
        inline(value, info);
    }
    for value in &args.named {
        inline(&value.value, info);
    }
}

fn inline(value: &InlineExpression<&str>, info: &mut PatternInfo) {
    match value {
        InlineExpression::VariableReference { id } => {
            info.variables.insert(id.name.into());
        }
        InlineExpression::MessageReference { id, attribute } => {
            info.references
                .insert(attribute.as_ref().map_or_else(|| id.name.into(), |attr| format!("{}.{}", id.name, attr.name)));
        }
        InlineExpression::TermReference { id, attribute, arguments: args } => {
            info.terms.insert(
                attribute
                    .as_ref()
                    .map_or_else(|| format!("-{}", id.name), |attr| format!("-{}.{}", id.name, attr.name)),
            );
            if let Some(args) = args {
                arguments(args, info);
            }
        }
        InlineExpression::FunctionReference { arguments: args, .. } => arguments(args, info),
        InlineExpression::Placeable { expression: value } => expression(value, info),
        InlineExpression::StringLiteral { .. } | InlineExpression::NumberLiteral { .. } => {}
    }
}

pub fn parse(text: &str) -> Result<Messages> {
    let resource = fluent_syntax::parser::parse(text).map_err(|(_, errors)| format!("Invalid Fluent: {errors:?}"))?;
    let mut entries = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for entry in &resource.body {
        let (id, value, attributes) = match entry {
            ast::Entry::Message(message) => (message.id.name.to_owned(), message.value.as_ref(), &message.attributes),
            ast::Entry::Term(term) => (format!("-{}", term.id.name), Some(&term.value), &term.attributes),
            _ => continue,
        };
        if !ids.insert(id.clone()) {
            return Err(format!("Duplicate Fluent ID: {id}").into());
        }
        if let Some(value) = value {
            let mut info = PatternInfo::default();
            pattern(value, &mut info);
            entries.insert(id.clone(), info);
        }
        for attribute in attributes {
            let key = format!("{id}.{}", attribute.id.name);
            let mut info = PatternInfo::default();
            pattern(&attribute.value, &mut info);
            if entries.insert(key.clone(), info).is_some() {
                return Err(format!("Duplicate attribute: {key}").into());
            }
        }
    }
    fn collect(
        id: &str,
        entries: &BTreeMap<String, PatternInfo>,
        stack: &mut BTreeSet<String>,
    ) -> Result<BTreeSet<String>> {
        if !stack.insert(id.into()) {
            return Err(format!("Cyclic Fluent reference: {id}").into());
        }
        let info = entries.get(id).ok_or_else(|| format!("Missing Fluent reference: {id}"))?;
        let mut vars = info.variables.clone();
        for reference in &info.references {
            vars.extend(collect(reference, entries, stack)?);
        }
        // Term variables have their own scope; only arguments supplied by the caller escape it.
        for term in &info.terms {
            collect(term, entries, stack)?;
        }
        stack.remove(id);
        Ok(vars)
    }
    let mut messages = BTreeMap::new();
    for id in entries.keys() {
        let variables = collect(id, &entries, &mut BTreeSet::new())?;
        if !id.starts_with('-') {
            messages.insert(id.clone(), variables);
        }
    }
    Ok(messages)
}

/// Every locale directory's catalog path, sorted by locale.
pub fn files(directory: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let locale = path.file_name().and_then(|s| s.to_str()).ok_or("invalid locale directory")?;
        locale.parse::<unic_langid::LanguageIdentifier>()?;
        files.push((locale.to_owned(), path.join("clawback.ftl")));
    }
    files.sort();
    Ok(files)
}

pub fn load(directory: &Path) -> Result<BTreeMap<String, Messages>> {
    let source = parse(&fs::read_to_string(directory.join("en/clawback.ftl"))?)?;
    let mut catalogs = BTreeMap::new();
    for (locale, path) in files(directory)? {
        if locale == "en" {
            continue;
        }
        let messages = parse(&fs::read_to_string(path)?).map_err(|e| format!("{locale}: {e}"))?;
        for id in source.keys() {
            if !messages.contains_key(id) {
                return Err(format!("{locale}: missing translation for {id}").into());
            }
        }
        for (id, variables) in &messages {
            let expected = source.get(id).ok_or_else(|| format!("{locale}: unknown message {id}"))?;
            // Languages may omit arguments they don't need, but cannot invent new runtime inputs.
            if !variables.is_subset(expected) {
                return Err(format!("{locale}: unexpected variables for {id}: {variables:?}").into());
            }
        }
        catalogs.insert(locale, messages);
    }
    catalogs.insert("en".into(), source);
    Ok(catalogs)
}

#[cfg(test)]
mod tests {
    #[test]
    fn revisions_track_attributes_variants_and_transitive_references_independently() {
        let source = "-brand = Clawback\na = { -brand }\n    .label = Open\nb = { a }\nc = Unchanged\ncount = { $n ->\n    [one] One file\n   *[other] Many files\n}\n";
        let before = revisions(source).expect("source");
        let after = revisions(&source.replace("Open", "Launch")).expect("attribute edit");
        let changed: Vec<_> = before.keys().filter(|id| before[*id] != after[*id]).map(String::as_str).collect();
        assert_eq!(changed, ["a.label"]);
        let after = revisions(&source.replace("Clawback", "New brand")).expect("term edit");
        let changed: Vec<_> = before.keys().filter(|id| before[*id] != after[*id]).map(String::as_str).collect();
        assert_eq!(changed, ["a", "b"]);
        let after = revisions(&source.replace("Many files", "Several files")).expect("variant edit");
        let changed: Vec<_> = before.keys().filter(|id| before[*id] != after[*id]).map(String::as_str).collect();
        assert_eq!(changed, ["count"]);
    }

    #[test]
    fn revisions_ignore_comments_order_and_formatting_but_preserve_literal_whitespace() {
        let before = revisions("a = Hello { $name }\nb = { \" \" }\n").expect("source");
        let after = revisions("# Comment\r\nb = {\" \"}\r\n\r\na = Hello {$name}\r\n").expect("formatting");
        assert_eq!(before, after);
        let after = revisions("a = Hello { $name }\nb = { \"  \" }\n").expect("literal edit");
        assert_ne!(before["b"], after["b"]);
        assert!(revisions("a = { b }\nb = { a }").is_err());
    }

    use super::*;
    #[test]
    fn requires_every_source_message_and_attribute() {
        let root = std::env::temp_dir().join(format!("clawback-catalog-test-{}", std::process::id()));
        fs::create_dir_all(root.join("en")).expect("source directory");
        fs::create_dir_all(root.join("fr")).expect("translation directory");
        fs::write(root.join("en/clawback.ftl"), "hello = Hello\nbutton =\n    .label = Save\n").expect("source");
        fs::write(root.join("fr/clawback.ftl"), "hello = Bonjour\n").expect("incomplete translation");
        assert!(load(&root).expect_err("missing attribute").to_string().contains("button.label"));
        fs::write(root.join("fr/clawback.ftl"), "hello = Bonjour\nbutton =\n    .label = Enregistrer\n")
            .expect("complete translation");
        assert_eq!(load(&root).expect("complete catalogs").len(), 2);
        fs::remove_dir_all(root).expect("remove test fixtures");
    }
    #[test]
    fn rejects_malformed_duplicate_and_unresolved_messages() {
        for bad in ["broken =", "a = One\na = Two", "a = { absent }", "a = { b }\nb = { a }"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn accepts_variants_attributes_and_scoped_terms() {
        let messages = parse("-user = { $gender ->\n [female] Elle\n *[other] Il\n}\nresult = { -user(gender: \"female\") }\ncount = { $count ->\n [one] One\n *[other] { $count } files\n}\ncopy = { count }\nbutton =\n .label = Cancel").expect("valid Fluent");
        assert!(messages["result"].is_empty());
        assert_eq!(messages["copy"], BTreeSet::from(["count".into()]));
        assert!(messages.contains_key("button.label"));
    }
}
