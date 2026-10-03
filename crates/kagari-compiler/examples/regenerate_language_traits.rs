//! Regenerate the checked executable declaration product from handwritten core source.
use bincode::serialize;
use kagari_compiler::source::lower::module_contract;
use kagari_contract::{
    language::{self, role::LangRole},
    types::PublicItem,
};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::{source::SourceFile, source_database::SourceSnapshot};
use std::{env, fs, path::Path, sync::Arc};

fn main() {
    let source = Arc::new(SourceFile::new(
        "memory://language-product.kgr",
        "fn main() {}",
    ));
    let snapshot = AnalysisDatabase::default()
        .snapshot(SourceSnapshot::single_file(source), &Default::default())
        .unwrap();
    let file = snapshot
        .module_graph()
        .node(&language::module_identity())
        .unwrap()
        .file;
    let checked = snapshot
        .file(file)
        .unwrap()
        .result()
        .clone()
        .into_codegen()
        .expect("core source must pass declaration, role and body checking");
    let contract = module_contract(&checked, &Default::default()).unwrap();
    let traits = LangRole::ALL
        .into_iter()
        .map(|role| {
            contract
                .public_items
                .iter()
                .find_map(|item| match item {
                    PublicItem::Trait(item) if item.name == role.protocol().name() => {
                        Some(item.clone())
                    }
                    _ => None,
                })
                .expect("required core declaration")
        })
        .collect::<Vec<_>>();
    let bytes = serialize(&traits).unwrap();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let output = workspace.join("crates/kagari-contract/src/language/traits.bin");
    if env::args().any(|argument| argument == "--check") {
        assert_eq!(
            bytes,
            fs::read(output).unwrap(),
            "core source differs from its checked product"
        );
    } else {
        fs::write(output, bytes).unwrap();
    }
}
