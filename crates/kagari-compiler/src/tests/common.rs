use crate::tests::standard::{io_module, standard_modules};
use crate::{
    bytecode::lower_program_to_bytecode,
    source::{
        lower::{MirLoweringError, instances::MirLoweringOptions},
        program::{SourceProgramError, lower_program_to_mir},
    },
};
use kagari_bytecode::program::{BytecodeProgram, identity, updates as bytecode_updates};
use kagari_common::identity::{
    DefinitionPath, map::DefinitionContext, metadata::DefinitionMetadata, scoped::RemapDefinitions,
    table::DefinitionId,
};
use kagari_hir::{
    analysis::AnalysisDatabase, compile_input::program::CheckedCompilation, program::CheckedProgram,
};
use kagari_mir::{
    function::MirModule,
    program::{VerifiedMirProgram, updates as mir_updates, verify_program},
    verify::{MirVerificationError, VerifiedMirModule, verify_mir},
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::declaration::module::ModuleDecl;
use std::sync::Arc;

/* stdlib test support paused (NC01-NC06).
use kagari_common::identity::{DefinitionKind, ModuleIdentity, table::DefinitionTable};
use kagari_stdlib::{identity as library, namespaces};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, module::ImplDecl},
    ty::Ty,
};

/// Resolve the provider's checked target by semantic contract, never impl occurrence.
fn standard_method_binding(
    owner: ModuleIdentity,
    name: &str,
    matches: impl Fn(&ImplDecl, &FnDecl, &DefinitionTable) -> bool,
) -> DefinitionPath {
    let providers = standard_modules();
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
} */

pub fn program_ok(text: &str) -> CheckedProgram {
    program_with_modules(text, standard_modules())
}

pub fn program_ok_with_io(text: &str) -> CheckedProgram {
    let mut modules = standard_modules();
    modules.push(Arc::new(io_module().to_declaration()));
    program_with_modules(text, modules)
}

fn program_with_modules(
    text: &str,
    modules: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
) -> CheckedProgram {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("test.kg", text.into(), SourceLayer::Base)
        .unwrap();
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(modules);
    let snapshot = database
        .snapshot(sources.snapshot(), &Default::default())
        .expect("analysis snapshot should succeed");
    snapshot
        .check_program(root, &Default::default())
        .expect("program analysis should succeed")
}

/// Admit exactly the dependency closure analyzed by a test fixture.
pub fn compilation(program: &CheckedProgram) -> CheckedCompilation<'_> {
    CheckedCompilation::from_program(program, &Default::default()).unwrap()
}

/// Extract a verified root for MIR mutation/assertion tests after compiling its closure.
pub fn lower_root(
    program: &CheckedProgram,
    options: &MirLoweringOptions,
) -> Result<VerifiedMirModule, MirLoweringError> {
    options
        .cancel
        .check()
        .map_err(|_| MirLoweringError::Cancelled)?;
    let compiled =
        lower_program_to_mir(&compilation(program), options).map_err(|error| match error {
            SourceProgramError::Lowering { error, .. } => error,
            error => panic!("program verification failed: {error:?}"),
        })?;
    Ok(compiled
        .modules()
        .iter()
        .find(|module| &module.identity == compiled.root())
        .unwrap()
        .clone())
}

pub fn mir_ok(text: &str) -> VerifiedMirProgram {
    mir_from_checked(&program_ok(text))
}

pub fn mir_ok_with_io(text: &str) -> VerifiedMirProgram {
    mir_from_checked(&program_ok_with_io(text))
}

fn mir_from_checked(checked: &CheckedProgram) -> VerifiedMirProgram {
    lower_program_to_mir(&compilation(checked), &Default::default())
        .expect("program IR lowering should succeed")
}

pub fn bytecode_ok(text: &str) -> DefinitionMetadata<BytecodeProgram> {
    bytecode_from_mir(&mir_ok(text))
}

pub fn bytecode_ok_with_io(text: &str) -> DefinitionMetadata<BytecodeProgram> {
    bytecode_from_mir(&mir_ok_with_io(text))
}

fn bytecode_from_mir(mir: &VerifiedMirProgram) -> DefinitionMetadata<BytecodeProgram> {
    lower_program_to_bytecode(mir, &Default::default())
        .map(|program| program.into_unverified())
        .expect("program bytecode lowering should succeed")
}

/// Module transformation tests revalidate the edited root within its original
/// checked dependency closure, including cross-module contracts and native proofs.
pub fn bytecode_with_edited_root(
    checked: &CheckedProgram,
    edited: &VerifiedMirModule,
) -> DefinitionMetadata<BytecodeProgram> {
    let original = lower_program_to_mir(&compilation(checked), &Default::default())
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
    members.update_evidence = mir_updates::refresh(
        &members.modules,
        &members.update_evidence,
        &context.snapshot(),
        &Default::default(),
    )
    .expect("edited executable evidence should project");
    let verified = verify_program(members, &context, &Default::default())
        .expect("edited program should verify");
    bytecode_from_mir(&verified)
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

/// Project deliberate executable edits before testing an independent boundary.
pub fn project_bytecode(context: &DefinitionContext, program: &mut BytecodeProgram) {
    program.update_evidence = bytecode_updates::project(
        &program.modules,
        &program.update_evidence,
        &context.snapshot(),
        &Default::default(),
    )
    .expect("edited fixture evidence");
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
