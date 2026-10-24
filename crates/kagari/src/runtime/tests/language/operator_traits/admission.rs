use crate::engine::KagariEngine;
use kagari_bytecode::program::{identity, verify_program};
use kagari_common::identity::map::DefinitionContext;
use kagari_contract::types::PublicItem;
use kagari_source::source::SourceFile;
use kagari_types::{language::Protocol, ty::Ty};

#[test]
fn portable_operator_contracts_reject_wrong_inputs_and_outputs() {
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "operator-wire.kgr",
                r#"use std::ops::{Add};

struct Number{val value:i32}
impl Add<i32> for Number {type Output=i32;fn add(self,rhs:i32)->i32 {self.value+rhs}}
fn main()->i32 {Number{value:20}+22}
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let definitions = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&definitions, &Default::default())
        .unwrap()
        .into_records();
    let identity_table = identity::collect(
        &artifact.program.modules.iter().collect::<Vec<_>>(),
        &definitions,
        &Default::default(),
    )
    .unwrap();
    verify_program(&artifact.program, &identity_table, &Default::default()).unwrap();
    for corrupt_input in [false, true] {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        let table = module
            .public_items
            .iter_mut()
            .find_map(|item| {
                let PublicItem::InterfaceTable(table) = item else {
                    return None;
                };
                let Ty::Trait(interface) = &table.trait_type else {
                    return None;
                };
                (Protocol::from_definition(&interface.declaration, &identity_table)
                    == Some(Protocol::Add))
                .then_some(table)
            })
            .unwrap();
        let Ty::Trait(interface) = &mut table.trait_type else {
            unreachable!()
        };
        if corrupt_input {
            interface.arguments.clear();
        } else {
            interface.associated_types.clear();
        }
        assert!(verify_program(&program, &identity_table, &Default::default()).is_err());
    }
}
