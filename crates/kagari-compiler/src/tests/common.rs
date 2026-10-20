use crate::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_bytecode::program::{BytecodeProgram, identity};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, ModuleIdentity,
    map::DefinitionContext,
    metadata::DefinitionMetadata,
    scoped::RemapDefinitions,
    table::{DefinitionId, DefinitionTable},
};
use kagari_hir::{
    CheckedAnalysis, analysis::AnalysisDatabase, analyze_source, program::CheckedProgram,
};
use kagari_mir::{
    function::MirModule,
    program::{VerifiedMirProgram, verify_program},
    verify::{MirVerificationError, VerifiedMirModule, verify_mir},
};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::{catalog as foundation_catalog, identity as library, namespaces};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, module::ImplDecl},
    scalar::BuiltinType,
    ty::Ty,
};

/// Resolve the provider's checked target by semantic contract, never impl occurrence.
fn standard_method_binding(
    owner: ModuleIdentity,
    name: &str,
    matches: impl Fn(&ImplDecl, &FnDecl, &DefinitionTable) -> bool,
) -> DefinitionPath {
    let providers = foundation_catalog::declarations().unwrap();
    let provider = providers
        .iter()
        .find(|module| module.records().identity == owner)
        .unwrap();
    let definitions = provider.definitions();
    let mut candidates = Vec::new();
    for implementation in &provider.records().implementations {
        for method in &implementation.methods {
            if method.name == name && matches(implementation, method, definitions) {
                let CallableImplementation::Native(binding) = method.implementation else {
                    panic!("expected a direct native binding for {owner}::{name}");
                };
                let path = definitions.resolve(binding).unwrap().to_path();
                assert_eq!(path.module, owner);
                candidates.push(path);
            }
        }
    }
    assert_eq!(
        candidates.len(),
        1,
        "unique semantic target for {owner}::{name}"
    );
    candidates.pop().unwrap()
}

pub fn standard_array_index_binding() -> DefinitionPath {
    standard_method_binding(
        namespaces::module("core", "array"),
        "index",
        |implementation, method, definitions| {
            let [parameter] = implementation.generic_params.as_slice() else {
                return false;
            };
            let item = parameter.as_type();
            let Some(interface) = &implementation.trait_type else {
                return false;
            };
            if implementation.for_type != Ty::Array(Box::new(item.clone()))
                || definitions
                    .resolve(interface.declaration)
                    .unwrap()
                    .to_path()
                    != library::trait_id("Index")
                || interface.arguments != [Ty::Builtin(BuiltinType::U64)]
            {
                return false;
            }
            let output = definitions
                .lookup_child(
                    interface.declaration,
                    DefinitionKind::AssociatedType,
                    "Output",
                    0,
                )
                .unwrap();
            assert_eq!(interface.associated_types.len(), 1);
            assert_eq!(interface.associated_types[&output], item);
            assert_eq!(method.return_type, item);
            true
        },
    )
}

pub fn standard_vec_binding(name: &str) -> DefinitionPath {
    let contract = match name {
        "push" => None,
        "pop" => Some("MutableList"),
        "len" => Some("List"),
        _ => panic!("unexpected Vec member"),
    };
    standard_method_binding(
        namespaces::module("alloc", "vec"),
        name,
        |implementation, method, definitions| {
            let [parameter] = implementation.generic_params.as_slice() else {
                return false;
            };
            let Ty::NativeObject(receiver) = &implementation.for_type else {
                return false;
            };
            if definitions.resolve(receiver.declaration).unwrap().to_path()
                != library::type_id("Vec", DefinitionKind::AssociatedType)
                || receiver.arguments != [parameter.as_type()]
                || !receiver.associated_types.is_empty()
            {
                return false;
            }
            let matches_interface = match (contract, &implementation.trait_type) {
                (None, None) => true,
                (Some(name), Some(interface)) => {
                    definitions
                        .resolve(interface.declaration)
                        .unwrap()
                        .to_path()
                        == library::trait_id(name)
                        && interface.arguments == [parameter.as_type()]
                        && interface.associated_types.is_empty()
                }
                _ => false,
            };
            if matches_interface && name == "push" {
                assert_eq!(
                    method.return_type, implementation.for_type,
                    "inherent Vec::push must be fluent, not the unit-returning capability method"
                );
            }
            matches_interface
        },
    )
}

pub fn analyze_ok(text: &str) -> Box<CheckedAnalysis> {
    let source = SourceFile::new("test.kg", text);

    Box::new(
        analyze_source(&source, foundation_catalog::shared().unwrap())
            .expect("installed declaration analysis")
            .into_codegen()
            .expect("analysis should succeed"),
    )
}

pub fn program_ok(text: &str) -> CheckedProgram {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("test.kg", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analysis_database()
        .snapshot(sources.snapshot(), &Default::default())
        .expect("analysis snapshot should succeed");
    snapshot
        .check_program(root, &Default::default())
        .expect("program analysis should succeed")
}

pub fn mir_ok(text: &str) -> VerifiedMirProgram {
    let checked = program_ok(text);
    lower_program_to_mir(&checked, &Default::default()).expect("program IR lowering should succeed")
}

pub fn bytecode_ok(text: &str) -> DefinitionMetadata<BytecodeProgram> {
    lower_program_to_bytecode(&mir_ok(text), &Default::default())
        .map(|program| program.into_unverified())
        .expect("program bytecode lowering should succeed")
}

/// Module transformation tests revalidate the edited root within its original
/// checked dependency closure, including cross-module contracts and native proofs.
pub fn bytecode_with_edited_root(
    checked: &CheckedProgram,
    edited: &VerifiedMirModule,
) -> DefinitionMetadata<BytecodeProgram> {
    let original = lower_program_to_mir(checked, &Default::default())
        .expect("program MIR lowering should succeed");
    assert_eq!(original.root(), &edited.identity);
    let root = original.root().clone();
    let context = DefinitionContext::new().unwrap();
    let mut members = original
        .into_unverified()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let slot = members
        .modules
        .iter()
        .position(|member| member.identity == root)
        .unwrap();
    members.modules[slot] = edited
        .clone()
        .into_unverified()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let verified = verify_program(members, &context, &Default::default())
        .expect("edited program should verify");
    lower_program_to_bytecode(&verified, &Default::default())
        .map(|program| program.into_unverified())
        .expect("edited program bytecode lowering should succeed")
}

fn analysis_database() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(foundation_catalog::shared().unwrap());
    database
}

/// Adopt editable fixture records into the context retained by the calling test.
/// This preserves reference validation; the test still invokes the real verifier.
pub fn adopt<T: RemapDefinitions>(
    context: &DefinitionContext,
    metadata: DefinitionMetadata<T>,
) -> T {
    metadata
        .import_into(context, &Default::default())
        .unwrap()
        .into_records()
}

/// Build a deliberately altered authoring identity for verifier rejection fixtures.
/// Only the selected reference changes; the semantic record still stores an ID.
pub fn edit_definition(
    context: &DefinitionContext,
    id: &mut DefinitionId,
    edit: impl FnOnce(&mut DefinitionPath),
) {
    let mut path = context.snapshot().resolve(*id).unwrap().to_path();
    edit(&mut path);
    *id = context.intern(&path).unwrap();
}

/// Register the implicit ABI names needed by source-free bytecode verification.
pub fn adopt_bytecode(
    context: &DefinitionContext,
    metadata: DefinitionMetadata<BytecodeProgram>,
) -> BytecodeProgram {
    let program = adopt(context, metadata);
    identity::collect(
        &program.modules.iter().collect::<Vec<_>>(),
        context,
        &Default::default(),
    )
    .unwrap();
    program
}

/// Recheck an edited MIR fixture with its retained definition scope.
pub fn verify_edited(
    context: &DefinitionContext,
    module: MirModule,
) -> Result<VerifiedMirModule, MirVerificationError> {
    let metadata =
        DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap();
    verify_mir(metadata, &Default::default())
}
