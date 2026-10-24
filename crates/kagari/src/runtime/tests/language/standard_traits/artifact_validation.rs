//! Source-free admission of standard implementation records and their prerequisites.
use crate::{BytecodeArtifact, engine::KagariEngine};
use kagari_bytecode::program::{identity, verify_program};
use kagari_common::identity::map::DefinitionContext;
use kagari_contract::types::PublicItem;
use kagari_source::source::SourceFile;
use kagari_types::{language as standard_traits, language::Protocol, scalar::BuiltinType, ty::Ty};

#[test]
fn malformed_standard_implementations_and_reserved_modules_are_rejected() {
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("format-wire.kgr", "use std::fmt::{Debug};\nstruct Item {} impl Debug for Item { fn debug(self)->String { \"ok\" } } fn main()->i32 { val item=Item {}; item.debug(); 42 }"),Default::default()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact.import_into(&context, &Default::default()).unwrap();
    let table = identity::collect(
        &artifact
            .records()
            .program
            .modules
            .iter()
            .collect::<Vec<_>>(),
        &context,
        &Default::default(),
    )
    .unwrap();
    verify_program(&artifact.records().program, &table, &Default::default()).unwrap();
    for mutation in 0..5 {
        let mut program = artifact.records().program.clone();
        let module = &mut program.modules[program.root.index()];
        if mutation == 0 {
            module.identity.package.0 = "kagari-core".into();
        } else {
            let table = module
                .public_items
                .iter_mut()
                .find_map(|item| {
                    if let PublicItem::InterfaceTable(table) = item {
                        Some(table)
                    } else {
                        None
                    }
                })
                .unwrap();
            match mutation {
                1 => table.methods[0].return_type = Ty::Builtin(BuiltinType::I32),
                2 => {
                    let Ty::Trait(trait_type) = &mut table.trait_type else {
                        panic!("trait");
                    };
                    trait_type.declaration = context
                        .intern(&standard_traits::identity(Protocol::Hash))
                        .unwrap();
                }
                3 => table.methods.clear(),
                _ => {
                    let Ty::Trait(trait_type) = &mut table.trait_type else {
                        panic!("trait");
                    };
                    trait_type.arguments.push(Ty::Builtin(BuiltinType::I32));
                }
            }
        }
        assert!(
            verify_program(&program, &context.snapshot(), &Default::default()).is_err(),
            "mutation {mutation}"
        );
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err()
        );
    }
}

#[test]
fn portable_hash_implementations_require_explicit_comparison_contracts() {
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "key-wire.kgr",
                r#"use std::hash::{Hash};

struct Key {val id:i32}
impl PartialEq for Key {fn eq(self,other:Self)->bool {self.id==other.id}}
impl Eq for Key {}
impl Hash for Key {fn hash(self)->i64 {self.id.hash()}}
fn main()->i64 {Key{id:1}.hash()}
"#,
            ),
            Default::default(),
        )
        .unwrap();
    for missing in [Protocol::Eq, Protocol::PartialEq] {
        let mut program = artifact.records().program.clone();
        let module = &mut program.modules[program.root.index()];
        module.public_items.retain(|item| !matches!(item,PublicItem::InterfaceTable(table) if matches!(&table.trait_type, Ty::Trait(t) if Some(t.declaration)==missing.definition(artifact.definitions()))));
        assert!(
            verify_program(&program, artifact.definitions(), &Default::default()).is_err(),
            "missing {missing:?}"
        );
    }
}
