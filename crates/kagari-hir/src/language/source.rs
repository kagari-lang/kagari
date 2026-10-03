//! Handwritten core source, combined with native declarations in the installed module.
use kagari_contract::language::role::LangRole;

pub const CORE_SOURCE: &str = include_str!("../../../../library/core/language.kgr");

pub(crate) fn trait_source(role: LangRole) -> &'static str {
    let marker = format!("#[lang = \"{}\"]", role.name());
    let start = CORE_SOURCE
        .find(&marker)
        .expect("core language role source");
    let text = &CORE_SOURCE[start..];
    let end = text[marker.len()..]
        .find("#[lang =")
        .map_or(text.len(), |offset| marker.len() + offset);
    text[..end].trim_end()
}
