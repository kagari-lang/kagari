use crate::{BytecodeArtifact, engine::KagariEngine};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_contract::types::PublicItem;
use kagari_source::source::SourceFile;
use kagari_types::{declaration::AssociatedConstDef, scalar::BuiltinType, ty::Ty};

#[test]
fn malformed_constant_records_and_dynamic_interface_forgery_are_rejected() {
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("constants.kgr", "pub trait Limit { const VALUE: i32; } struct Number {} impl Limit for Number { const VALUE: i32 = 42; } fn main() -> i32 { Number::VALUE }"),  Default::default()).unwrap();
    let definitions = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&definitions, &Default::default())
        .unwrap()
        .into_records();
    BytecodeArtifact::from_program(
        artifact.program.clone(),
        Default::default(),
        &definitions,
        &Default::default(),
    )
    .unwrap();
    for mutation in 0..4 {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        if mutation < 2 {
            let record = module
                .public_items
                .iter_mut()
                .find_map(|item| {
                    if let PublicItem::Trait(record) = item {
                        Some(record)
                    } else {
                        None
                    }
                })
                .unwrap();
            if mutation == 0 {
                record
                    .associated_consts
                    .push(record.associated_consts[0].clone());
            } else {
                record.associated_consts[0].default_value = Some("const-v1:i32:00042".into());
            }
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
            if mutation == 2 {
                table.associated_consts.clear();
            } else {
                table.associated_consts[0].value = "const-v1:i32:2147483648".into();
            }
        }
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &definitions,
                &Default::default()
            )
            .is_err()
        );
    }
    let dynamic = engine.compile_to_artifact(SourceFile::new("dynamic.kgr", "pub trait Read { fn read(self) -> i32; } struct Number {} impl Read for Number { fn read(self) -> i32 { 42 } } fn main() -> i32 { val x: Read = Number {}; x.read() }"),  Default::default()).unwrap();
    let dynamic = dynamic
        .import_into(&definitions, &Default::default())
        .unwrap()
        .into_records();
    BytecodeArtifact::from_program(
        dynamic.program.clone(),
        Default::default(),
        &definitions,
        &Default::default(),
    )
    .unwrap();
    let mut program = dynamic.program;
    let module = &mut program.modules[program.root.index()];
    let identity = module
        .public_items
        .iter()
        .find_map(|item| {
            let PublicItem::InterfaceTable(table) = item else {
                return None;
            };
            let Ty::Trait(interface) = &table.trait_type else {
                return None;
            };
            Some(interface.declaration)
        })
        .unwrap();
    let record = module
        .public_items
        .iter_mut()
        .find_map(|item| {
            if let PublicItem::Trait(record) = item {
                Some(record)
            } else {
                None
            }
        })
        .unwrap();
    record.associated_consts.push(AssociatedConstDef {
        declaration: definitions
            .intern_child(identity, DefinitionKind::Const, "VALUE", 0)
            .unwrap(),
        ty: Ty::Builtin(BuiltinType::I32),
        default_value: Some("const-v1:i32:42".into()),
    });
    assert!(
        BytecodeArtifact::from_program(
            program,
            Default::default(),
            &definitions,
            &Default::default()
        )
        .is_err()
    );
}
