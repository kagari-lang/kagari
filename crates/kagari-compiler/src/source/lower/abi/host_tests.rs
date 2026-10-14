use super::collect_module_abi;
use crate::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, ArtifactCompatibility, KbcArtifact},
    program::verified::VerifiedBytecodeProgram,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::types::{PublicItem, verify};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_mir::{
    codec::{decode_program, encode_program},
    program::verify_program as verify_mir_program,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    declaration::{
        FieldDef, TypeDef, TypeDefKind,
        host::{
            HostFieldPolicy, HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::ModuleDecl,
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::sync::Arc;

#[test]
fn external_native_abi_preserves_shared_field_policy_and_nominal_scope() {
    let context = DefinitionContext::new().unwrap();
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("example".into()),
        path: vec!["roots".into()],
    });
    module.types.push(TypeDef {
        name: "Player".into(),
        kind: TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: PathAccess::ReadWrite,
        }),
        generic_params: vec![],
        bounds: vec![],
        fields: vec![FieldDef {
            name: "hp".into(),
            ty: Ty::Builtin(BuiltinType::I32),
            visibility: Visibility::Public,
            mutable: true,
            host_policy: Some(HostFieldPolicy {
                readable: true,
                path_access: PathAccess::ReadWrite,
            }),
        }],
        variants: vec![],
    });
    module.collect_definitions(&context).unwrap();
    let player = module
        .definition(DefinitionKind::Struct, "Player", &context.snapshot())
        .unwrap();
    module.types[0].fields.push(FieldDef {
        name: "parent".into(),
        ty: Ty::Host(player),
        visibility: Visibility::Private,
        mutable: false,
        host_policy: Some(HostFieldPolicy {
            readable: true,
            path_access: PathAccess::ReadOnly,
        }),
    });
    module.collect_definitions(&context).unwrap();
    let metadata = Arc::new(
        DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap(),
    );
    let mut providers = kagari_stdlib::catalog::shared().unwrap();
    providers.push(metadata.clone());
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(providers);
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("external-abi.kgr", "use example::roots::Player; pub fn relay(player: Player) -> Player { player } fn main() {}".into(), SourceLayer::Base)
        .unwrap();
    let snapshot = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let declared = snapshot
        .declaration_snapshot()
        .files()
        .find(|file| *file.source().module_identity() == metadata.records().identity)
        .unwrap();
    let analyzed = snapshot.file(declared.source().id()).unwrap();
    assert!(
        analyzed.result().diagnostics().is_empty(),
        "{:?}",
        analyzed.result().diagnostics()
    );
    let facts = analyzed.to_unverified(&Default::default()).unwrap();
    let abi = collect_module_abi(facts.records().facts());
    let [PublicItem::Type(ty)] = abi.public_items.as_slice() else {
        panic!("external type ABI")
    };
    assert_eq!(ty.kind, metadata.records().types[0].kind);
    assert_eq!(ty.fields[0], metadata.records().types[0].fields[0]);
    assert_eq!(
        ty.fields[1].host_policy,
        metadata.records().types[0].fields[1].host_policy
    );
    assert_eq!(ty.fields[1].visibility, Visibility::Private);
    let Ty::Host(parent) = ty.fields[1].ty else {
        panic!("nominal external member")
    };
    assert_eq!(
        analyzed.definitions().resolve(parent).unwrap().to_path(),
        metadata.definitions().resolve(player).unwrap().to_path()
    );
    verify::validate(
        &abi.public_items,
        &metadata.records().identity,
        &Default::default(),
        analyzed.definitions(),
    )
    .unwrap();
    // This exercises the previously failing recursive external dependency through
    // both portable products and rejects schema copies that disagree with providers.
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let wire = encode_program(&mir, &Default::default()).unwrap();
    let restored = decode_program(&wire, &Default::default()).unwrap();
    assert_eq!(
        wire,
        encode_program(&restored, &Default::default()).unwrap()
    );
    let bytecode = lower_program_to_bytecode(&restored).unwrap();
    let bytecode = bytecode
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    VerifiedBytecodeProgram::new(bytecode.clone(), &context).unwrap();
    let artifact =
        KbcArtifact::from_program(bytecode.clone(), ArtifactBuildOptions::default(), &context)
            .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes).unwrap();
    assert_eq!(
        bytes,
        decoded.records().to_bytes(decoded.definitions()).unwrap()
    );
    let decoded = decoded
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    decoded
        .validate_for_loader(&ArtifactCompatibility::default(), &context)
        .unwrap();

    let mut forged_mir = restored
        .into_unverified()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let caller = forged_mir
        .modules
        .iter_mut()
        .find(|module| module.identity == checked.root().lowered.source.module_identity().clone())
        .unwrap();
    assert_eq!(caller.external_types.len(), 1);
    caller.external_types[0].definition.fields[0].mutable = false;
    caller.external_types[0].definition.fields[0]
        .host_policy
        .as_mut()
        .unwrap()
        .path_access = PathAccess::ReadOnly;
    assert!(verify_mir_program(forged_mir, &context, &Default::default()).is_err());

    let mut forged_bytecode = bytecode;
    let caller_index = forged_bytecode.root.index();
    forged_bytecode.modules[caller_index].external_types[0]
        .definition
        .fields[0]
        .mutable = false;
    forged_bytecode.modules[caller_index].external_types[0]
        .definition
        .fields[0]
        .host_policy
        .as_mut()
        .unwrap()
        .path_access = PathAccess::ReadOnly;
    assert!(VerifiedBytecodeProgram::new(forged_bytecode, &context).is_err());
}
