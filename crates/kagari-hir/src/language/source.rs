//! Handwritten core traits, combined with native declarations in their owning modules.

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
