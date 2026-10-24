use crate::engine::KagariEngine;
use kagari_bytecode::program::{identity, verify_program};
use kagari_common::identity::{map::DefinitionContext, table::DefinitionId};
use kagari_contract::types::PublicItem;
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn forged_access_upgrades_are_rejected_before_loading() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "access-wire.kgr",
                r#"
pub fn inspect(values: std::collections::List<i32>) { values.len(); }
fn main() { val values = Vec::from([1, 2]); inspect(values); }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let definitions = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&definitions, &Default::default())
        .unwrap();
    identity::collect(
        &artifact
            .records()
            .program
            .modules
            .iter()
            .collect::<Vec<_>>(),
        &definitions,
        &Default::default(),
    )
    .unwrap();
    verify_program(
        &artifact.records().program,
        &definitions.snapshot(),
        &Default::default(),
    )
    .unwrap();
    let root = artifact.records().program.root.index();
    let index = artifact.records().program.modules[root]
        .functions
        .iter()
        .position(|f| f.name == "inspect")
        .unwrap();
    let mut forged = artifact.records().program.clone();
    let function = &mut forged.modules[root].functions[index];
    if let Some(Ty::Trait(interface)) = function.metadata.semantic.params.get_mut(&0) {
        interface.declaration = mutable_list(&definitions, interface.declaration);
    }
    assert!(verify_program(&forged, &definitions.snapshot(), &Default::default()).is_err());
    let mut forged = artifact.records().program.clone();
    forged.modules[root].functions[index]
        .metadata
        .semantic
        .params
        .clear();
    assert!(verify_program(&forged, &definitions.snapshot(), &Default::default()).is_err());
    let mut forged = artifact.into_records();
    let table = forged.program.modules.iter_mut().flat_map(|module| &mut module.public_items).find_map(|item| match item {
        PublicItem::InterfaceTable(table) if matches!(&table.trait_type, Ty::Trait(interface) if definitions.snapshot().resolve(interface.declaration).unwrap().last().unwrap().name == "List") => Some(table),
        _ => None,
    }).unwrap();
    if let Ty::Trait(interface) = &mut table.trait_type {
        interface.declaration = mutable_list(&definitions, interface.declaration);
    }
    assert!(
        forged
            .validate_for_loader(&Default::default(), &definitions, &Default::default())
            .is_err()
    );
}

#[test]
fn boxed_interface_dispatch_requires_an_interface_value_without_collection_markers() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "ordinary-interface-receiver.kgr",
                r#"
trait Reader { fn read(self) -> i32; }
pub fn inspect(value: Reader) -> i32 { value.read() }
fn main() -> i32 { 42 }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact.import_into(&context, &Default::default()).unwrap();
    identity::collect(
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
    verify_program(
        &artifact.records().program,
        &context.snapshot(),
        &Default::default(),
    )
    .unwrap();
    let mut program = artifact.records().program.clone();
    let root = program.root.index();
    let function = program.modules[root]
        .functions
        .iter_mut()
        .find(|function| function.name == "inspect")
        .unwrap();
    let original = function.metadata.semantic.params[&0].clone();
    let replacement = Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32)]);
    let semantic = &mut function.metadata.semantic;
    for ty in semantic
        .params
        .values_mut()
        .chain(semantic.slots.values_mut())
    {
        if *ty == original {
            *ty = replacement.clone();
        }
    }
    let declaration = program.modules[root]
        .public_items
        .iter_mut()
        .find_map(|item| match item {
            PublicItem::Function(function) if function.name == "inspect" => Some(function),
            _ => None,
        })
        .unwrap();
    declaration.params[0].ty = replacement;
    assert!(
        verify_program(&program, &context.snapshot(), &Default::default()).is_err(),
        "a raw heap value cannot satisfy boxed-interface dispatch by sharing its physical representation"
    );
}

fn mutable_list(definitions: &DefinitionContext, id: DefinitionId) -> DefinitionId {
    let table = definitions.snapshot();
    let parent = table.parent(id).unwrap().unwrap();
    let segment = table.resolve(id).unwrap().last().unwrap();
    definitions
        .intern_child(parent, segment.kind, "MutableList", segment.occurrence)
        .unwrap()
}
