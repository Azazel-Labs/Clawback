# Translating Clawback

Translations live in Git and are reviewed in pull requests. No translation
service or API token is needed.

The English sources are `locales/en/clawback.ftl` and `README.md`. Each locale
directory must contain every English message (including attributes), with valid
Fluent syntax and no unknown message IDs or runtime arguments. Each locale also
needs `docs/i18n/README.<locale>.md`. These localized READMEs provide the overview,
features, installation, terminal commands, scanning behavior and contribution
links; the detailed guides remain in English.

After changing English text, update each affected translation. The check reports
exact message IDs with their previous and current English text. Review only the
strings you have checked, for each locale:

```sh
cargo xtask translations fmt
cargo xtask translations check
cargo xtask translations review fr empty-temp-body
cargo xtask translations review fr button.label another-message
cargo xtask translations check
```

Commit the catalogs and `localization/reviewed/<locale>.toml` files together.
These files store the reviewed English and translated text for each message
value and attribute. Changing one string invalidates only that string in each
locale. Changing a translation invalidates only its review in that locale.
Plural variants belong to their containing string. Referenced messages and
terms are included transitively, so changes also invalidate strings that use
them. Comments, entry order, Fluent formatting and line endings do not invalidate
UI string reviews. Meaningful literal whitespace still does.

If an existing translation remains correct, review that ID without making an
artificial text change. New strings need translations and reviews in every
locale. Removed IDs are reported too; review those IDs to remove their obsolete
records. Reviewing selected IDs leaves all other pending reviews untouched.

README reviews remain separate, using whole-file SHA-256 fingerprints of the
English and localized README. After checking the localized README, run:

```sh
cargo xtask translations review fr --readme
```

This does not accept changed UI strings. README wording or link changes require
README review; line-ending changes alone do not. To initialize a new locale, or
after explicitly reviewing its entire catalog and README, use
`cargo xtask translations review fr --all`. Calling `review fr` without IDs or
an explicit flag fails, so it cannot silently approve the whole locale.

`cargo xtask translations check` fails on missing, empty or stale files and runs
in CI. Configure the repository's branch rules to require its existing
**Clippy + tests** checks before merging; a workflow by itself cannot prevent
someone with permission from bypassing branch rules. Review records prove which
revision was acknowledged, not linguistic accuracy. Reviewers must still check
the translation, especially any machine-assisted wording.

The initial README translations and newly completed UI messages were
machine-assisted. They were checked for Fluent syntax, coverage, runtime
variables and literal shell commands, with corrections for technical context
and destructive-action wording. Native-speaker proofreading is still welcome;
the review records are not a certification of linguistic accuracy.

To add a language, add a complete Fluent catalog and a localized README, add its
native-language link to the README navigation, and record its review. The
application's language list is generated from the locale directories.
