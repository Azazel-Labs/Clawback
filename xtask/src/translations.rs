use crate::Result;
use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    fs,
    path::Path,
};
use syn::{parse::Parser, visit::Visit};
#[path = "../../localization/catalog.rs"]
mod catalog;

#[derive(Default)]
struct Extractor {
    messages: BTreeMap<String, BTreeSet<String>>,
    error: Option<syn::Error>,
}
impl<'ast> Visit<'ast> for Extractor {
    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if node.path.segments.last().is_some_and(|part| part.ident == "tr") {
            self.message(node.tokens.clone());
        } else {
            self.nested(node.tokens.clone());
        }
    }
}

impl Extractor {
    fn message(&mut self, tokens: proc_macro2::TokenStream) {
        let parser = |input: syn::parse::ParseStream<'_>| {
            let id: syn::LitStr = input.parse()?;
            let mut arguments = BTreeSet::new();
            while !input.is_empty() {
                input.parse::<syn::Token![,]>()?;
                if input.is_empty() {
                    break;
                }
                let name: syn::Ident = input.parse()?;
                input.parse::<syn::Token![=]>()?;
                input.parse::<syn::Expr>()?;
                if !arguments.insert(name.to_string()) {
                    return Err(syn::Error::new(name.span(), "duplicate argument"));
                }
            }
            Ok((id, arguments))
        };
        match parser.parse2(tokens) {
            Ok((id, arguments)) => match self.messages.entry(id.value()) {
                Entry::Vacant(entry) => {
                    entry.insert(arguments);
                }
                Entry::Occupied(entry) if *entry.get() != arguments => {
                    let message = format!("inconsistent arguments for message ID {:?}", entry.key());
                    self.fail(syn::Error::new(id.span(), message));
                }
                Entry::Occupied(_) => {}
            },
            Err(error) => self.fail(error),
        }
    }

    /// Keep every error, not just the last.
    fn fail(&mut self, error: syn::Error) {
        match &mut self.error {
            Some(errors) => errors.combine(error),
            None => self.error = Some(error),
        }
    }

    /// syn does not visit Rust expressions inside macros such as vec!.
    fn nested(&mut self, tokens: proc_macro2::TokenStream) {
        use proc_macro2::TokenTree;
        let tokens = tokens.into_iter().collect::<Vec<_>>();
        let mut i = 0;
        while i < tokens.len() {
            if matches!(&tokens[i], TokenTree::Ident(name) if name == "tr")
                && matches!(tokens.get(i + 1), Some(TokenTree::Punct(p)) if p.as_char() == '!')
                && let Some(TokenTree::Group(group)) = tokens.get(i + 2)
            {
                self.message(group.stream());
                i += 3;
                continue;
            }
            if let TokenTree::Group(group) = &tokens[i] {
                self.nested(group.stream());
            }
            i += 1;
        }
    }
}

fn extract(directory: &Path, extractor: &mut Extractor) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            extract(&path, extractor)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            extractor.visit_file(&syn::parse_file(&fs::read_to_string(path)?)?);
        }
    }
    Ok(())
}

pub const USAGE: &str = "cargo xtask translations <check|fmt [--check]>";

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    let directory = root.join("locales");
    match args {
        [command] if command == "check" => {}
        [command] if command == "fmt" => return format_catalogs(&directory, true),
        [command, flag] if command == "fmt" && flag == "--check" => return format_catalogs(&directory, false),
        _ => return Err(crate::usage(USAGE)),
    }
    let mut extractor = Extractor::default();
    extract(&root.join("src"), &mut extractor)?;
    if let Some(errors) = extractor.error {
        return Err(errors.into_iter().map(|error| error.to_string()).collect::<Vec<_>>().join("\n").into());
    }
    let catalogs = catalog::load(&directory)?;
    let source = catalogs.get("en").ok_or("missing English catalog")?;
    for (id, arguments) in &extractor.messages {
        if source.get(id) != Some(arguments) {
            return Err(format!(
                "Message {id:?}: Rust arguments {arguments:?} do not match the English Fluent catalog {:?}",
                source.get(id)
            )
            .into());
        }
    }
    format_catalogs(&directory, false)?;
    println!("Validated {} messages in {} catalogs.", extractor.messages.len(), catalogs.len());
    Ok(())
}

fn formatted(source: &str) -> Result<String> {
    // Accept Windows checkouts while keeping canonical output independent of OS.
    let source = source.replace("\r\n", "\n");
    let resource = fluent_syntax::parser::parse(source.as_str())
        .map_err(|(_, errors)| format!("Invalid Fluent; refusing to format: {errors:?}"))?;
    Ok(fluent_syntax::serializer::serialize(&resource))
}

fn format_catalogs(directory: &Path, write: bool) -> Result<()> {
    let mut changes = Vec::new();
    for (_, path) in catalog::files(directory)? {
        let source = fs::read_to_string(&path)?;
        let output = formatted(&source).map_err(|error| format!("{}: {error}", path.display()))?;
        if source.replace("\r\n", "\n") != output {
            changes.push((path, output));
        }
    }
    if !write && !changes.is_empty() {
        let paths = changes.iter().map(|(path, _)| path.display().to_string()).collect::<Vec<_>>().join("\n");
        return Err(format!("Fluent formatting differs:\n{paths}\nRun cargo xtask translations fmt").into());
    }
    // Parse every file before writing any, so malformed input cannot be silently discarded.
    for (path, output) in &changes {
        fs::write(path, output)?;
    }
    if write {
        println!("Formatted {} Fluent catalogs.", changes.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formatting_preserves_grammar_comments_and_literal_whitespace() {
        let source = "# Translator context\ncount = { $n ->\n [one] One\n *[other] { $n } files\n    }\n\nline = { \"\\u000A  \" }{ $path }\n";
        let output = formatted(source).expect("valid Fluent");
        assert_eq!(fluent_syntax::parser::parse(source), fluent_syntax::parser::parse(output.as_str()));
        assert_eq!(formatted(&output).expect("formatted Fluent"), output);
        assert_eq!(formatted(&source.replace('\n', "\r\n")).expect("Windows newlines"), output);
        assert!(formatted("broken = { ").is_err());
    }
    #[test]
    fn extracts_nested_macros_and_checks_arguments() {
        let mut extractor = Extractor::default();
        extractor.visit_file(
            &syn::parse_file(r#"fn f() { vec![tr!("name"), tr!("workers", count = 2)]; }"#)
                .expect("valid Rust fixture"),
        );
        assert!(extractor.error.is_none());
        assert_eq!(extractor.messages.len(), 2);
        extractor.visit_file(&syn::parse_file(r#"fn f() { tr!("workers", other = 2); }"#).expect("valid Rust fixture"));
        extractor.visit_file(&syn::parse_file(r#"fn f() { tr!("name", other = 2); }"#).expect("valid Rust fixture"));
        let errors: Vec<_> = extractor.error.expect("errors").into_iter().map(|error| error.to_string()).collect();
        assert_eq!(errors.len(), 2);
        assert!(errors[0].contains("\"workers\""), "{}", errors[0]);
    }
}
