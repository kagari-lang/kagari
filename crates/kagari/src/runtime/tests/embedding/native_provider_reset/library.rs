use crate::{context::ExecutionContext, engine::KagariEngine, program::Program};
use kagari_runtime::module::entry::ProgramEntry;
use kagari_source::source::SourceFile;
use {kagari_stdlib as foundation, kagari_stdlib::catalog as foundation_catalog};

#[test]
fn foundation_algorithms_are_available_from_explicit_composition() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), foundation::modules().unwrap())
            .unwrap();
    let sources = engine.native_declaration_sources();
    assert_eq!(sources.len(), foundation_catalog::shared().unwrap().len());
    let program = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://sort.kgr",
                r#"
        fn main() -> i32 { val values = Vec::from([3,1,2]); values.sort(); values[0] }
    "#,
            ),
            Default::default(),
        )
        .unwrap();
    let program =
        Program::from_artifact(program, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .as_i32()
            .unwrap(),
        1
    );
}

#[test]
fn explicit_library_composition_keeps_collection_algorithms() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), foundation::modules().unwrap())
            .unwrap();
    assert_eq!(
        engine.native_declaration_sources().len(),
        foundation_catalog::shared().unwrap().len()
    );
    assert!(
        engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://foundation-sort.kgr",
                    "fn main() { Vec::from([2,1]).sort(); }"
                ),
                Default::default()
            )
            .is_ok()
    );
    let program = engine.compile_to_artifact(SourceFile::new("memory://foundation.kgr", r#"use std::collections::{HashMap, HashSet};

        fn main() -> i32 {
            val values = Vec::from([20,22]); val map: HashMap<i32,i32> = HashMap::new(); map.insert(1, values[0]);
            val set: HashSet<i32> = HashSet::new(); set.insert(values[1]);
            if set.contains(22) && map.contains_key(1) { values[0] + values[1] } else { 0 }
        }
    "#),  Default::default()).unwrap();
    let program =
        Program::from_artifact(program, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .as_i32()
            .unwrap(),
        42
    );
}

#[test]
fn application_installation_rejects_duplicate_explicit_providers() {
    let module = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("std", "collections")
        })
        .unwrap();
    let mut builder = KagariEngine::builder().unwrap();
    builder.install_all(foundation::modules().unwrap()).unwrap();
    assert!(builder.install(module).is_err());
}
