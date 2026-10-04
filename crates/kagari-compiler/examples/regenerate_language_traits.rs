//! Regenerate the checked executable declaration product from handwritten core source.
use bincode::serialize;
use kagari_compiler::source::lower::module_contract;
use kagari_contract::types::PublicItem;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::{source::SourceFile, source_database::SourceSnapshot};
use kagari_stdlib::catalog as foundation_catalog;
use kagari_types::{language, language::role::LangRole};
use std::{collections::BTreeMap, env, fs, path::Path, sync::Arc};

fn main() {
    let source = Arc::new(SourceFile::new(
        "memory://language-product.kgr",
        "fn main() {}",
    ));
    let snapshot = analysis_database()
        .snapshot(SourceSnapshot::single_file(source), &Default::default())
        .unwrap();
    let mut contracts = BTreeMap::new();
    for role in LangRole::ALL {
        let owner = language::identity(role.protocol()).module;
        if contracts.contains_key(&owner) {
            continue;
        }
        let file = snapshot.module_graph().node(&owner).unwrap().file;
        let checked = snapshot
            .file(file)
            .unwrap()
            .result()
            .clone()
            .into_codegen()
            .expect("core source must pass declaration, role and body checking");
        contracts.insert(
            owner,
            module_contract(&checked, &Default::default()).unwrap(),
        );
    }
    let traits = LangRole::ALL
        .into_iter()
        .map(|role| {
            contracts[&language::identity(role.protocol()).module]
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

fn analysis_database() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(foundation_catalog::shared());
    database
}
