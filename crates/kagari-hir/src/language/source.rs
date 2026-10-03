//! Handwritten core source, combined with native declarations in the installed module.
use kagari_contract::language::role::LangRole;

pub const CORE_URI: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../library/core/language.kgr"
);

pub const CORE_SOURCE: &str = include_str!("../../../../library/core/language.kgr");

pub(crate) fn trait_source(role: LangRole) -> &'static str {
    let marker = format!("#[lang = \"{}\"]", role.name());
    let attribute = CORE_SOURCE
        .find(&marker)
        .expect("core language role source");
    let start = CORE_SOURCE[..attribute]
        .rfind("\n\n")
        .map_or(0, |offset| offset + 2);
    let text = &CORE_SOURCE[start..];
    let end = text.find("\n\n").unwrap_or(text.len());
    text[..end].trim_end()
}
