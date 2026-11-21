use super::{compile, insert};
use crate::test_support::standard::standard_library;
use crate::{BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::Program};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    module::BytecodeFunction,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{DefinitionKind, table::DefinitionId};
use kagari_contract::types::named::NamedContract;
use kagari_runtime::module::entry::ProgramEntry;
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn direct_callee<'a>(
    program: &'a BytecodeProgram,
    caller_module: ModuleRef,
    caller: &BytecodeFunction,
    expected: DefinitionId,
    arguments: &[Ty],
) -> (ModuleRef, &'a BytecodeFunction) {
    caller
        .instructions
        .iter()
        .find_map(|instruction| {
            let BytecodeInstruction::Call { callee, .. } = instruction else {
                return None;
            };
            let (module, function) = match callee {
                CallTarget::Function(function) => (caller_module, function),
                CallTarget::ModuleFunction { module, function } => (*module, function),
                _ => return None,
            };
            let function = &program.modules[module.index()].functions[function.index()];
            let identity = function.identity.as_ref()?;
            (identity.declaration == expected && identity.arguments == arguments)
                .then_some((module, function))
        })
        .unwrap_or_else(|| {
            panic!(
                "{} did not call {expected:?} with {arguments:?}",
                caller.name
            )
        })
}

#[test]
fn conditional_blanket_methods_execute_for_concrete_and_open_callers() {
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();
    insert(
        &engine,
        "provider",
        include_str!("../../../../tests/fixtures/language/blanket_traits/provider.kgr"),
    );
    let root = insert(
        &engine,
        "caller",
        include_str!("../../../../tests/fixtures/language/blanket_traits/caller.kgr"),
    );
    let artifact = compile(&engine, root);
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let artifact = BytecodeArtifact::from_bytes(&bytes).unwrap();
    let program = &artifact.records().program;
    let definitions = artifact.definitions();
    let provider = program
        .modules
        .iter()
        .find(|m| m.identity.path == ["provider"])
        .unwrap();
    let provider_root = definitions.lookup_root(&provider.identity).unwrap();
    let method = |trait_name, name| {
        let interface = definitions
            .lookup_child(provider_root, DefinitionKind::Trait, trait_name, 0)
            .unwrap();
        provider
            .named_declarations
            .iter()
            .find_map(|item| {
                let NamedContract::Implementation(header) = &item.contract else {
                    return None;
                };
                let Some(Ty::Trait(applied)) = &header.trait_type else {
                    return None;
                };
                if applied.declaration != interface {
                    return None;
                }
                definitions.lookup_child(item.declaration, DefinitionKind::Method, name, 0)
            })
            .unwrap()
    };
    let reverse = method("Reverse", "reverse");
    let forward = method("Forward", "forward");
    let caller = &program.modules[program.root.index()];
    let owner = definitions.lookup_root(&caller.identity).unwrap();
    let open = definitions
        .lookup_child(owner, DefinitionKind::Function, "open", 0)
        .unwrap();
    let main = caller.functions.iter().find(|f| f.name == "main").unwrap();
    let destination = Ty::Builtin(BuiltinType::I32);
    let tag = Ty::Builtin(BuiltinType::Bool);
    let open_source = Ty::Builtin(BuiltinType::U64);
    let (module, open) = direct_callee(
        program,
        program.root,
        main,
        open,
        &[open_source.clone(), destination.clone()],
    );
    for (module, caller, source) in [
        (program.root, main, Ty::Builtin(BuiltinType::I64)),
        (module, open, open_source),
    ] {
        let (module, body) = direct_callee(
            program,
            module,
            caller,
            reverse,
            &[source.clone(), destination.clone(), tag.clone()],
        );
        direct_callee(program, module, body, forward, &[source]);
    }
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}
