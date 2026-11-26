use crate::{
    callable::CallableImplementation,
    declaration::{
        FieldDef, FnDecl, NativeDeclaration, Param, TraitDef, TypeDeclaration, TypeDef,
        TypeDefKind,
        host::{
            HostCallPolicy, HostFieldPolicy, HostFunctionEffects, HostPassingStyle,
            HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::ModuleDecl,
        path::{
            HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration,
            HostVirtualSegmentDeclaration, NativePathOperation, NativePathOperationKind,
        },
        verify::{
            DeclarationValidationError, Parameters, aggregate_shape_valid, function_valid,
            validate_native_declarations,
        },
        wire::{declarations::TypeDefRecord, signature::FnDeclRecord},
    },
    decode_limits::MAX_NESTED_RECORDS,
    language::binding,
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
    visibility::Visibility,
};
use bincode::{DefaultOptions, Options};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity,
    map::DefinitionContext,
    mapping::DefinitionMappingError,
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
            path_operation: None,
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
fn explicit_call_contract_uses_segment_order_and_checks_the_writable_leaf() {
    let (context, mut module) = fixture();
    let definitions = context.snapshot();
    let player = module
        .definition(DefinitionKind::Struct, "Player", &definitions)
        .unwrap();
    let heal = module
        .definition(DefinitionKind::Function, "heal", &definitions)
        .unwrap();
    let hp = definitions
        .lookup_child(player, DefinitionKind::Field, "hp", 0)
        .unwrap();
    let index = |slot, ty| {
        HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
            slot,
            collection: Ty::Host(player),
            result: Ty::Host(player),
            index: Ty::Builtin(ty),
            access: PathAccess::ReadWrite,
        })
    };
    let path = HostPathDeclaration {
        root: player,
        access: PathAccess::ReadWrite,
        schema_epoch: 7,
        segments: vec![
            index(1, BuiltinType::String),
            index(0, BuiltinType::U64),
            HostPathSegmentDeclaration::Field(hp),
        ],
    };
    let function = &mut module.functions[0];
    function.params.splice(
        1..1,
        [("key", BuiltinType::String), ("position", BuiltinType::U64)].map(|(name, ty)| Param {
            name: name.into(),
            ty: Ty::Builtin(ty),
            mutable: false,
        }),
    );
    function.host_policy.as_mut().unwrap().passing = vec![HostPassingStyle::Owned; 4];
    module.path_operations.insert(
        heal,
        NativePathOperation {
            kind: NativePathOperationKind::Add,
            path: path.clone(),
        },
    );
    assert!(module.validate(&|_| None, &definitions).is_err());
    module.paths.push(path);
    let cancel = Default::default();
    let bytes = module.compiler_input_bytes(&definitions, &cancel).unwrap();
    let mut omitted = module.clone();
    omitted.path_operations.clear();
    assert_ne!(
        bytes,
        omitted.compiler_input_bytes(&definitions, &cancel).unwrap()
    );
    let foreign = DefinitionContext::new().unwrap();
    let adopted = DefinitionMetadata::checked(definitions.clone(), module.clone(), &cancel)
        .unwrap()
        .import_into(&foreign, &cancel)
        .unwrap();
    adopted
        .records()
        .validate(&|_| None, adopted.definitions())
        .unwrap();
    assert_eq!(
        bytes,
        adopted
            .records()
            .compiler_input_bytes(adopted.definitions(), &cancel)
            .unwrap()
    );
    module.validate(&|_| None, &definitions).unwrap();
    let declaration = module
        .native_declarations(&definitions)
        .unwrap()
        .pop()
        .unwrap();
    let schemas = [TypeDeclaration {
        declaration: player,
        definition: module.types[0].clone(),
    }];
    let check = |decl: &NativeDeclaration| decl.validate_path_operation(&schemas, &definitions);
    check(&declaration).unwrap();
    let invalid = Err(DefinitionMappingError::InvalidContract);
    let mut wrong = declaration.clone();
    wrong.function.params.swap(1, 2);
    assert_eq!(check(&wrong), invalid);
    let mut wrong = declaration.clone();
    wrong
        .function
        .host_policy
        .as_mut()
        .unwrap()
        .effects
        .may_mutate_host_state = false;
    assert_eq!(check(&wrong), invalid);
    let mut wrong = declaration.clone();
    wrong.function.host_policy.as_mut().unwrap().passing[0] = HostPassingStyle::UniqueBorrow;
    assert_eq!(check(&wrong), invalid);
    let mut wrong = declaration.clone();
    wrong.function.return_type = Ty::Builtin(BuiltinType::I64);
    wrong.function.params[3].ty = wrong.function.return_type.clone();
    assert_eq!(check(&wrong), invalid);
    let mut wrong = declaration.clone();
    wrong.path_operation.as_mut().unwrap().path.access = PathAccess::ReadOnly;
    assert_eq!(check(&wrong), invalid);
    let mut set = declaration.clone();
    set.path_operation.as_mut().unwrap().kind = NativePathOperationKind::Set;
    assert_eq!(check(&set), invalid);
    set.function.return_type = Ty::Builtin(BuiltinType::Unit);
    check(&set).unwrap();
    let mut string_leaf = schemas;
    string_leaf[0].definition.fields[0].ty = Ty::Builtin(BuiltinType::String);
    let mut concat = declaration;
    concat.function.params[3].ty = Ty::Builtin(BuiltinType::String);
    concat.function.return_type = Ty::Builtin(BuiltinType::String);
    assert_eq!(
        concat.validate_path_operation(&string_leaf, &definitions),
        invalid
    );
}

#[test]
fn whole_path_write_traverses_only_a_readable_object_val() {
    let (context, module) = fixture();
    let table = context.snapshot();
    let player = module
        .definition(DefinitionKind::Struct, "Player", &table)
        .unwrap();
    let mut schema = TypeDeclaration {
        declaration: player,
        definition: module.types[0].clone(),
    };
    let hp = HostPathSegmentDeclaration::Field(
        table
            .lookup_child(player, DefinitionKind::Field, "hp", 0)
            .unwrap(),
    );
    let baseline = HostPathSegmentDeclaration::Field(
        context
            .intern_child(player, DefinitionKind::Field, "baseline", 0)
            .unwrap(),
    );
    let table = context.snapshot();
    let object = Ty::Host(player);
    let scalar = Ty::Builtin(BuiltinType::I32);
    schema.definition.fields.push(FieldDef {
        name: "baseline".into(),
        ty: object.clone(),
        visibility: Visibility::Public,
        mutable: false,
        host_policy: Some(HostFieldPolicy {
            readable: true,
            path_access: PathAccess::ReadOnly,
        }),
    });
    let check = |schema: &TypeDeclaration, segments, writable| {
        let mut path = HostPathDeclaration {
            root: player,
            segments,
            access: PathAccess::ReadOnly,
            schema_epoch: 0,
        };
        let mut contract = path.contract(slice::from_ref(schema), &table).unwrap();
        let read = contract.fingerprint(&table).unwrap();
        contract.access = PathAccess::ReadWrite;
        path.access = PathAccess::ReadWrite;
        let resolved = path.contract(slice::from_ref(schema), &table);
        if writable {
            assert_eq!(resolved.unwrap(), contract);
            assert_ne!(contract.fingerprint(&table).unwrap(), read);
        } else {
            assert_eq!(resolved, Err(DefinitionMappingError::InvalidContract));
            assert_eq!(
                contract.fingerprint(&table),
                Err(DefinitionMappingError::InvalidContract)
            );
        }
    };
    let nested = vec![baseline.clone(), hp.clone()];
    check(&schema, nested.clone(), true);
    check(&schema, vec![baseline.clone()], false);
    for prefix in [
        HostPathSegmentDeclaration::Virtual(HostVirtualSegmentDeclaration {
            name: "baseline".into(),
            result: object.clone(),
            access: PathAccess::ReadOnly,
        }),
        HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
            slot: 0,
            collection: object.clone(),
            index: scalar.clone(),
            result: object,
            access: PathAccess::ReadOnly,
        }),
    ] {
        check(&schema, vec![prefix, hp.clone()], false);
    }
    // A host-imposed read cap on a `var` is not the `val` traversal exception.
    schema.definition.fields[1].mutable = true;
    check(&schema, nested.clone(), false);
    schema.definition.fields[1].mutable = false;
    let hp = &mut schema.definition.fields[0];
    hp.mutable = false;
    hp.host_policy.as_mut().unwrap().path_access = PathAccess::ReadOnly;
    check(&schema, nested, false);
    schema.definition.fields[1].ty = scalar.clone();
    check(
        &schema,
        vec![
            baseline,
            HostPathSegmentDeclaration::Virtual(HostVirtualSegmentDeclaration {
                name: "value".into(),
                result: scalar,
                access: PathAccess::ReadWrite,
            }),
        ],
        false,
    );
}

#[test]
fn host_policy_checks_unused_inherited_binders_on_the_executable_template() {
    let (context, mut module) = fixture();
    let root = context.snapshot().lookup_root(&module.identity).unwrap();
    let owner = context
        .intern_child(root, DefinitionKind::Trait, "Service", 0)
        .unwrap();
    let id = context
        .intern_child(owner, DefinitionKind::Method, "heal", 0)
        .unwrap();
    let mut method = module.functions.remove(0);
    method.implementation = CallableImplementation::Native(id);
    module.traits.push(TraitDef {
        name: "Service".into(),
        methods: vec![method],
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        conversion_adapter: None,
    });
    let table = context.snapshot();
    let check = |module: &ModuleDecl| {
        validate_native_declarations(
            &module.native_declarations(&table).unwrap(),
            &module.identity,
            &table,
            &Default::default(),
        )
    };
    check(&module).unwrap();
    module.traits[0]
        .generic_params
        .push(GenericParam { owner, position: 0 });
    assert_eq!(check(&module), Err(DeclarationValidationError::Invalid));
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
    function.host_policy = None;
    assert!(!checked(&function, &table));
    function.params[0].ty = Ty::Tuple(vec![function.params[0].ty.clone()]);
    assert!(!checked(&function, &table));
    function = original.clone();
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
