//! Shared build/xtask validation for Fluent syntax, IDs, and message arguments.
use fluent_syntax::ast::{self, Expression, InlineExpression, PatternElement};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    path::Path,
};

pub type Messages = BTreeMap<String, BTreeSet<String>>;
type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Default)]
struct PatternInfo {
    variables: BTreeSet<String>,
    references: BTreeSet<String>,
    terms: BTreeSet<String>,
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

pub fn load(directory: &Path) -> Result<BTreeMap<String, Messages>> {
    let source = parse(&fs::read_to_string(directory.join("en/clawback.ftl"))?)?;
    let mut catalogs = BTreeMap::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let locale = path.file_name().and_then(|s| s.to_str()).ok_or("invalid locale directory")?;
        locale.parse::<unic_langid::LanguageIdentifier>()?;
        let messages = parse(&fs::read_to_string(path.join("clawback.ftl"))?).map_err(|e| format!("{locale}: {e}"))?;
        for (id, variables) in &messages {
            let expected = source.get(id).ok_or_else(|| format!("{locale}: unknown message {id}"))?;
            // Languages may omit arguments they don't need, but cannot invent new runtime inputs.
            if !variables.is_subset(expected) {
                return Err(format!("{locale}: unexpected variables for {id}: {variables:?}").into());
            }
        }
        catalogs.insert(locale.into(), messages);
    }
    Ok(catalogs)
}

#[cfg(test)]
mod tests {
    use super::*;
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
