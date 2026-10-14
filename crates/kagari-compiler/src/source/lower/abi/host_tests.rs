use super::collect_module_abi;
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::types::{PublicItem, verify};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    declaration::{
        FieldDef, TypeDef, TypeDefKind,
        host::{HostFieldPolicy, HostTypePolicy},
        module::ModuleDecl,
    },
    host_interface::type_declaration::{HostReflectionPolicy, HostTypeOwnership, PathAccess},
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
    sources
        .set("external-abi.kgr", "fn main() {}".into(), SourceLayer::Base)
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
}
