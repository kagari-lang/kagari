use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::PathId,
    module::{BytecodeModule, PathRecord},
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_contract::types::PublicItem;
use kagari_types::{
    declaration::{
        FieldDef, TypeDeclaration, TypeDef, TypeDefKind,
        host::{
            HostFieldPolicy, HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};

/// Build a source-free path fixture from the shared host schema and resolved contract.
pub(super) fn actor_path_module(
    context: &DefinitionContext,
    field_name: &str,
    access: PathAccess,
) -> BytecodeModule {
    let mut module = BytecodeModule::default();
    let owner = context.intern_root(&module.identity).unwrap();
    let actor = context
        .intern_child(owner, DefinitionKind::Struct, "Actor", 0)
        .unwrap();
    let field = context
        .intern_child(actor, DefinitionKind::Field, field_name, 0)
        .unwrap();
    let definition = TypeDef {
        name: "Actor".into(),
        kind: TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: access,
        }),
        generic_params: vec![],
        bounds: vec![],
        fields: vec![FieldDef {
            host_policy: Some(HostFieldPolicy {
                readable: true,
                path_access: access,
            }),
            visibility: Visibility::Public,
            name: field_name.into(),
            ty: Ty::Builtin(BuiltinType::I32),
            mutable: access == PathAccess::ReadWrite,
        }],
        variants: vec![],
    };
    module
        .public_items
        .push(PublicItem::Type(definition.clone()));
    module.external_types.push(TypeDeclaration {
        declaration: actor,
        definition,
    });
    let path = HostPathDeclaration {
        root: actor,
        segments: vec![HostPathSegmentDeclaration::Field(field)],
        access,
        schema_epoch: 1,
    };
    let definitions = context.snapshot();
    let contract = path.contract(&module.external_types, &definitions).unwrap();
    module.paths.push(PathRecord {
        contract_fingerprint: contract.fingerprint(&definitions).unwrap(),
        id: PathId::new(0),
        root_ty: ValueType::HostHandle,
        result_ty: ValueType::I32,
        read_only: access == PathAccess::ReadOnly,
        debug_name: format!("Actor.{field_name}"),
    });
    module.path_declarations.push(path);
    module
}
