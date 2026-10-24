//! Public-path evidence survives source-free MIR/KBC decoding and identity adoption.

use crate::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_bytecode::{
    artifact::KbcArtifact,
    program::{identity, verify_program as verify_bytecode},
    verifier::BytecodeVerificationError,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, PackageId,
        map::DefinitionContext,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    exports::{ExportNamespace, TypeExport},
    types::PublicItem,
};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_mir::{
    codec::{decode_program, encode_program},
    program::{ProgramErrorKind, VerifiedMirProgram, records::MirProgram, verify_program},
    verify::{MirVerificationError, MirVerificationErrorKind},
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    declaration::{
        TypeDef, TypeDefKind,
        host::{HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess},
        module::ModuleDecl,
    },
    scalar::BuiltinType,
    ty::Ty,
};
use std::sync::Arc;

fn compiled() -> VerifiedMirProgram {
    let mut source = SourceDatabase::default();
    let root = source
        .set(
            "exports.kgr",
            r#"
        mod hidden {
            pub fn calculate(value: i32) -> i32 { value }
            fn secret() -> i32 { 2 }
            pub fn main() -> i32 { 3 }
            pub struct User { pub val value: i32 }
            pub struct Cell<T> { val value: T }
            pub type IntCell = Cell<i32>;
            pub enum Packet<T> { Data(T) }
            pub trait Marker {}
            pub type Alias = User;
            pub type Erased<T> = User;
        }
        pub mod api { pub fn tick() -> i32 { 4 } }
        pub use crate::hidden::calculate as damage;
        pub use crate::hidden::{User as Model, Packet, Marker, Alias, IntCell, Erased};
        pub use host::api::Token;
        pub type Count = i32;
        pub type Pair = (i32, bool);
        pub type Message = hidden::Packet<i32>;
        pub type Interface = hidden::Marker;
        pub type Handle = Token;
        pub use crate as again;
        pub fn api() -> bool { true }
        pub fn generic<T>(value: T) -> T { value }
        pub fn wrapper() -> i32 { generic(5) }
        fn main() -> i32 { 6 }
    "#
            .into(),
            SourceLayer::Base,
        )
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let mut native = ModuleDecl::new(ModuleIdentity {
        package: PackageId("host".into()),
        path: vec!["api".into()],
    });
    native.types.push(TypeDef {
        name: "Token".into(),
        kind: TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::Opaque,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::None,
        }),
        generic_params: vec![],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    });
    native.collect_definitions(&context).unwrap();
    let native = Arc::new(
        DefinitionMetadata::checked(context.snapshot(), native, &Default::default()).unwrap(),
    );
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![native]);
    let checked = analysis
        .snapshot(source.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    lower_program_to_mir(&checked, &Default::default()).unwrap()
}

fn function(program: &MirProgram, module: &[&str], name: &str) -> DefinitionId {
    program
        .modules
        .iter()
        .find(|owner| {
            owner.identity.package == program.root.package && owner.identity.path == module
        })
        .unwrap()
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap()
        .instance
        .declaration
}

fn assert_type_exports(namespace: &ExportNamespace, definitions: &DefinitionTable) {
    let TypeExport::Declaration(model) = &namespace.types["Model"] else {
        panic!("public struct declaration")
    };
    let TypeExport::Closed(Ty::Struct(alias)) = &namespace.types["Alias"] else {
        panic!("closed struct alias")
    };
    assert_eq!(*model, alias.declaration);
    assert!(alias.arguments.is_empty());
    let model = definitions.resolve(*model).unwrap();
    assert_eq!(model.module().path, ["hidden"]);
    assert_eq!(model.last().unwrap().name, "User");
    let TypeExport::Closed(Ty::Struct(cell)) = &namespace.types["IntCell"] else {
        panic!("unused closed generic struct alias")
    };
    assert_eq!(cell.arguments, [Ty::Builtin(BuiltinType::I32)]);
    let declaration = definitions.resolve(cell.declaration).unwrap();
    assert_eq!(declaration.module().path, ["hidden"]);
    assert_eq!(declaration.last().unwrap().name, "Cell");
    let TypeExport::Declaration(packet) = &namespace.types["Packet"] else {
        panic!("unused public enum")
    };
    let TypeExport::Closed(Ty::Enum(message)) = &namespace.types["Message"] else {
        panic!("closed enum alias")
    };
    assert_eq!(*packet, message.declaration);
    assert_eq!(message.arguments, [Ty::Builtin(BuiltinType::I32)]);
    let TypeExport::Declaration(marker) = &namespace.types["Marker"] else {
        panic!("unused public trait")
    };
    let TypeExport::Closed(Ty::Trait(interface)) = &namespace.types["Interface"] else {
        panic!("closed interface alias")
    };
    assert_eq!(*marker, interface.declaration);
    let TypeExport::Declaration(token) = &namespace.types["Token"] else {
        panic!("unused opaque declaration")
    };
    assert_eq!(
        namespace.types["Handle"],
        TypeExport::Closed(Ty::Host(*token))
    );
    assert_eq!(
        namespace.types["Count"],
        TypeExport::Closed(Ty::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        namespace.types["Pair"],
        TypeExport::Closed(Ty::Tuple(vec![
            Ty::Builtin(BuiltinType::I32),
            Ty::Builtin(BuiltinType::Bool)
        ]))
    );
    assert_eq!(
        namespace.types["Erased"],
        TypeExport::OpenAlias { arity: 1 }
    );
    assert!(!namespace.modules.contains_key("hidden"));
}

#[test]
fn source_free_exports_preserve_aliases_cycles_and_separate_entry() {
    let program = compiled();
    let bytes = encode_program(&program, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    assert_eq!(
        bytes,
        encode_program(&decoded, &Default::default()).unwrap()
    );
    let context = DefinitionContext::new().unwrap();
    let editable = decoded
        .into_unverified()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let root = &editable.root;
    assert_type_exports(&editable.exports.namespaces[root], &context.snapshot());
    let hidden = editable
        .modules
        .iter()
        .find(|module| module.identity.path == ["hidden"])
        .unwrap();
    for name in ["User", "Packet", "Marker"] {
        assert!(
            hidden
                .abi
                .public_items
                .iter()
                .any(|item| item.name() == name)
        );
    }
    assert!(!editable.exports.namespaces.contains_key(&hidden.identity));
    let damage = function(&editable, &["hidden"], "calculate");
    assert_eq!(editable.exports.function(root, "damage"), Some(damage));
    assert_eq!(
        editable.exports.function(root, "again::again::damage"),
        Some(damage)
    );
    assert_eq!(
        editable.exports.function(root, "api"),
        Some(function(&editable, &[], "api"))
    );
    assert_eq!(
        editable.exports.function(root, "api::tick"),
        Some(function(&editable, &["api"], "tick"))
    );
    assert_eq!(
        editable.exports.function(root, "wrapper"),
        Some(function(&editable, &[], "wrapper"))
    );
    assert_eq!(
        editable.exports.entry,
        Some(function(&editable, &[], "main"))
    );
    for private in [
        "main",
        "hidden::calculate",
        "secret",
        "generic",
        "::damage",
        "damage::",
    ] {
        assert_eq!(editable.exports.function(root, private), None, "{private}");
    }
    assert_eq!(
        editable
            .exports
            .function(root, &format!("{}damage", "again::".repeat(64))),
        None
    );
    let mir = verify_program(editable, &context, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
        .into_records();
    let artifact =
        KbcArtifact::from_program(bytecode, Default::default(), &context, &Default::default())
            .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let fresh = DefinitionContext::new().unwrap();
    let artifact = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&fresh, &Default::default())
        .unwrap()
        .into_records();
    assert_eq!(bytes, artifact.to_bytes(&fresh.snapshot()).unwrap());
    let verified = artifact
        .into_verified(&Default::default(), &fresh, &Default::default())
        .unwrap()
        .into_bytecode();
    let program = verified.program();
    let root = &program.modules[program.root.index()].identity;
    assert_type_exports(&program.exports.namespaces[root], &fresh.snapshot());
    let hidden = program
        .modules
        .iter()
        .find(|module| module.identity.path == ["hidden"])
        .unwrap();
    for name in ["User", "Packet", "Marker"] {
        assert!(hidden.public_items.iter().any(|item| item.name() == name));
    }
    assert!(!program.exports.namespaces.contains_key(&hidden.identity));
    let opaque = program
        .modules
        .iter()
        .find(|module| module.identity.package.0 == "host")
        .unwrap();
    assert!(opaque.public_items.iter().any(|item| matches!(item, PublicItem::Type(ty) if ty.name == "Token" && matches!(ty.kind, TypeDefKind::Host(_)))));
    for (path, name) in [
        ("damage", "calculate"),
        ("again::damage", "calculate"),
        ("api", "api"),
        ("api::tick", "tick"),
        ("wrapper", "wrapper"),
    ] {
        let id = program.exports.function(root, path).unwrap();
        let target = verified.export_targets()[&id];
        let body = &program.modules[target.module.index()].functions[target.function.index()];
        assert_eq!(body.name, name);
        assert_eq!(body.identity.as_ref().unwrap().declaration, id);
        assert!(body.identity.as_ref().unwrap().arguments.is_empty());
    }
    assert!(program.exports.function(root, "main").is_none());
    let entry = verified.export_targets()[&program.exports.entry.unwrap()];
    assert_eq!(entry.module, program.root);
    assert_eq!(
        program.modules[entry.module.index()].functions[entry.function.index()].name,
        "main"
    );
}

#[test]
fn executable_export_validation_rejects_forged_authority_and_dangling_graphs() {
    let program = compiled();
    let bytes = encode_program(&program, &Default::default()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let bytecode = lower_program_to_bytecode(&program, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    identity::collect(
        &bytecode.modules.iter().collect::<Vec<_>>(),
        &context,
        &Default::default(),
    )
    .unwrap();
    verify_bytecode(&bytecode, &context.snapshot(), &Default::default()).unwrap();
    let raw = decode_program(&bytes, &Default::default())
        .unwrap()
        .into_unverified()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    for corruption in 0..8 {
        let mut forged = raw.clone();
        let hidden = function(&forged, &["hidden"], "secret");
        let foreign_main = function(&forged, &["hidden"], "main");
        let wrapper = function(&forged, &[], "wrapper");
        match corruption {
            0 => {
                forged
                    .exports
                    .namespaces
                    .get_mut(&forged.root)
                    .unwrap()
                    .functions
                    .insert("stolen".into(), hidden);
            }
            1 => {
                forged.exports.entry = Some(foreign_main);
            }
            2 => {
                forged.exports.entry = Some(wrapper);
            }
            3 => {
                forged
                    .exports
                    .namespaces
                    .get_mut(&forged.root)
                    .unwrap()
                    .modules
                    .insert("missing".into(), ModuleIdentity::single_file("missing"));
            }
            4 => {
                forged.exports.namespaces.insert(
                    ModuleIdentity::single_file("unreachable"),
                    ExportNamespace::default(),
                );
            }
            5 => {
                let absent = context
                    .intern_child(
                        context.intern_root(&forged.root).unwrap(),
                        DefinitionKind::Function,
                        "absent",
                        0,
                    )
                    .unwrap();
                forged
                    .exports
                    .namespaces
                    .get_mut(&forged.root)
                    .unwrap()
                    .functions
                    .insert("absent".into(), absent);
            }
            6 => {
                let module = forged
                    .modules
                    .iter_mut()
                    .find(|module| module.identity.path == ["hidden"])
                    .unwrap();
                let item = module
                    .abi
                    .public_items
                    .iter_mut()
                    .find_map(|item| match item {
                        PublicItem::Function(function) if function.name == "calculate" => {
                            Some(function)
                        }
                        _ => None,
                    })
                    .unwrap();
                item.return_type = Ty::Builtin(BuiltinType::Bool);
            }
            7 => {
                forged
                    .modules
                    .iter_mut()
                    .find(|module| module.identity == forged.root)
                    .unwrap()
                    .functions
                    .iter_mut()
                    .find(|function| function.name == "main")
                    .unwrap()
                    .semantic
                    .result = None;
            }
            _ => unreachable!(),
        }
        let mut forged_bytecode = bytecode.clone();
        forged_bytecode.exports = forged.exports.clone();
        if corruption == 6 {
            let module = forged_bytecode
                .modules
                .iter_mut()
                .find(|module| module.identity.path == ["hidden"])
                .unwrap();
            let item = module
                .public_items
                .iter_mut()
                .find_map(|item| match item {
                    PublicItem::Function(function) if function.name == "calculate" => {
                        Some(function)
                    }
                    _ => None,
                })
                .unwrap();
            item.return_type = Ty::Builtin(BuiltinType::Bool);
        }
        if corruption == 7 {
            forged_bytecode.modules[forged_bytecode.root.index()]
                .functions
                .iter_mut()
                .find(|function| function.name == "main")
                .unwrap()
                .metadata
                .semantic
                .result = None;
        }
        let error = verify_bytecode(&forged_bytecode, &context.snapshot(), &Default::default())
            .unwrap_err();
        if corruption == 6 {
            // The named inventory now rejects disagreement with public ABI before
            // executable flow or export binding can confer callable authority.
            assert_eq!(error, BytecodeVerificationError::InvalidPublicAbi);
        } else if corruption == 7 {
            // Missing executable result metadata is rejected before export binding.
            assert!(
                matches!(
                    error,
                    BytecodeVerificationError::InvalidOperation {
                        reason: "invalid collection access flow",
                        ..
                    }
                ),
                "bytecode corruption {corruption}: {error:?}"
            );
        } else {
            assert_eq!(
                error,
                BytecodeVerificationError::InvalidProgramExports,
                "bytecode corruption {corruption}"
            );
        }
        let error = verify_program(forged, &context, &Default::default()).unwrap_err();
        let expected = match corruption {
            6 => matches!(
                error.kind,
                ProgramErrorKind::Verification(MirVerificationError {
                    kind: MirVerificationErrorKind::InvalidPublicAbi,
                    ..
                })
            ),
            7 => matches!(
                error.kind,
                ProgramErrorKind::Verification(MirVerificationError {
                    kind: MirVerificationErrorKind::InvalidParameterLayout,
                    ..
                })
            ),
            _ => matches!(error.kind, ProgramErrorKind::InvalidExports),
        };
        assert!(expected, "MIR corruption {corruption}: {error:?}");
    }
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        verify_program(raw, &context, &cancel).unwrap_err().kind,
        ProgramErrorKind::Cancelled
    ));
}
