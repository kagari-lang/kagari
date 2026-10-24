use crate::{
    BytecodeArtifact, context::ExecutionContext, engine::EngineConfig, program::PreparedProgram,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget, ConstructionTarget},
    program::{ModuleRef, identity, verify_program},
};
use kagari_common::identity::{
    ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
    table::DefinitionId,
};
use kagari_contract::{
    callable::{construction::evidence::ConstructionSelection, witness::OperationWitness},
    types::PublicItem,
};
use kagari_runtime::module::entry::ProgramEntry;
use kagari_runtime::value::Value;
use kagari_source::{source::SourceFile, source_database::SourceLayer};
use kagari_stdlib::identity as library;
use kagari_types::{callable::CallableImplementation, scalar::BuiltinType, ty::Ty};
use rstest::rstest;
use std::{fs, path::PathBuf};

use crate::test_support::execution;

mod bound_construction;
mod constraint_members;
mod default_method_contracts;
mod shared_arithmetic;

#[rstest]
fn ordinary_default_methods(
    #[files("**/*.kgr")]
    #[base_dir = "tests/fixtures/language/default_methods"]
    #[files("../../iterable-default-cursor.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read default-method fixture {}: {error}",
            path.display()
        )
    });
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = execution::engine(config);
    let artifact = engine
        .compile_source(SourceFile::new("defaults.kgr", source))
        .unwrap();
    execution::assert_returns_i32(
        &engine,
        artifact,
        &[
            execution::Route::Source,
            execution::Route::Artifact,
            execution::Route::ArtifactJit,
        ],
        42,
    );
}

#[test]
fn imported_defaults_preserve_private_helpers_and_definition_context() {
    let engine = execution::engine(Default::default());
    let mut root = None;
    for (name, source) in [
        (
            "model",
            "pub trait Read<T> { fn read(self) -> T; fn again(self) -> T { helper(self.read()) } } fn helper<T>(x: T) -> T { x }",
        ),
        (
            "root",
            "use pkg::model::Read; struct Holder<T> { val value: T } impl<T> Read<T> for Holder<T> { fn read(self) -> T { self.value } } fn helper(x: i32) -> i32 { 0 } fn boxed<T>(x: Holder<T>) -> Read<T> { x } fn main() -> i32 { boxed(Holder { value: 42 }).again() }",
        ),
    ] {
        let path = format!("mem://{name}");
        engine
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let id = engine
            .set_source(&path, source.into(), SourceLayer::Base)
            .unwrap();
        if name == "root" {
            root = Some(id);
        }
    }
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), root.unwrap(), &Default::default())
        .unwrap();
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    let module = &artifact.records().program.modules[artifact.records().program.root.index()];
    let default = module
        .functions
        .iter()
        .find(|function| {
            function.identity.as_ref().is_some_and(|identity| {
                artifact
                    .definitions()
                    .resolve(identity.declaration)
                    .unwrap()
                    .last()
                    .unwrap()
                    .name
                    == "again"
            })
        })
        .unwrap();
    let origin = default.metadata.debug.source_module.unwrap();
    assert_eq!(
        artifact.records().program.modules[origin.index()]
            .identity
            .path,
        ["model"]
    );
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(
                BytecodeArtifact::from_bytes(
                    &artifact.records().to_bytes(artifact.definitions()).unwrap(),
                )
                .unwrap(),
                &Default::default(),
                &Default::default(),
            )
            .unwrap(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn invalid_default_bodies_are_checked_even_without_an_implementation() {
    let engine = execution::engine(Default::default());
    for source in [
        "trait Bad { fn bad(self) -> i32 { missing } } fn main() {}",
        "trait Bad { fn bad(self) -> i32 { true } } fn main() {}",
        "trait Bad { fn required(self) -> i32; } struct Number {} impl Bad for Number {} fn main() {}",
        "trait Bad { fn bad(self) -> Self { 0 } } fn main() {}",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-default.kgr", source),
                    Default::default()
                )
                .is_err(),
            "accepted {source}"
        );
    }
}

#[test]
fn malformed_default_contracts_and_source_origins_are_rejected() {
    let artifact = execution::engine(Default::default()).compile_to_artifact(SourceFile::new("defaults.kgr", "pub trait Read { fn read(self) -> i32 { 42 } } struct Number {} impl Read for Number {} fn main() -> i32 { Number {}.read() }"),  Default::default()).unwrap();
    let (context, artifact) = editable_artifact(artifact);
    for mutation in 0..5 {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        if mutation < 3 {
            let contract = module
                .public_items
                .iter_mut()
                .find_map(|item| {
                    let PublicItem::Trait(contract) = item else {
                        return None;
                    };
                    Some(contract)
                })
                .unwrap();
            if mutation == 0 {
                contract.methods.push(contract.methods[0].clone());
            } else if mutation == 1 {
                contract.methods[0].name = "missing_implementation".into();
            } else {
                contract.methods[0].implementation = CallableImplementation::Native(
                    context.intern(&library::trait_id("List")).unwrap(),
                );
            }
        } else if mutation == 3 {
            let implementation = module
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
            implementation.methods.clear();
        } else {
            module.functions[0].metadata.debug.source_module = Some(ModuleRef::new(999));
        }
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
fn forged_shared_generic_environments_and_applications_are_rejected_without_source() {
    let artifact = execution::engine(Default::default())
        .compile_to_artifact(
            SourceFile::new(
                "generics.kgr",
                r#"
trait Capture { fn capture<T>(self, value: T) -> fn() -> T { || value } }
struct Number {}
impl Capture for Number {}
fn main() -> i32 { val source: Capture = Number {}; val get = source.capture(42); get() }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let (context, artifact) = editable_artifact(artifact);
    for mutation in 0..5 {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        if mutation < 2 {
            let contract = module
                .functions
                .iter_mut()
                .flat_map(|function| &mut function.instructions)
                .find_map(|instruction| match instruction {
                    BytecodeInstruction::Call {
                        callee: CallTarget::InterfaceMethod { contract, .. },
                        ..
                    } => Some(contract),
                    _ => None,
                })
                .unwrap();
            if mutation == 0 {
                contract.arguments.clear();
            } else {
                contract.arguments[0] = Ty::Builtin(BuiltinType::String);
            }
        } else {
            let function = module
                .functions
                .iter_mut()
                .find(|function| function.metadata.semantic.generic.is_some())
                .unwrap();
            if mutation == 2 {
                function.metadata.semantic.generic = None;
            } else if mutation == 3 {
                function
                    .metadata
                    .semantic
                    .generic
                    .as_mut()
                    .unwrap()
                    .parameters[0]
                    .position += 1;
            } else {
                function.metadata.semantic.params.remove(&1);
            }
        }
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn shared_list_table_mappings_are_checked_without_source() {
    let artifact = execution::engine(Default::default())
        .compile_to_artifact(
            SourceFile::new(
                "shared-list.kgr",
                r#"use std::collections::{List};

trait Wrap { fn wrap<T>(self, value: T) -> List<T> { Vec::from([value]) } }
struct Source {}
impl Wrap for Source {}
fn main() -> i32 { val source: Wrap = Source {}; source.wrap(42)[0] }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let (context, artifact) = editable_artifact(artifact);
    for mutation in 0..6 {
        let mut program = artifact.program.clone();
        if mutation == 0 {
            let arguments = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.functions)
                .flat_map(|function| &mut function.instructions)
                .find_map(|instruction| match instruction {
                    BytecodeInstruction::MakeInterface {
                        target: ConstructionTarget::Declared { arguments, .. },
                        ..
                    } if arguments.iter().any(|ty| !ty.is_concrete()) => Some(arguments),
                    _ => None,
                })
                .unwrap();
            arguments[0] = Ty::Builtin(BuiltinType::String);
        } else if mutation == 5 {
            let view = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.interface_tables)
                .filter(|table| table.arguments.iter().any(|ty| !ty.is_concrete()))
                .find_map(|table| table.view.as_mut())
                .unwrap();
            view.results[0].implementation.arguments.clear();
        } else {
            let table = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.interface_tables)
                .find(|table| {
                    !table.parents.is_empty() && table.arguments.iter().any(|ty| !ty.is_concrete())
                })
                .unwrap();
            match mutation {
                1 => table.methods[0].arguments.clear(),
                2 => table.parents.clear(),
                3 => {
                    table.parents[0].implementation.arguments[0] = Ty::Builtin(BuiltinType::String)
                }
                4 => {
                    let Ty::Parameter { position, .. } = &mut table.arguments[0] else {
                        panic!("shared table binder")
                    };
                    *position += 1;
                }
                _ => unreachable!(),
            }
        }
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err(),
            "mutation {mutation}"
        );
    }
}

const SHARED_RECEIVER_COMPARISON: &str = include_str!(
    "../../../../tests/fixtures/language/default_methods/shared_receiver_implementation_supplies_nongeneric_comparison.kgr"
);

#[test]
fn shared_function_call_arguments_and_operations_are_verified_without_source() {
    let artifact = execution::engine(Default::default())
        .compile_to_artifact(
            SourceFile::new(
                "shared-helper.kgr",
                r#"
fn invoke<F: Fn() -> i32>(callback: F) -> i32 { callback() }
trait Run { fn run<F: Fn() -> i32>(self, callback: F) -> i32 { invoke(callback) } }
struct Source {}
impl Run for Source {}
fn main() -> i32 { val source: Run = Source {}; source.run(|| 42) }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let (context, artifact) = editable_artifact(artifact);
    for mutation in 0..10 {
        let mut program = artifact.program.clone();
        let call = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| match instruction {
                BytecodeInstruction::Call {
                    callee: CallTarget::Shared { contract, .. },
                    ..
                } => Some(contract),
                _ => None,
            })
            .unwrap();
        match mutation {
            0 => call.arguments.clear(),
            1 => call.signature.params[0] = Ty::Builtin(BuiltinType::I32),
            2 => call.operations.clear(),
            3 => call.operations.push(call.operations[0].clone()),
            4 => call.instance.declaration = renamed(&context, call.instance.declaration, "forged"),
            5 => {
                let Ty::Parameter { position, .. } = &mut call.arguments[0] else {
                    panic!("caller binder")
                };
                *position += 1;
            }
            6 => call.constructions.clear(),
            7 => call.constructions.push(call.constructions[0].clone()),
            8 => call.constructions[0].requirement.receiver = Ty::Builtin(BuiltinType::Bool),
            9 => call.constructions[0].selection = ConstructionSelection::Interface,
            _ => unreachable!(),
        }
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn shared_helpers_are_materialized_in_their_declaring_module() {
    let engine = execution::engine(Default::default());
    let mut root = None;
    for (name, source) in [
        (
            "model",
            r#"
use std::hash::Hash;
pub trait Read { fn read(self) -> i32; }
fn retain<T: Read>(value: T) -> fn() -> Read { || value }
fn erase_hash<T: Hash>(value: T) -> Hash { value }
pub trait Keep {
    fn keep<T: Read>(self, value: T) -> fn() -> Read { retain(value) }
    fn hashed<T: Hash>(self, value: T) -> Hash { erase_hash((value, 7)) }
}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Keep, Read};
use std::hash::Hash;
struct Source {}
impl Keep for Source {}
struct Item { val value: i32 }
impl Read for Item { fn read(self) -> i32 { self.value } }
impl PartialEq for Item { fn eq(self, other: Self) -> bool { self.value == other.value } }
impl Eq for Item {}
impl Hash for Item { fn hash(self) -> i64 { 17i64 } }
fn main() -> i32 {
    val source: Keep = Source {};
    val item = Item { value: 42 };
    val get = source.keep(item);
    if source.hashed(item).hash() == (item, 7).hash() { get().read() } else { 0 }
}
"#,
        ),
    ] {
        let path = format!("mem://{name}");
        engine
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let revision = engine
            .set_source(&path, source.into(), SourceLayer::Base)
            .unwrap();
        if name == "root" {
            root = Some(revision);
        }
    }
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), root.unwrap(), &Default::default())
        .unwrap();
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    let decoded =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let program =
        PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn generic_interface_upcasts_reject_forged_scopes_and_parents() {
    let artifact = execution::engine(Default::default())
        .compile_to_artifact(
            SourceFile::new(
                "shared-upcast.kgr",
                r#"use std::collections::{List, MutableList};

trait Wrap {
    fn wrap<T>(self, value: T) -> List<T> {
        val mutable: MutableList<T> = Vec::from([value]);
        mutable
    }
}
struct Source {}
impl Wrap for Source {}
fn main() -> i32 { val source: Wrap = Source {}; source.wrap(42)[0] }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let (context, artifact) = editable_artifact(artifact);
    for mutation in 0..4 {
        let mut program = artifact.program.clone();
        let (source, target) = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| match instruction {
                BytecodeInstruction::UpcastInterface { source, target, .. } => {
                    Some((source, target))
                }
                _ => None,
            })
            .unwrap();
        match mutation {
            0 => {
                let Ty::Parameter { position, .. } = &mut target.arguments[0] else {
                    panic!("shared binder")
                };
                *position += 1;
            }
            1 => target.arguments[0] = Ty::Builtin(BuiltinType::String),
            2 => target.arguments.clear(),
            3 => std::mem::swap(source, target),
            _ => unreachable!(),
        }
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn ordinary_interface_methods_require_receiver_bound_witnesses() {
    let artifact = execution::engine(Default::default())
        .compile_to_artifact(
            SourceFile::new(
                "receiver-bound.kgr",
                r#"
trait Reader<T: Ord> {
    fn lesser(self, left: T, right: T) -> T { if left < right { left } else { right } }
}
struct Source {}
impl<T: Ord> Reader<T> for Source {}
trait Choose {
    fn choose<T: Ord>(self, left: T, right: T) -> T {
        val reader: Reader<T> = Source {};
        reader.lesser(left, right)
    }
}
impl Choose for Source {}
fn main() -> i32 { val chooser: Choose = Source {}; chooser.choose(42, 43) }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let (context, artifact) = editable_artifact(artifact);
    let mut program = artifact.program;
    let call = program
        .modules
        .iter_mut()
        .flat_map(|module| &mut module.functions)
        .flat_map(|function| &mut function.instructions)
        .find_map(|instruction| match instruction {
            BytecodeInstruction::Call {
                callee: CallTarget::InterfaceMethod { contract, .. },
                ..
            } if contract.arguments.is_empty() && !contract.operations.is_empty() => Some(contract),
            _ => None,
        })
        .unwrap();
    call.operations.clear();
    assert!(
        BytecodeArtifact::from_program(program, Default::default(), &context, &Default::default())
            .is_err()
    );
}

#[test]
fn shared_nominal_layout_applications_are_verified_without_source() {
    let artifact = execution::engine(Default::default())
        .compile_to_artifact(
            SourceFile::new(
                "generic-layout.kgr",
                r#"
struct Box<T> { var value: T }
trait Wrap {
    fn wrap<T>(self, value: T) -> Box<T> { Box { value: value } }
    fn read<T>(self, value: Box<T>) -> T { value.value }
}
struct Source {}
impl Wrap for Source {}
fn main() -> i32 { val source: Wrap = Source {}; source.read(source.wrap(42)) }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let (context, artifact) = editable_artifact(artifact);
    for mutation in 0..6 {
        let mut program = artifact.program.clone();
        if mutation < 2 {
            let layout = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.structures)
                .find(|layout| layout.arguments.iter().any(|ty| !ty.is_concrete()))
                .unwrap();
            let ty = if mutation == 0 {
                &mut layout.arguments[0]
            } else {
                &mut layout.fields[0].ty
            };
            let Ty::Parameter { position, .. } = ty else {
                panic!("layout binder")
            };
            *position += 1;
        } else if mutation == 5 {
            let layout = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.structures)
                .find(|layout| {
                    !layout.arguments.is_empty() && layout.arguments.iter().all(Ty::is_concrete)
                })
                .unwrap();
            layout.fields[0].ty = Ty::Builtin(BuiltinType::String);
        } else {
            let arguments = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.functions)
                .flat_map(|function| &mut function.instructions)
                .find_map(|instruction| match instruction {
                    BytecodeInstruction::MakeStruct { arguments, .. }
                        if mutation != 4 && arguments.iter().any(|ty| !ty.is_concrete()) =>
                    {
                        Some(arguments)
                    }
                    BytecodeInstruction::ReadAggregateField { field, .. }
                        if mutation == 4 && field.arguments.iter().any(|ty| !ty.is_concrete()) =>
                    {
                        Some(&mut field.arguments)
                    }
                    _ => None,
                })
                .unwrap();
            if mutation == 2 {
                arguments.clear();
            } else if mutation == 3 {
                let Ty::Parameter { position, .. } = &mut arguments[0] else {
                    panic!("caller binder")
                };
                *position += 1;
            } else {
                arguments[0] = Ty::Builtin(BuiltinType::String);
            }
        }
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err(),
            "mutation {mutation}"
        );
    }
}

/// Retain one scope and prove the unmodified input before testing each forgery.
fn editable_artifact(
    artifact: DefinitionMetadata<BytecodeArtifact>,
) -> (DefinitionContext, BytecodeArtifact) {
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let table = identity::collect(
        &artifact.program.modules.iter().collect::<Vec<_>>(),
        &context,
        &Default::default(),
    )
    .unwrap();
    verify_program(&artifact.program, &table, &Default::default()).unwrap();
    (context, artifact)
}

fn renamed(context: &DefinitionContext, id: DefinitionId, name: &str) -> DefinitionId {
    let table = context.snapshot();
    let parent = table.parent(id).unwrap().unwrap();
    let segment = table.resolve(id).unwrap().last().unwrap();
    context
        .intern_child(parent, segment.kind, name, segment.occurrence)
        .unwrap()
}
