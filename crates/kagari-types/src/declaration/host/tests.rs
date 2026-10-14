use crate::{
    callable::CallableImplementation,
    declaration::{
        FieldDef, FnDecl, NativeDeclaration, Param, TypeDef, TypeDefKind,
        host::{HostCallPolicy, HostFieldPolicy, HostTypePolicy},
        module::ModuleDecl,
        verify::{Parameters, aggregate_shape_valid, function_valid, validate_native_declarations},
        wire::{declarations::TypeDefRecord, signature::FnDeclRecord},
    },
    decode_limits::MAX_NESTED_RECORDS,
    host_interface::{
        HostFunctionEffects, HostPassingStyle,
        type_declaration::{HostReflectionPolicy, HostTypeOwnership, PathAccess},
    },
    language::binding,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
    visibility::Visibility,
};
use bincode::{DefaultOptions, Options};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity,
    map::DefinitionContext,
    metadata::DefinitionMetadata,
    table::{DefinitionTable, wire::PortableDefinitionTable},
};
use std::slice;

fn fixture() -> (DefinitionContext, ModuleDecl) {
    let context = DefinitionContext::new().unwrap();
    let mut module = ModuleDecl::new(ModuleIdentity::single_file("host_policy"));
    module.types.push(TypeDef {
        name: "Player".into(),
        kind: TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: PathAccess::ReadWrite,
        }),
        generic_params: vec![],
        bounds: vec![],
        variants: vec![],
        fields: vec![FieldDef {
            host_policy: Some(HostFieldPolicy {
                readable: true,
                path_access: PathAccess::ReadWrite,
            }),
            visibility: Visibility::Public,
            name: "hp".into(),
            ty: Ty::Builtin(BuiltinType::I32),
            mutable: true,
        }],
    });
    module.collect_definitions(&context).unwrap();
    let root = context.snapshot().lookup_root(&module.identity).unwrap();
    let player = context
        .snapshot()
        .lookup_child(root, DefinitionKind::Struct, "Player", 0)
        .unwrap();
    let heal = context
        .intern_child(root, DefinitionKind::Function, "heal", 0)
        .unwrap();
    module.functions.push(FnDecl {
        host_policy: Some(HostCallPolicy {
            passing: vec![HostPassingStyle::UniqueBorrow, HostPassingStyle::Owned],
            effects: HostFunctionEffects {
                may_mutate_host_state: true,
                ..Default::default()
            },
        }),
        method_policy: Default::default(),
        name: "heal".into(),
        implementation: CallableImplementation::Native(heal),
        generic_params: vec![],
        bounds: vec![],
        params: vec![
            Param {
                name: "player".into(),
                ty: Ty::Host(player),
                mutable: false,
            },
            Param {
                name: "amount".into(),
                ty: Ty::Builtin(BuiltinType::I32),
                mutable: false,
            },
        ],
        return_type: Ty::Builtin(BuiltinType::I32),
    });
    module.collect_definitions(&context).unwrap();
    (context, module)
}

fn checked(function: &FnDecl, table: &DefinitionTable) -> bool {
    let CallableImplementation::Native(id) = function.implementation else {
        return false;
    };
    function_valid(
        function,
        id,
        &Parameters::new(),
        None,
        &Default::default(),
        table,
    ) && validate_native_declarations(
        slice::from_ref(&NativeDeclaration {
            declaration: id,
            function: function.clone(),
            concrete_result: None,
            callable_requirements: vec![],
        }),
        table.resolve(id).unwrap().module(),
        table,
        &Default::default(),
    )
    .is_ok()
}

#[test]
fn host_policy_records_round_trip_and_remap_without_second_signatures() {
    let (context, module) = fixture();
    let definitions = context.snapshot();
    let cancel = Default::default();
    module.validate(&|_| None, &definitions).unwrap();
    assert!(checked(&module.functions[0], &definitions));
    let metadata =
        DefinitionMetadata::checked(definitions.clone(), module.clone(), &cancel).unwrap();
    let destination = DefinitionContext::new().unwrap();
    let adopted = metadata.import_into(&destination, &cancel).unwrap();
    adopted
        .records()
        .validate(&|_| None, adopted.definitions())
        .unwrap();
    assert!(checked(
        &adopted.records().functions[0],
        adopted.definitions()
    ));
    assert_eq!(
        module.compiler_input_bytes(&definitions, &cancel).unwrap(),
        adopted
            .records()
            .compiler_input_bytes(adopted.definitions(), &cancel)
            .unwrap()
    );
    let root = definitions.lookup_root(&module.identity).unwrap();
    let heal = definitions
        .lookup_child(root, DefinitionKind::Function, "heal", 0)
        .unwrap();
    let player = definitions
        .lookup_child(root, DefinitionKind::Struct, "Player", 0)
        .unwrap();
    let encoding = definitions.encode([heal, player]).unwrap();
    let type_record = TypeDefRecord::encode(&module.types[0], &encoding, &cancel).unwrap();
    let function_record = FnDeclRecord::encode(&module.functions[0], &encoding, &cancel).unwrap();
    let bytes = DefaultOptions::new()
        .serialize(&(encoding.table.clone(), type_record, function_record))
        .unwrap();
    let (table, type_record, function_record): (
        PortableDefinitionTable,
        TypeDefRecord,
        FnDeclRecord,
    ) = DefaultOptions::new().deserialize(&bytes).unwrap();
    let decoding = table.decode().unwrap();
    let ty = type_record.decode(&decoding, &cancel).unwrap();
    let function = function_record.decode(&decoding, &cancel).unwrap();
    assert_eq!(ty, module.types[0]);
    assert_eq!(function.host_policy, module.functions[0].host_policy);
    assert!(checked(&function, &decoding.table));
    let mut changed = module;
    changed.types[0].fields[0]
        .host_policy
        .as_mut()
        .unwrap()
        .readable = false;
    assert!(!aggregate_shape_valid(&changed.types[0], &cancel));
    assert_ne!(
        changed.compiler_input_bytes(&definitions, &cancel).unwrap(),
        metadata
            .records()
            .compiler_input_bytes(&definitions, &cancel)
            .unwrap()
    );
}

#[test]
fn host_policy_rejects_invalid_borrow_access_and_immutable_configuration_shapes() {
    let oversized = ((MAX_NESTED_RECORDS + 1) as u64).to_le_bytes();
    let error = DefaultOptions::new()
        .with_fixint_encoding()
        .deserialize::<HostCallPolicy>(&oversized)
        .unwrap_err()
        .to_string();
    assert!(error.contains("nested declaration"), "{error}");
    let (context, module) = fixture();
    let table = context.snapshot();
    let original = &module.functions[0];
    let mut function = original.clone();
    function.params[1].name = function.params[0].name.clone();
    assert!(!checked(&function, &table));
    function = original.clone();
    function.host_policy.as_mut().unwrap().passing.pop();
    assert!(!checked(&function, &table));
    function = original.clone();
    function.host_policy.as_mut().unwrap().passing[1] = HostPassingStyle::UniqueBorrow;
    assert!(!checked(&function, &table));
    function.params[1].ty = Ty::Builtin(BuiltinType::String);
    function.host_policy.as_mut().unwrap().passing[1] = HostPassingStyle::SharedBorrow;
    assert!(checked(&function, &table));
    function
        .host_policy
        .as_mut()
        .unwrap()
        .effects
        .may_read_immutable_configuration = true;
    assert!(!checked(&function, &table));
    function
        .host_policy
        .as_mut()
        .unwrap()
        .passing
        .fill(HostPassingStyle::Owned);
    function.params[0].ty = Ty::Builtin(BuiltinType::I32);
    assert!(checked(&function, &table));
    function.params[0].ty = Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32)));
    assert!(!checked(&function, &table));
    let option = context.intern(&binding::option_declaration()).unwrap();
    function.params[0].ty = Ty::Enum(NominalTy {
        declaration: option,
        arguments: vec![Ty::Builtin(BuiltinType::I32)],
        associated_types: Default::default(),
    });
    assert!(checked(&function, &context.snapshot()));
    let mut ordinary = module.types[0].clone();
    ordinary.kind = TypeDefKind::Struct;
    assert!(!aggregate_shape_valid(&ordinary, &Default::default()));
    ordinary.fields[0].host_policy = None;
    assert!(aggregate_shape_valid(&ordinary, &Default::default()));
}
