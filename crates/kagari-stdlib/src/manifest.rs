/// One exact source in the installed package. Construction is private so callers
/// cannot substitute user input while retaining installed-package provenance.
#[derive(Debug, Clone, Copy)]
pub struct BundledSource {
    pub(crate) module: &'static str,
    pub(crate) uri: &'static str,
    pub(crate) text: &'static str,
}

impl BundledSource {
    pub fn module(&self) -> &'static str {
        self.module
    }
    pub fn uri(&self) -> &'static str {
        self.uri
    }
    pub fn text(&self) -> &'static str {
        self.text
    }
}

macro_rules! source {
    ($module:literal) => {
        BundledSource {
            module: $module,
            uri: concat!("kagari://std/", $module, ".kgr"),
            text: include_str!(concat!("../../../stdlib/", $module, ".kgr")),
        }
    };
}

/// Deterministic manifest order, independent of the current working directory.
pub fn bundled_sources() -> &'static [BundledSource] {
    &[
        source!("std"),
        source!("prelude"),
        source!("map"),
        source!("set"),
        source!("string"),
        source!("option"),
        source!("result"),
        source!("iter"),
        source!("debug"),
        source!("cmp"),
        source!("hash"),
        source!("fmt"),
        source!("convert"),
        source!("numeric"),
    ]
}
