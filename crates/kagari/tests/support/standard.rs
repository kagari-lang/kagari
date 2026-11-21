//! Share only the immutable standard bundle; each test installs its own providers.
use kagari_stdlib::StandardLibrary;
#[cfg(feature = "source")]
use std::sync::LazyLock;

#[cfg(feature = "source")]
use super::KagariEngine;

#[cfg(feature = "source")]
pub fn standard_library() -> &'static StandardLibrary {
    static LIBRARY: LazyLock<StandardLibrary> = LazyLock::new(|| {
        let builder = KagariEngine::builder().unwrap();
        StandardLibrary::new(builder.declarations()).unwrap()
    });
    &LIBRARY
}

#[cfg(not(feature = "source"))]
#[path = "portable_standard.rs"]
mod portable_standard;

#[cfg(not(feature = "source"))]
pub fn from_fixture(name: &str) -> StandardLibrary {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/fixtures")
        .join(name);
    portable_standard::from_bytes(&std::fs::read(path).expect("emit the portable fixture first"))
        .unwrap()
}
