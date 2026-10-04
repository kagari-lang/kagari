//! Handwritten core traits, combined with native declarations in their owning modules.
use kagari_contract::language::{self, role::LangRole};

pub fn module_source(path: &str) -> (&'static str, &'static str) {
    macro_rules! source {
        ($name:literal) => {
            (
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../library/core/",
                    $name,
                    ".kgr"
                ),
                include_str!(concat!("../../../../library/core/", $name, ".kgr")),
            )
        };
    }
    match path {
        "iter" => source!("iter"),
        "cmp" => source!("cmp"),
        "hash" => source!("hash"),
        "ops" => source!("ops"),
        "fmt" => source!("fmt"),
        "convert" => source!("convert"),
        _ => panic!("unknown handwritten language module"),
    }
}

pub(crate) fn trait_source(role: LangRole) -> &'static str {
    let module = language::identity(role.protocol()).module;
    let (_, source) = module_source(&module.path[0]);
    let marker = format!("#[lang = \"{}\"]", role.name());
    let attribute = source.find(&marker).expect("core language role source");
    let start = source[..attribute]
        .rfind("\n\n")
        .map_or(0, |offset| offset + 2);
    let text = &source[start..];
    let end = text.find("\n\n").unwrap_or(text.len());
    text[..end].trim_end()
}
