//! Source-enabled runs emit then decode. The independent consumer only reads producer bytes.
#[cfg(feature = "source")]
use crate::standard_library;
use kagari::{
    engine::{EngineConfig, KagariEngine},
    program::Program,
};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, CallTarget},
    module::{BytecodeFunction, BytecodeModule},
    program::{BytecodeProgram, ModuleRef, identity, verify_program},
    verifier::BytecodeVerificationError,
};
use kagari_common::identity::{
    DefinitionKind,
    map::DefinitionContext,
    metadata::DefinitionMetadata,
    table::{DefinitionId, DefinitionTable},
};
use kagari_contract::types::{InterfaceTable, PublicItem, named::NamedContract};
use kagari_runtime::module::entry::ProgramEntry;
#[cfg(feature = "source")]
use kagari_source::source::SourceFile;

use kagari_types::{language::Protocol, scalar::BuiltinType, ty::Ty};
use std::sync::OnceLock;
#[cfg(not(feature = "source"))]
use std::{fs, path::Path};

#[cfg(feature = "source")]
fn fixture_bytes() -> Vec<u8> {
    let source = include_str!(
        "../fixtures/language/conversion_traits/reverse-conversions-call-the-destination-implementation.kgr"
    );
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("reverse-conversions.kgr", source),
            Default::default(),
        )
        .unwrap();
    artifact.records().to_bytes(artifact.definitions()).unwrap()
}

#[cfg(not(feature = "source"))]
fn fixture_bytes() -> Vec<u8> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/reverse_conversions.kbc");
    fs::read(path).expect("emit reverse_conversions.kbc with regenerate_feature_artifact first")
}

fn artifact() -> DefinitionMetadata<KbcArtifact> {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    KbcArtifact::from_bytes(BYTES.get_or_init(fixture_bytes)).unwrap()
}

#[test]
fn decoded_reverse_conversions_execute_once_with_exact_payloads() {
    let program =
        Program::from_artifact(artifact(), &Default::default(), &Default::default()).unwrap();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    #[cfg(feature = "source")]
    let library = standard_library();
    #[cfg(not(feature = "source"))]
    let library = {
        let library = crate::standard::from_fixture("reverse_conversions.kbc");
        let bytecode = program.bytecode();
        let application = &bytecode.modules()[bytecode.root().index()].identity;
        let catalog = library.declarations().catalog();
        assert!(catalog.authored_origin(application).is_none());
        library
    };
    let engine = KagariEngine::with_native_modules(config, library.modules().unwrap()).unwrap();
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    for entry in [ProgramEntry::Main, ProgramEntry::Export("checked_scalars")] {
        assert_eq!(
            runtime
                .execute(&loaded, entry, &[], &Default::default())
                .unwrap()
                .return_value
                .as_i32(),
            Some(42),
            "{entry:?}"
        );
    }
}

fn implementation(module: &BytecodeModule, interface: DefinitionId) -> &InterfaceTable {
    module.public_items.iter().find_map(|item| match item {
        PublicItem::InterfaceTable(table) if matches!(&table.trait_type, Ty::Trait(t) if t.declaration == interface) => Some(table.as_ref()),
        _ => None,
    }).expect("missing ordinary conversion implementation")
}

fn callee<'a>(
    program: &'a BytecodeProgram,
    owner: ModuleRef,
    caller: &BytecodeFunction,
    expected: DefinitionId,
) -> (ModuleRef, &'a BytecodeFunction) {
    caller
        .instructions
        .iter()
        .find_map(|op| {
            let BytecodeInstruction::Call { callee, .. } = op else {
                return None;
            };
            let (owner, function) = match callee {
                CallTarget::Function(f) => (owner, *f),
                CallTarget::ModuleFunction { module, function } => (*module, *function),
                _ => return None,
            };
            let f = &program.modules[owner.index()].functions[function.index()];
            let id = f.identity.as_ref()?.declaration;
            let origin = program.modules[owner.index()]
                .function_origins
                .iter()
                .find(|o| o.placement == id)
                .map_or(id, |o| o.authored);
            (origin == expected).then_some((owner, f))
        })
        .unwrap_or_else(|| {
            panic!(
                "{} does not call the selected method {expected:?}",
                caller.name
            )
        })
}

fn interface(defs: &DefinitionTable, name: &str) -> DefinitionId {
    let owner = defs.lookup_root(&Protocol::From.module()).unwrap();
    defs.lookup_child(owner, DefinitionKind::Trait, name, 0)
        .unwrap()
}

#[test]
fn reverse_calls_select_the_core_blanket_then_the_user_method() {
    let artifact = artifact();
    let defs = artifact.definitions();
    let program = &artifact.records().program;
    let core = program
        .modules
        .iter()
        .find(|m| m.identity == Protocol::From.module())
        .unwrap();
    let root = &program.modules[program.root.index()];
    for (caller, reverse, reverse_method, forward, forward_method) in [
        ("convert", "Into", "into", "From", "from"),
        ("attempt", "TryInto", "try_into", "TryFrom", "try_from"),
    ] {
        let blanket = implementation(core, interface(defs, reverse));
        let user = implementation(root, interface(defs, forward));
        let method = |table: &InterfaceTable, name| {
            defs.lookup_child(table.declaration, DefinitionKind::Method, name, 0)
                .unwrap()
        };
        let owner = defs.lookup_root(&root.identity).unwrap();
        let caller = defs
            .lookup_child(owner, DefinitionKind::Function, caller, 0)
            .unwrap();
        let main = root.functions.iter().find(|f| f.name == "main").unwrap();
        let (owner, caller) = callee(program, program.root, main, caller);
        let (owner, body) = callee(program, owner, caller, method(blanket, reverse_method));
        callee(program, owner, body, method(user, forward_method));
    }
}

#[test]
fn missing_or_replaced_try_into_evidence_is_not_supplied_by_try_from() {
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact()
        .import_into(&context, &Default::default())
        .unwrap();
    let program = &artifact.records().program;
    let definitions = identity::collect(
        &program.modules.iter().collect::<Vec<_>>(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let defs = &definitions;
    verify_program(program, defs, &Default::default()).unwrap();
    let core = program
        .modules
        .iter()
        .position(|m| m.identity == Protocol::From.module())
        .unwrap();
    let id = implementation(&program.modules[core], interface(defs, "TryInto")).declaration;
    for missing in [true, false] {
        let mut changed = program.clone();
        let owner = &mut changed.modules[core];
        // Alter both descriptions of the header, leaving the user's forward impl untouched.
        let rewrite = |ty: &mut Ty| {
            let Ty::Trait(t) = ty else {
                panic!("trait header")
            };
            let error = defs
                .lookup_child(t.declaration, DefinitionKind::AssociatedType, "Error", 0)
                .unwrap();
            let output = t.associated_types.get_mut(&error).unwrap();
            assert!(matches!(output, Ty::Projection { .. }));
            *output = Ty::Builtin(BuiltinType::Bool);
        };
        owner.public_items.retain_mut(|item| {
            if let PublicItem::InterfaceTable(t) = item
                && t.declaration == id
            {
                if missing {
                    return false;
                }
                rewrite(&mut t.trait_type);
            }
            true
        });
        owner.named_declarations.retain_mut(|item| {
            if item.declaration == id {
                if missing {
                    return false;
                }
                let NamedContract::Implementation(h) = &mut item.contract else {
                    panic!("impl header")
                };
                rewrite(h.trait_type.as_mut().unwrap());
            }
            true
        });
        let error = verify_program(&changed, defs, &Default::default()).unwrap_err();
        assert!(
            matches!(
                error,
                BytecodeVerificationError::InvalidPublicAbi
                    | BytecodeVerificationError::InvalidInterfaceTable
            ),
            "missing={missing}: {error}"
        );
    }
}
