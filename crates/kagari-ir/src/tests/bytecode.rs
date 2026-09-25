use crate::{
    bytecode::{
        ArtifactBuildOptions, ArtifactCompatibility, ArtifactFingerprint, ArtifactSectionId,
        ArtifactValidationError, BinaryOp, BytecodeFunction, BytecodeInstruction, BytecodeModule,
        BytecodeVerificationError, CallTarget, DebugMetadata, DependencyFingerprint, FieldRef,
        FunctionMetadata, FunctionRef, JumpTarget, KBC_MAGIC, KbcArtifact, LocalSlot, PathId,
        PathRecord, Register, RuntimeHelper, SafeDebugPointKind, StandardIntrinsic, StructId,
        UnaryOp, verify_module,
    },
    module::{PublicAbiItem, TypeAbiKind, ValueType},
    tests::common,
};
use kagari_common::identity::{ModuleIdentity, PackageId};

#[test]
fn rejects_function_fallthrough_before_loading() {
    for source in ["fn main() {}", "fn main() -> i32 { 42 }"] {
        let mut module = common::bytecode_ok(source);
        let function = module
            .functions
            .iter_mut()
            .find(|function| function.name == "main")
            .unwrap();
        assert!(matches!(
            function.instructions.last(),
            Some(BytecodeInstruction::Return(_))
        ));
        function.instructions.pop();
        assert!(matches!(
            verify_module(&module),
            Err(BytecodeVerificationError::InvalidOperation {
                reason: "function falls through without a terminator",
                ..
            })
        ));
        assert!(
            KbcArtifact::from_program(
                crate::bytecode::BytecodeProgram {
                    root: crate::bytecode::ModuleRef::new(0),
                    modules: vec![module],
                },
                ArtifactBuildOptions::default(),
            )
            .is_err()
        );
    }
}

#[test]
fn applied_trait_bounds_change_public_abi_fingerprint() {
    let fingerprint = |argument: &str| {
        let module = common::bytecode_ok(&format!(
            "pub trait Echo<T> {{}} pub struct Bag<T: Echo<{argument}>> {{ val value: T }} fn main() {{}}"
        ));
        let bag = module
            .public_items
            .iter()
            .find_map(|item| match item {
                PublicAbiItem::Type(item) if item.name == "Bag" => Some(item),
                _ => None,
            })
            .unwrap();
        assert!(matches!(&bag.bounds[0].constraints[0],
            crate::module::abi::ConstraintAbi::Trait(ty) if ty.arguments.len() == 1));
        let artifact = KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![module],
            },
            ArtifactBuildOptions::default(),
        )
        .unwrap();
        artifact
            .verification
            .public_abi_fingerprints
            .into_iter()
            .find(|item| item.name == "type:Bag")
            .unwrap()
            .fingerprint
    };
    assert_ne!(fingerprint("i32"), fingerprint("String"));
}

#[test]
fn applied_trait_bound_rejects_a_foreign_binder_before_loading() {
    let mut module = common::bytecode_ok(
        "pub trait Echo<T> {} pub struct Bag<T: Echo<T>> { val value: T } fn main() {}",
    );
    let bag = module
        .public_items
        .iter_mut()
        .find_map(|item| match item {
            PublicAbiItem::Type(item) if item.name == "Bag" => Some(item),
            _ => None,
        })
        .unwrap();
    let crate::module::abi::ConstraintAbi::Trait(ty) = &mut bag.bounds[0].constraints[0] else {
        panic!("trait bound");
    };
    let crate::module::abi::AbiType::Parameter { owner, .. } = &mut ty.arguments[0] else {
        panic!("template argument");
    };
    *owner = ty.declaration.clone();
    assert!(matches!(
        verify_module(&module),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    assert!(matches!(
        KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![module],
            },
            ArtifactBuildOptions::default(),
        ),
        Err(ArtifactValidationError::Bytecode(_))
    ));
}

#[test]
fn applied_trait_interface_table_preserves_method_contract() {
    let module = common::bytecode_ok(
        "pub trait Echo<T> { fn get(self) -> T; } pub struct Pair { val number: i32 } impl Echo<i32> for Pair { fn get(self) -> i32 { self.number } } fn main() {}",
    );
    assert_eq!(module.interface_tables.len(), 1);
    assert_eq!(module.interface_tables[0].methods.len(), 1);
    verify_module(&module).unwrap();
    let artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    KbcArtifact::from_bytes(&artifact.to_bytes().unwrap())
        .unwrap()
        .validate_for_loader(&ArtifactCompatibility::default())
        .unwrap();
}

#[test]
fn applied_trait_method_bounds_match_across_binder_owners() {
    let module = common::bytecode_ok(
        "pub trait Marker<T> {} pub struct Holder {} impl Marker<i32> for Holder {} pub trait Consumer<T> { fn take<U: Marker<T>>(self, value: U) -> U; } impl Consumer<i32> for Holder { fn take<V: Marker<i32>>(self, value: V) -> V { value } } fn main() {}",
    );
    verify_module(&module).unwrap();
}

#[test]
fn applied_trait_template_keeps_impl_and_trait_arguments() {
    let module = common::bytecode_ok(
        "pub trait Echo<T> { fn get(self) -> T; } pub struct Holder<T> { val value: T } impl<T> Echo<T> for Holder<T> { fn get(self) -> T { self.value } } fn main() {}",
    );
    assert_eq!(module.interface_tables.len(), 1);
    assert!(module.interface_tables[0].methods.is_empty());
    assert!(module.public_items.iter().any(|item| matches!(item,
        PublicAbiItem::InterfaceTable(table)
            if table.generic_params.len() == 1
                && matches!(&table.trait_type, crate::module::abi::AbiType::Trait(instance) if instance.arguments.len() == 1)
    )));
}

#[test]
fn generic_interface_implementation_specializes_reachable_method() {
    let module = common::bytecode_ok(
        "pub trait Get { fn get(self) -> i32; } pub struct Holder<T> { val value: T } impl<T> Get for Holder<T> { fn get(self) -> i32 { 42 } } fn read<U: Get>(x: U) -> i32 { x.get() } fn main() -> (i32, i32) { (read(Holder { value: 1 }), read(Holder { value: \"a\" })) }",
    );
    assert_eq!(module.interface_tables.len(), 1);
    assert_eq!(module.interface_tables[0].methods.len(), 2);
    let abi = module
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) => Some(table),
            _ => None,
        })
        .unwrap();
    assert_eq!(abi.generic_params.len(), 1);
    assert!(abi.methods[0].generic_params.is_empty());
    let arguments = module.interface_tables[0]
        .methods
        .iter()
        .map(|slot| {
            module.functions[slot.function.index()]
                .identity
                .as_ref()
                .unwrap()
                .arguments
                .clone()
        })
        .collect::<Vec<_>>();
    assert!(
        arguments.contains(&vec![crate::module::abi::AbiType::Builtin(
            kagari_hir::types::BuiltinType::I32
        )])
    );
    assert!(
        arguments.contains(&vec![crate::module::abi::AbiType::Builtin(
            kagari_hir::types::BuiltinType::String
        )])
    );
    let mut wrong_arity = module.clone();
    let method = wrong_arity.interface_tables[0].methods[0].function.index();
    wrong_arity.functions[method]
        .identity
        .as_mut()
        .unwrap()
        .arguments
        .clear();
    wrong_arity.function_table[method].identity = wrong_arity.functions[method].identity.clone();
    assert!(matches!(
        verify_module(&wrong_arity),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
}

#[test]
fn generic_interface_slot_requires_instantiated_method_layout() {
    let module = common::bytecode_ok(
        "pub trait Echo<T> { fn get(self) -> T; } pub struct Holder<T> { val value: T } impl<T> Echo<T> for Holder<T> { fn get(self) -> T { self.value } } fn read<U: Echo<i32>>(x: U) -> i32 { x.get() } fn main() -> i32 { read(Holder { value: 7 }) }",
    );
    assert_eq!(module.interface_tables[0].methods.len(), 1);
    verify_module(&module).unwrap();
    let mut wrong_instance = module;
    let method = wrong_instance.interface_tables[0].methods[0]
        .function
        .index();
    wrong_instance.functions[method]
        .identity
        .as_mut()
        .unwrap()
        .arguments[0] = crate::module::abi::AbiType::Builtin(kagari_hir::types::BuiltinType::Bool);
    wrong_instance.function_table[method].identity =
        wrong_instance.functions[method].identity.clone();
    assert!(matches!(
        verify_module(&wrong_instance),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
}

#[test]
fn concrete_interface_methods_have_verified_executable_slots() {
    let module = common::bytecode_ok(
        "pub struct Pair { val number: i32 } pub trait Number { fn get(self) -> i32; } impl Number for Pair { fn get(self) -> i32 { self.number } } fn main() -> i32 { 1 }",
    );
    assert_eq!(module.interface_tables.len(), 1);
    let table = &module.interface_tables[0];
    assert_eq!(table.methods.len(), 1);
    let slot = &table.methods[0];
    assert_eq!(slot.method.path.last().unwrap().name, "get");
    let function = &module.functions[slot.function.index()];
    let identity = function.identity.as_ref().unwrap();
    assert_eq!(identity.declaration.path.last().unwrap().name, "get");
    assert_eq!(
        identity.declaration.path[0].kind,
        kagari_common::identity::DefinitionKind::Impl
    );
    assert!(identity.arguments.is_empty());
    verify_module(&module).unwrap();

    let artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module.clone()],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    decoded
        .validate_for_loader(&ArtifactCompatibility::default())
        .unwrap();
    assert_eq!(
        decoded.program.modules[0].interface_tables[0].methods[0].function,
        slot.function
    );

    let mut missing = module.clone();
    missing.interface_tables[0].methods.clear();
    assert!(matches!(
        verify_module(&missing),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
    let mut wrong_target = module.clone();
    wrong_target.interface_tables[0].methods[0].function = wrong_target
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap()
        .id;
    assert!(matches!(
        verify_module(&wrong_target),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
    let mut wrong_method = module.clone();
    wrong_method.interface_tables[0].methods[0]
        .method
        .path
        .last_mut()
        .unwrap()
        .name = "other".into();
    assert!(matches!(
        verify_module(&wrong_method),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
    let mut missing_table = module;
    missing_table.interface_tables.clear();
    assert!(matches!(
        verify_module(&missing_table),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
}

#[test]
fn executable_function_identities_survive_lowering_and_reject_mismatched_records() {
    let module = common::bytecode_ok(
        "fn id<T>(value: T) -> T { value } fn other() -> i32 { 2 } fn main() -> i32 { id(1) + other() }",
    );
    assert!(module.functions.iter().all(|function| {
        function.identity.is_some()
            && module.function_table[function.id.index()].identity == function.identity
    }));
    let generic = module
        .functions
        .iter()
        .position(|function| {
            function.identity.as_ref().is_some_and(|identity| {
                identity
                    .declaration
                    .path
                    .last()
                    .is_some_and(|part| part.name == "id")
            })
        })
        .expect("reachable generic instance");
    assert_eq!(
        module.functions[generic]
            .identity
            .as_ref()
            .unwrap()
            .arguments,
        [crate::module::abi::AbiType::Builtin(
            kagari_hir::types::BuiltinType::I32
        )]
    );
    let mut mismatched_record = module.clone();
    mismatched_record.function_table[generic].identity = None;
    assert!(matches!(
        verify_module(&mismatched_record),
        Err(BytecodeVerificationError::FunctionRecordMismatch { .. })
    ));
    let mut duplicate = module.clone();
    let other = duplicate
        .functions
        .iter()
        .position(|function| {
            function.identity.as_ref().is_some_and(|identity| {
                identity
                    .declaration
                    .path
                    .last()
                    .is_some_and(|part| part.name == "other")
            })
        })
        .unwrap();
    duplicate.functions[other].identity = duplicate.functions[generic].identity.clone();
    duplicate.function_table[other].identity = duplicate.functions[other].identity.clone();
    assert!(matches!(
        verify_module(&duplicate),
        Err(BytecodeVerificationError::InvalidFunctionIdentity { .. })
    ));
    let mut foreign = module.clone();
    foreign.functions[generic]
        .identity
        .as_mut()
        .unwrap()
        .declaration
        .module
        .package
        .0 = "foreign".into();
    foreign.function_table[generic].identity = foreign.functions[generic].identity.clone();
    assert!(matches!(
        verify_module(&foreign),
        Err(BytecodeVerificationError::InvalidFunctionIdentity { .. })
    ));
    let mut wrong_kind = module.clone();
    wrong_kind.functions[generic]
        .identity
        .as_mut()
        .unwrap()
        .declaration
        .path
        .last_mut()
        .unwrap()
        .kind = kagari_common::identity::DefinitionKind::Struct;
    wrong_kind.function_table[generic].identity = wrong_kind.functions[generic].identity.clone();
    assert!(matches!(
        verify_module(&wrong_kind),
        Err(BytecodeVerificationError::InvalidFunctionIdentity { .. })
    ));
    let mut oversized = module.clone();
    oversized.functions[generic]
        .identity
        .as_mut()
        .unwrap()
        .arguments = vec![
        crate::module::abi::AbiType::Builtin(kagari_hir::types::BuiltinType::I32);
        crate::decode_limits::MAX_NESTED_RECORDS + 1
    ];
    oversized.function_table[generic].identity = oversized.functions[generic].identity.clone();
    assert!(matches!(
        KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![oversized],
            },
            Default::default(),
        ),
        Err(ArtifactValidationError::ResourceLimit(
            "nested module record limit exceeded"
        ))
    ));
    let mut unresolved = module;
    unresolved.functions[generic]
        .identity
        .as_mut()
        .unwrap()
        .arguments[0] = crate::module::abi::AbiType::Parameter {
        owner: unresolved.function_table[generic]
            .identity
            .as_ref()
            .unwrap()
            .declaration
            .clone(),
        position: 0,
    };
    unresolved.function_table[generic].identity = unresolved.functions[generic].identity.clone();
    assert!(matches!(
        verify_module(&unresolved),
        Err(BytecodeVerificationError::InvalidFunctionIdentity { .. })
    ));
}

#[test]
fn host_imports_are_interned_and_checked_before_execution() {
    let module = common::bytecode_ok(r#"fn main() { print("one"); print("two"); }"#);
    assert_eq!(
        module.host_interface.functions,
        vec![kagari_common::host_interface::standard_log()]
    );
    let mut absent = module.clone();
    absent.host_interface.functions.clear();
    assert!(matches!(
        verify_module(&absent),
        Err(BytecodeVerificationError::InvalidHostImport { .. })
    ));
    let mut wrong_parameter = module.clone();
    wrong_parameter.host_interface.functions[0].params[0].ty =
        kagari_common::host_interface::HostValueType::Bool;
    wrong_parameter.host_interface.functions[0].params[0].passing =
        kagari_common::host_interface::HostPassingStyle::Owned;
    assert!(matches!(
        verify_module(&wrong_parameter),
        Err(BytecodeVerificationError::TypeMismatch { .. })
    ));
    let mut wrong_arity = module.clone();
    wrong_arity.host_interface.functions[0].params.clear();
    assert!(matches!(
        verify_module(&wrong_arity),
        Err(BytecodeVerificationError::InvalidOperation { .. })
    ));
    let mut duplicate = module;
    duplicate
        .host_interface
        .functions
        .push(duplicate.host_interface.functions[0].clone());
    assert!(matches!(
        verify_module(&duplicate),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
}

#[test]
fn unsupported_dynamic_calls_fail_before_artifact_execution() {
    let module = common::bytecode_ok("fn main() {}");
    let valid = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    for callee in [
        CallTarget::Register(Register::new(0)),
        CallTarget::RuntimeHelper(RuntimeHelper::DynamicCall),
    ] {
        let mut forged = valid.clone();
        forged.program.modules[0].functions[0].instructions.insert(
            0,
            BytecodeInstruction::Call {
                dst: None,
                callee,
                args: vec![],
            },
        );
        let decoded = KbcArtifact::from_bytes(&forged.to_bytes().unwrap()).unwrap();
        assert!(matches!(
            decoded.validate_for_loader(&ArtifactCompatibility::default()),
            Err(ArtifactValidationError::Bytecode(
                BytecodeVerificationError::InvalidOperation { .. }
            ))
        ));
    }
}

fn host_trait_test_module(source: &str) -> BytecodeModule {
    let mut module = common::bytecode_ok(source);
    add_readable_host(&mut module);
    module
}

fn add_readable_host(module: &mut BytecodeModule) {
    use kagari_common::{
        host_interface::{
            HostMethodDeclaration, HostTraitImplementationDeclaration, HostTraitMethodBinding,
            HostTypeDeclaration, HostValueType,
        },
        identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
    };

    let trait_id = DefinitionId {
        module: module.identity.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Readable".into(),
            occurrence: 0,
        }],
    };
    let mut trait_method = trait_id.clone();
    trait_method.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Method,
        name: "get".into(),
        occurrence: 0,
    });
    let mut host = HostTypeDeclaration::new("demo.Counter");
    let method = HostMethodDeclaration::new(&host.id, "read", vec![], HostValueType::I32);
    host.methods.push(method.clone());
    host.trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            trait_id,
            vec![HostValueType::I32],
            vec![HostTraitMethodBinding {
                trait_method,
                host_method: method.id.clone(),
            }],
        ));
    module
        .host_interface
        .functions
        .push(host.method_contract(&method.id).unwrap());
    module.host_interface.types.push(host);
}

#[test]
fn public_host_trait_tables_are_rechecked_after_artifact_decode() {
    use kagari_common::host_interface::HostValueType;

    let module =
        host_trait_test_module("pub trait Readable<T> { fn get(self) -> T; } fn main() {}");
    assert!(module.trait_contracts.is_empty());
    verify_module(&module).unwrap();

    let valid = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    for corrupt in ["argument", "return", "method"] {
        let mut forged = valid.clone();
        let host = &mut forged.program.modules[0].host_interface.types[0];
        match corrupt {
            "argument" => host.trait_implementations[0].trait_arguments = vec![HostValueType::Bool],
            "return" => {
                host.methods[0].return_type = HostValueType::Bool;
                forged.program.modules[0].host_interface.functions[0].return_type =
                    HostValueType::Bool;
            }
            "method" => host.trait_implementations[0].methods.clear(),
            _ => unreachable!(),
        }
        let decoded = KbcArtifact::from_bytes(&forged.to_bytes().unwrap()).unwrap();
        assert!(
            matches!(
                decoded.validate_for_loader(&ArtifactCompatibility::default()),
                Err(ArtifactValidationError::Bytecode(
                    BytecodeVerificationError::InvalidHostInterface(_)
                ))
            ),
            "{corrupt}"
        );
    }

    let owner = valid.program.modules[0].clone();
    let mut importer = BytecodeModule {
        identity: ModuleIdentity {
            package: PackageId("pkg".into()),
            path: vec!["consumer".into()],
        },
        dependencies: vec![crate::bytecode::ModuleRef::new(0)],
        host_interface: owner.host_interface.clone(),
        ..Default::default()
    };
    assert!(
        crate::bytecode::verify_program(&crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(1),
            modules: vec![owner.clone(), importer.clone()],
        })
        .is_ok()
    );
    importer.host_interface.types[0].trait_implementations[0].trait_arguments =
        vec![HostValueType::Bool];
    assert!(matches!(
        crate::bytecode::verify_program(&crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(1),
            modules: vec![owner, importer],
        }),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
}

#[test]
fn private_host_trait_contracts_survive_encoding_and_reject_tampering() {
    use crate::module::abi::{AbiType, BuiltinType};

    let module = host_trait_test_module("trait Readable<T> { fn get(self) -> T; } fn main() {}");
    assert!(
        !module
            .public_items
            .iter()
            .any(|item| matches!(item, PublicAbiItem::Trait(_)))
    );
    assert_eq!(module.trait_contracts.len(), 1);
    verify_module(&module).unwrap();
    let valid = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let decoded = KbcArtifact::from_bytes(&valid.to_bytes().unwrap()).unwrap();
    decoded
        .validate_for_loader(&ArtifactCompatibility::default())
        .unwrap();
    for corruption in ["missing", "signature"] {
        let mut forged = valid.clone();
        let module = &mut forged.program.modules[0];
        match corruption {
            "missing" => module.trait_contracts.clear(),
            "signature" => {
                module.trait_contracts[0].abi.methods[0].return_type =
                    AbiType::Builtin(BuiltinType::Bool)
            }
            _ => unreachable!(),
        }
        let decoded = KbcArtifact::from_bytes(&forged.to_bytes().unwrap()).unwrap();
        assert!(
            matches!(
                decoded.validate_for_loader(&ArtifactCompatibility::default()),
                Err(ArtifactValidationError::Bytecode(
                    BytecodeVerificationError::InvalidHostInterface(_)
                ))
            ),
            "{corruption}"
        );
    }
    let mut duplicate = valid.program.modules[0].clone();
    duplicate
        .trait_contracts
        .push(duplicate.trait_contracts[0].clone());
    assert!(matches!(
        verify_module(&duplicate),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    let mut public_collision = valid.program.modules[0].clone();
    public_collision.public_items.push(PublicAbiItem::Trait(
        public_collision.trait_contracts[0].abi.clone(),
    ));
    assert!(matches!(
        verify_module(&public_collision),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    let mut foreign_identity = valid.program.modules[0].clone();
    foreign_identity.trait_contracts[0].declaration.module = ModuleIdentity {
        package: PackageId("foreign".into()),
        path: vec!["api".into()],
    };
    assert!(matches!(
        verify_module(&foreign_identity),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
}

#[test]
fn host_trait_standard_bounds_are_rechecked_after_decode() {
    use kagari_common::host_interface::HostValueType;

    let module =
        host_trait_test_module("trait Readable<T: HashKey> { fn get(self) -> T; } fn main() {}");
    verify_module(&module).unwrap();
    let valid = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let mut forged = valid.clone();
    let host = &mut forged.program.modules[0].host_interface.types[0];
    host.trait_implementations[0].trait_arguments = vec![HostValueType::F32];
    host.methods[0].return_type = HostValueType::F32;
    forged.program.modules[0].host_interface.functions[0].return_type = HostValueType::F32;
    let decoded = KbcArtifact::from_bytes(&forged.to_bytes().unwrap()).unwrap();
    assert!(matches!(
        decoded.validate_for_loader(&ArtifactCompatibility::default()),
        Err(ArtifactValidationError::Bytecode(
            BytecodeVerificationError::InvalidHostInterface(_)
        ))
    ));
}

#[test]
fn host_trait_bounds_accept_host_implementation_evidence() {
    use kagari_common::{
        host_interface::{
            HostMethodDeclaration, HostTraitImplementationDeclaration, HostTraitMethodBinding,
            HostValueType,
        },
        identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
    };

    let mut module = host_trait_test_module(
        "trait Marker { fn mark(self) -> i32; } trait Readable<T: Marker> { fn get(self) -> T; } fn main() {}",
    );
    let host = &mut module.host_interface.types[0];
    let host_id = host.id.clone();
    let method = HostMethodDeclaration::new(&host_id, "mark", vec![], HostValueType::I32);
    host.methods.push(method.clone());
    let trait_id = DefinitionId {
        module: module.identity.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Marker".into(),
            occurrence: 0,
        }],
    };
    let mut trait_method = trait_id.clone();
    trait_method.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Method,
        name: "mark".into(),
        occurrence: 0,
    });
    host.trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            trait_id,
            vec![],
            vec![HostTraitMethodBinding {
                trait_method,
                host_method: method.id.clone(),
            }],
        ));
    host.trait_implementations[0].trait_arguments = vec![HostValueType::Opaque(host_id.clone())];
    host.methods[0].return_type = HostValueType::Opaque(host_id);
    module.host_interface.functions[0].return_type = host.methods[0].return_type.clone();
    module
        .host_interface
        .functions
        .push(host.method_contract(&method.id).unwrap());
    verify_module(&module).unwrap();
    let artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module.clone()],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    KbcArtifact::from_bytes(&artifact.to_bytes().unwrap())
        .unwrap()
        .validate_for_loader(&ArtifactCompatibility::default())
        .unwrap();
    module.host_interface.types[0].trait_implementations.pop();
    assert!(matches!(
        verify_module(&module),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
}

#[test]
fn host_trait_script_bounds_are_rechecked_after_decode() {
    use kagari_common::host_interface::HostValueType;

    let module = host_trait_test_module(
        "trait Marker { fn mark(self) -> i32; } impl Marker for i32 { fn mark(self) -> i32 { self } } trait Readable<T: Marker> { fn get(self) -> T; } fn main() {}",
    );
    verify_module(&module).unwrap();
    let valid = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let mut forged = valid.clone();
    let host = &mut forged.program.modules[0].host_interface.types[0];
    host.trait_implementations[0].trait_arguments = vec![HostValueType::Bool];
    host.methods[0].return_type = HostValueType::Bool;
    forged.program.modules[0].host_interface.functions[0].return_type = HostValueType::Bool;
    let decoded = KbcArtifact::from_bytes(&forged.to_bytes().unwrap()).unwrap();
    assert!(matches!(
        decoded.validate_for_loader(&ArtifactCompatibility::default()),
        Err(ArtifactValidationError::Bytecode(
            BytecodeVerificationError::InvalidHostInterface(_)
        ))
    ));
}

#[test]
fn host_trait_bounds_use_imported_script_implementations() {
    use kagari_common::{
        host_interface::HostValueType,
        identity::{ModuleIdentity, PackageId},
        source_database::{SourceDatabase, SourceLayer},
    };

    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "dependency",
            "pub trait Marker { fn mark(self) -> i32; } impl Marker for i32 { fn mark(self) -> i32 { self } }",
        ),
        (
            "root",
            "use pkg::dependency::Marker; trait Readable<T: Marker> { fn get(self) -> T; } fn main() {}",
        ),
    ] {
        let uri = format!("mem://{name}");
        sources
            .bind_module(
                &uri,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        root = Some(sources.set(&uri, text.into(), SourceLayer::Base).unwrap());
    }
    let snapshot = kagari_hir::analysis::AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let checked = snapshot
        .check_program(root.unwrap(), &Default::default())
        .unwrap();
    let ir = crate::program::lower_program_to_ir(&checked, &Default::default()).unwrap();
    let mut program = crate::bytecode::lower_program_to_bytecode(&ir).unwrap();
    let root_index = program.root.index();
    add_readable_host(&mut program.modules[root_index]);
    crate::bytecode::verify_program(&program).unwrap();
    let mut forged = program.clone();
    let root = &mut forged.modules[root_index];
    root.host_interface.types[0].trait_implementations[0].trait_arguments =
        vec![HostValueType::Bool];
    root.host_interface.types[0].methods[0].return_type = HostValueType::Bool;
    root.host_interface.functions[0].return_type = HostValueType::Bool;
    assert!(matches!(
        crate::bytecode::verify_program(&forged),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
    let artifact = KbcArtifact::from_program(program, ArtifactBuildOptions::default()).unwrap();
    KbcArtifact::from_bytes(&artifact.to_bytes().unwrap())
        .unwrap()
        .validate_for_loader(&ArtifactCompatibility::default())
        .unwrap();
}

#[test]
fn source_interface_coercion_links_an_imported_implementation_table() {
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::{SourceDatabase, SourceLayer},
    };

    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, source) in [
        (
            "dependency",
            "pub trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self + 1 } }",
        ),
        (
            "root",
            "use pkg::dependency::Tag; fn accept(value: Tag) -> i32 { value.tag() } fn main() -> i32 { accept(7) }",
        ),
    ] {
        let uri = format!("mem://{name}");
        sources
            .bind_module(
                &uri,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let revision = sources.set(&uri, source.into(), SourceLayer::Base).unwrap();
        if name == "root" {
            root = Some(revision);
        }
    }
    let snapshot = kagari_hir::analysis::AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let checked = snapshot
        .check_program(root.unwrap(), &Default::default())
        .unwrap();
    let ir = crate::program::lower_program_to_ir(&checked, &Default::default()).unwrap();
    let mut forged = ir.clone().into_unverified();
    let root_ir = forged
        .iter_mut()
        .find(|module| module.identity.path == ["root"])
        .unwrap();
    let implementation = root_ir
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.instructions)
        .find_map(|instruction| match instruction {
            crate::module::Instruction::MakeInterface { implementation, .. } => {
                Some(implementation)
            }
            _ => None,
        })
        .unwrap();
    implementation.path.last_mut().unwrap().name = "forged".into();
    assert!(matches!(
        crate::program::verify_program(ir.root().clone(), forged, &Default::default()),
        Err(crate::program::ProgramError {
            kind: crate::program::ProgramErrorKind::InterfaceContract(_),
            ..
        })
    ));
    let mut forged_call = ir.clone().into_unverified();
    let method_slot = forged_call
        .iter_mut()
        .find(|module| module.identity.path == ["root"])
        .unwrap()
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.instructions)
        .find_map(|instruction| match instruction {
            crate::module::Instruction::Call {
                callee: crate::module::CallTarget::InterfaceMethod(contract),
                ..
            } => Some(&mut contract.method_slot),
            _ => None,
        })
        .unwrap();
    *method_slot = 99;
    assert!(matches!(
        crate::program::verify_program(ir.root().clone(), forged_call, &Default::default()),
        Err(crate::program::ProgramError {
            kind: crate::program::ProgramErrorKind::InterfaceContract(_),
            ..
        })
    ));
    let bytecode = crate::bytecode::lower_program_to_bytecode(&ir).unwrap();
    let root_module = &bytecode.modules[bytecode.root.index()];
    assert!(root_module.functions.iter().flat_map(|function| &function.instructions).any(
        |instruction| matches!(instruction, crate::bytecode::BytecodeInstruction::MakeInterface { module, .. } if module.index() != bytecode.root.index())
    ));
    assert!(root_module.functions.iter().flat_map(|function| &function.instructions).any(
        |instruction| matches!(instruction, crate::bytecode::BytecodeInstruction::Call { callee: crate::bytecode::CallTarget::InterfaceMethod { module, .. }, .. } if module.index() != bytecode.root.index())
    ));
    crate::bytecode::verify_program(&bytecode).unwrap();
    let mut invalid = bytecode.clone();
    let call = invalid.modules[bytecode.root.index()]
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.instructions)
        .find_map(|instruction| match instruction {
            crate::bytecode::BytecodeInstruction::Call {
                callee: crate::bytecode::CallTarget::InterfaceMethod { method_slot, .. },
                ..
            } => Some(method_slot),
            _ => None,
        })
        .unwrap();
    *call = 99;
    assert!(crate::bytecode::verify_program(&invalid).is_err());
    let mut wrong_owner = bytecode.clone();
    let owner_slot = wrong_owner.modules[bytecode.root.index()]
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.instructions)
        .find_map(|instruction| match instruction {
            crate::bytecode::BytecodeInstruction::Call {
                callee: crate::bytecode::CallTarget::InterfaceMethod { module, .. },
                ..
            } => Some(module),
            _ => None,
        })
        .unwrap();
    *owner_slot = bytecode.root;
    assert!(crate::bytecode::verify_program(&wrong_owner).is_err());
}

#[test]
fn forged_interface_method_slots_are_rejected_before_execution() {
    let original = common::bytecode_ok(
        "trait Tag { fn tag(self) -> i32; } impl Tag for i32 { fn tag(self) -> i32 { self } } fn read(value: Tag) -> i32 { value.tag() } fn main() -> i32 { read(7) }",
    );
    crate::bytecode::verify_module(&original).unwrap();
    for corruption in ["slot", "owner", "argument"] {
        let mut forged = original.clone();
        let function = forged
            .functions
            .iter_mut()
            .find(|function| function.name == "read")
            .unwrap();
        let instruction = function
            .instructions
            .iter_mut()
            .find(|instruction| {
                matches!(
                    instruction,
                    crate::bytecode::BytecodeInstruction::Call {
                        callee: crate::bytecode::CallTarget::InterfaceMethod { .. },
                        ..
                    }
                )
            })
            .unwrap();
        let crate::bytecode::BytecodeInstruction::Call { callee, args, .. } = instruction else {
            unreachable!()
        };
        let crate::bytecode::CallTarget::InterfaceMethod {
            interface,
            method_slot,
            ..
        } = callee
        else {
            unreachable!()
        };
        match corruption {
            "slot" => *method_slot = 99,
            "owner" => interface.declaration.path.last_mut().unwrap().name = "Other".into(),
            "argument" => args[0] = crate::bytecode::Register::new(999),
            _ => unreachable!(),
        }
        assert!(
            crate::bytecode::verify_module(&forged).is_err(),
            "accepted {corruption}"
        );
    }
}

#[test]
fn private_interface_tables_must_match_their_trait_contract() {
    use crate::module::abi::{AbiType, BuiltinType};

    let original = common::bytecode_ok(
        "trait Readable { fn get(self) -> i32; } struct Counter { val value: i32 } impl Readable for Counter { fn get(self) -> i32 { self.value } } fn main() {}",
    );
    let index = original
        .public_items
        .iter()
        .position(|item| matches!(item, PublicAbiItem::InterfaceTable(_)))
        .unwrap();
    assert_eq!(original.trait_contracts.len(), 1);
    verify_module(&original).unwrap();
    for corruption in ["result", "roster", "missing trait"] {
        let mut forged = original.clone();
        match corruption {
            "result" => {
                let PublicAbiItem::InterfaceTable(table) = &mut forged.public_items[index] else {
                    unreachable!()
                };
                table.methods[0].return_type = AbiType::Builtin(BuiltinType::Bool);
            }
            "roster" => {
                let PublicAbiItem::InterfaceTable(table) = &mut forged.public_items[index] else {
                    unreachable!()
                };
                table.methods.clear();
            }
            "missing trait" => forged.trait_contracts.clear(),
            _ => unreachable!(),
        }
        assert!(
            matches!(
                verify_module(&forged),
                Err(BytecodeVerificationError::InvalidPublicAbi)
            ),
            "{corruption}"
        );
    }
}

#[test]
fn program_rejects_conflicting_host_types_before_linking() {
    use kagari_common::host_interface::{HostTypeDeclaration, HostTypeOwnership};

    let base = HostTypeDeclaration::new("demo.Counter");
    let program = |other: HostTypeDeclaration| crate::bytecode::BytecodeProgram {
        root: crate::bytecode::ModuleRef::new(1),
        modules: vec![
            BytecodeModule {
                identity: ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec!["owner".into()],
                },
                host_interface: kagari_common::host_interface::HostInterface {
                    types: vec![base.clone()],
                    ..Default::default()
                },
                ..Default::default()
            },
            BytecodeModule {
                identity: ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec!["consumer".into()],
                },
                dependencies: vec![crate::bytecode::ModuleRef::new(0)],
                host_interface: kagari_common::host_interface::HostInterface {
                    types: vec![other],
                    ..Default::default()
                },
                ..Default::default()
            },
        ],
    };
    let mut documentation_only = base.clone();
    documentation_only.documentation = "editor help".into();
    crate::bytecode::verify_program(&program(documentation_only)).unwrap();

    let mut conflict = base.clone();
    conflict.ownership = HostTypeOwnership::HostRoot;
    assert!(matches!(
        crate::bytecode::verify_program(&program(conflict)),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
}

#[test]
fn verifier_rejects_iter_get_scalar_result_and_wrong_arity() {
    let module = common::bytecode_ok(
        "fn main() -> bool { val a = [7]; std::iter::get(a, a.len()).is_none() }",
    );
    let mut scalar_result = module.clone();
    let function = &mut scalar_result.functions[0];
    let dst = function
        .instructions
        .iter()
        .find_map(|instruction| {
            if let BytecodeInstruction::Call {
                dst,
                callee: CallTarget::StandardIntrinsic(StandardIntrinsic::IterGet),
                ..
            } = instruction
            {
                *dst
            } else {
                None
            }
        })
        .unwrap();
    function.metadata.registers[dst.index()] = ValueType::I32;
    function
        .metadata
        .roots
        .registers
        .retain(|register| *register != dst);
    assert!(matches!(
        verify_module(&scalar_result),
        Err(BytecodeVerificationError::TypeMismatch {
            expected: ValueType::HeapObject,
            found: ValueType::I32,
            ..
        })
    ));

    let mut wrong_arity = module;
    for instruction in &mut wrong_arity.functions[0].instructions {
        if let BytecodeInstruction::Call {
            callee: CallTarget::StandardIntrinsic(StandardIntrinsic::IterGet),
            args,
            ..
        } = instruction
        {
            args.pop();
        }
    }
    assert!(matches!(
        verify_module(&wrong_arity),
        Err(
            BytecodeVerificationError::StandardIntrinsicSignatureMismatch {
                intrinsic: StandardIntrinsic::IterGet,
                ..
            }
        )
    ));
}

#[test]
fn encoded_root_layout_must_cover_exact_heap_slots() {
    let module = common::bytecode_ok("fn main() -> i32 { val values = [7]; values[0] }");
    let function = &module.functions[0];
    assert!(
        !function.metadata.roots.locals.is_empty() || !function.metadata.roots.registers.is_empty()
    );
    verify_module(&module).unwrap();
    let artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    for corruption in ["missing", "extra"] {
        let mut forged = artifact.clone();
        let roots = &mut forged.program.modules[0].functions[0].metadata.roots;
        if corruption == "missing" {
            if !roots.registers.is_empty() {
                roots.registers.pop();
            } else {
                roots.locals.pop();
            }
        } else {
            roots.registers.push(Register::new(0));
        }
        assert!(matches!(
            verify_module(&forged.program.modules[0]),
            Err(BytecodeVerificationError::InvalidRootLayout { .. })
        ));
        let decoded = KbcArtifact::from_bytes(&forged.to_bytes().unwrap()).unwrap();
        assert!(matches!(
            decoded.validate_for_loader(&ArtifactCompatibility::default()),
            Err(ArtifactValidationError::Bytecode(
                BytecodeVerificationError::InvalidRootLayout { .. }
            ))
        ));
    }
}

#[test]
fn const_abi_uses_evaluated_values_and_preserves_float_bits() {
    let artifact = |source: &str| {
        KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![common::bytecode_ok(source)],
            },
            Default::default(),
        )
        .unwrap()
    };
    let expression = artifact("pub const VALUE: i32 = 6 * 7;");
    let literal = artifact("pub const VALUE: i32 = 42;");
    assert_eq!(
        expression.verification.public_abi_fingerprints,
        literal.verification.public_abi_fingerprints
    );
    let positive_zero = artifact("pub const VALUE: f32 = 0.0;");
    let negative_zero = artifact("pub const VALUE: f32 = -0.0;");
    assert_ne!(
        positive_zero.verification.public_abi_fingerprints,
        negative_zero.verification.public_abi_fingerprints
    );
    for version in 1..crate::bytecode::KBC_ARTIFACT_FORMAT_VERSION {
        let mut old = literal.clone();
        old.header.format_version = version;
        assert!(KbcArtifact::from_bytes(&old.to_bytes().unwrap()).is_err());
        assert!(matches!(
            old.validate_for_loader(&ArtifactCompatibility {
                format_version: version,
                ..Default::default()
            }),
            Err(ArtifactValidationError::FormatVersionMismatch { .. })
        ));
    }
}

#[test]
fn lowers_function_metadata_into_bytecode() {
    let bytecode = common::bytecode_ok("fn add(a: i32, b: i32) -> i32 { val c = a + b; c }");
    let function = &bytecode.functions[0];

    assert_eq!(function.id, FunctionRef::new(0));
    assert_eq!(function.name, "add");
    assert_eq!(function.parameter_count, 2);
    assert_eq!(function.local_count, 3);
    assert!(function.register_count >= 4);
    assert_eq!(
        function.metadata.params,
        vec![ValueType::I32, ValueType::I32]
    );
    assert_eq!(function.metadata.return_type, ValueType::I32);
    assert_eq!(
        function.metadata.locals[..3],
        [ValueType::I32, ValueType::I32, ValueType::I32]
    );
    assert_eq!(
        function.metadata.registers.len(),
        usize::from(function.register_count)
    );
}

#[test]
fn lowers_debugger_metadata_into_bytecode() {
    let bytecode = common::bytecode_ok(
        r#"
fn main(value: i32) -> i32 {
    val next = value + 1;
    print("debug");
    next
}
"#,
    );
    let function = &bytecode.functions[0];
    let debug = &function.metadata.debug;

    assert_eq!(debug.source_spans.len(), function.instructions.len());
    assert_eq!(debug.line_table.len(), function.instructions.len());
    assert_eq!(debug.frame_layout.locals, function.metadata.locals);
    assert_eq!(debug.frame_layout.registers, function.metadata.registers);
    assert!(
        debug
            .safe_debug_points
            .iter()
            .any(|point| point.kind == SafeDebugPointKind::FunctionEntry)
    );
    assert!(
        debug
            .safe_debug_points
            .iter()
            .any(|point| point.kind == SafeDebugPointKind::CallBoundary)
    );
    assert!(
        debug
            .safe_debug_points
            .iter()
            .any(|point| point.kind == SafeDebugPointKind::FunctionReturn)
    );
    assert!(
        debug
            .local_live_ranges
            .iter()
            .any(|range| range.name == "value" && range.is_parameter)
    );
    assert!(
        debug
            .local_live_ranges
            .iter()
            .any(|range| range.name == "next" && !range.is_parameter)
    );

    let artifact_debug = DebugMetadata::from_module(&bytecode);
    assert!(!artifact_debug.stripped);
    assert_eq!(artifact_debug.functions.len(), bytecode.functions.len());
    assert!(artifact_debug.debug_names.iter().any(|name| name == "main"));
}

#[test]
fn populates_bytecode_tables_and_effect_metadata() {
    let bytecode = common::bytecode_ok(
        r#"
fn add(a: i32, b: i32) -> i32 { a + b }

fn main() -> i32 {
    print("ok");
    add(1, 2)
}
"#,
    );
    let main = bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");

    assert!(bytecode.constants.iter().any(|constant| matches!(
        constant,
        crate::bytecode::ConstantOperand::Str(text) if text == "ok"
    )));
    assert!(bytecode.types.contains(&ValueType::I32));
    assert!(bytecode.types.contains(&ValueType::Str));
    assert_eq!(bytecode.function_table.len(), bytecode.functions.len());
    assert_eq!(bytecode.function_table[0].name, "add");
    assert_eq!(
        bytecode.function_table[0].params,
        vec![ValueType::I32, ValueType::I32]
    );
    assert_eq!(bytecode.function_table[0].return_type, ValueType::I32);
    assert!(main.metadata.effects.calls);
    assert!(main.metadata.effects.touches_runtime);
    assert!(verify_module(&bytecode).is_ok());
}

#[test]
fn builds_versioned_kbc_artifact_metadata() {
    let mut module = common::bytecode_ok(
        r#"
fn add(a: i32, b: i32) -> i32 { a + b }
fn main() -> i32 { add(1, 2) }
"#,
    );
    let identity = ModuleIdentity {
        package: PackageId("pkg".into()),
        path: vec!["main".into()],
    };
    module.identity = identity.clone();
    for function in &mut module.functions {
        function.identity.as_mut().unwrap().declaration.module = identity.clone();
    }
    for record in &mut module.function_table {
        record.identity.as_mut().unwrap().declaration.module = identity.clone();
    }
    let dependency_module = BytecodeModule {
        identity: ModuleIdentity {
            package: PackageId("pkg".into()),
            path: vec!["math".into()],
        },
        ..Default::default()
    };
    let dependency = DependencyFingerprint {
        module_id: dependency_module.identity.clone(),
        fingerprint: ArtifactFingerprint::of_serialized(&dependency_module),
    };
    module.dependencies = vec![crate::bytecode::ModuleRef::new(0)];
    let artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(1),
            modules: vec![dependency_module, module],
        },
        ArtifactBuildOptions {
            security_profile: Some("dev".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(artifact.header.magic, KBC_MAGIC);
    assert_eq!(artifact.header.module_identity, identity);
    assert!(artifact.header.content_hash != ArtifactFingerprint::empty());
    assert!(
        artifact.tables.sections.iter().any(|section| {
            section.id == ArtifactSectionId::Constants && section.record_count > 0
        })
    );
    assert!(artifact.tables.sections.iter().any(|section| {
        section.id == ArtifactSectionId::Functions && section.record_count == 2
    }));
    assert!(artifact.tables.sections.iter().any(|section| {
        section.id == ArtifactSectionId::Verification && section.record_count == 2
    }));
    assert!(artifact.verification.bytecode_verified);
    assert_eq!(artifact.verification.function_layouts.len(), 2);
    assert_eq!(
        artifact.verification.loader.dependency_fingerprints,
        vec![dependency]
    );
    assert_eq!(
        artifact.verification.loader.security_profile.as_deref(),
        Some("dev")
    );

    let requirements = ArtifactCompatibility {
        module_identity: Some(identity),
        dependency_fingerprints: Some(artifact.verification.loader.dependency_fingerprints.clone()),
        security_profile: Some("dev".to_owned()),
        ..Default::default()
    };
    assert!(artifact.validate_for_loader(&requirements).is_ok());
}

#[test]
fn serializes_kbc_artifact_bytes_for_loader_execution() {
    let module = common::bytecode_ok("fn main() -> i32 { 1 }");
    let artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();

    let bytes = artifact.to_bytes().expect("artifact should encode");
    let decoded = KbcArtifact::from_bytes(&bytes).expect("artifact should decode");

    assert_eq!(decoded.header, artifact.header);
    assert_eq!(
        decoded.program.modules[decoded.program.root.index()]
            .functions
            .len(),
        artifact.program.modules[artifact.program.root.index()]
            .functions
            .len()
    );
    decoded
        .validate_for_loader(&ArtifactCompatibility::default())
        .expect("decoded artifact should validate");
}

#[test]
fn fingerprints_public_module_abi_records() {
    use crate::module::abi::{AbiType, NominalAbiType};
    use kagari_hir::types::BuiltinType;
    let module = common::bytecode_ok(
        r#"
pub const VERSION: i32 = 1;

pub struct Player {
    val name: String,
    var score: i32,
}

pub enum Status {
    Ready,
    Waiting,
}

pub trait Display {
    fn show(self) -> String;
}

impl Display for Player {
    fn show(self) -> String {
        self.name
    }
}

pub fn greet(player: Player) -> String {
    player.name
}
"#,
    );

    let player = AbiType::Struct(NominalAbiType {
        declaration: module
            .structures
            .iter()
            .find(|layout| layout.name() == "Player")
            .unwrap()
            .declaration
            .clone(),
        arguments: vec![],
    });
    assert!(module.public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::Const(item)
            if item.name == "VERSION" && item.ty == AbiType::Builtin(BuiltinType::I32) && item.value == "const-v1:i32:1"
    )));
    assert!(module.public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::Type(item)
            if item.name == "Player"
                && item.kind == TypeAbiKind::Struct
                && item.fields.iter().any(|field| {
                    field.name == "score" && field.ty == AbiType::Builtin(BuiltinType::I32) && field.mutable
                })
    )));
    assert!(module.public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::Type(item)
            if item.name == "Status"
                && item.kind == TypeAbiKind::Enum
                && item.variants.iter().any(|variant| variant.name == "Ready")
    )));
    assert!(module.public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::Trait(item)
            if item.name == "Display"
                && item.methods.iter().any(|method| {
                    method.name == "show" && method.return_type == AbiType::Builtin(BuiltinType::String)
                })
    )));
    assert!(module.public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::InterfaceTable(item)
            if matches!(&item.trait_type, AbiType::Trait(ty) if ty.declaration.module == module.identity && ty.declaration.path.last().unwrap().name == "Display")
                && item.for_type == player
                && item.methods.iter().any(|method| method.name == "show")
    )));
    let table = module
        .public_items
        .iter()
        .find(|item| matches!(item, PublicAbiItem::InterfaceTable(_)))
        .unwrap();
    let mut same_label = table.clone();
    let PublicAbiItem::InterfaceTable(other) = &mut same_label else {
        unreachable!()
    };
    other.declaration.module.package.0 = "other-package".into();
    assert_eq!(table.name(), same_label.name());
    assert_ne!(table.fingerprint_name(), same_label.fingerprint_name());
    assert!(module.public_items.iter().any(|item| matches!(
        item,
        PublicAbiItem::Function(item)
            if item.name == "greet"
                && item.params.len() == 1
                && item.params[0].ty == player
                && item.return_type == AbiType::Builtin(BuiltinType::String)
    )));

    let artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let names = artifact
        .verification
        .public_abi_fingerprints
        .iter()
        .map(|fingerprint| fingerprint.name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"const:VERSION"));
    assert!(names.contains(&"type:Player"));
    assert!(names.contains(&"type:Status"));
    assert!(names.contains(&"trait:Display"));
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("interface_table:"))
            .count(),
        1
    );
    assert!(!names.contains(&"interface_table:Player as Display"));
    assert!(names.contains(&"function:greet"));
}

#[test]
fn abi_fingerprints_change_with_public_signatures_and_path_descriptors() {
    let first = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![common::bytecode_ok("pub fn main() -> i32 { 1 }")],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let second = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![common::bytecode_ok(
                "pub fn main(value: i32) -> i32 { value }",
            )],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let first_main = first
        .verification
        .public_abi_fingerprints
        .iter()
        .find(|fingerprint| fingerprint.name == "function:main")
        .expect("public main ABI should be fingerprinted");
    let second_main = second
        .verification
        .public_abi_fingerprints
        .iter()
        .find(|fingerprint| fingerprint.name == "function:main")
        .expect("public main ABI should be fingerprinted");
    assert_ne!(first_main.fingerprint, second_main.fingerprint);

    let path_artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![BytecodeModule {
                types: vec![ValueType::HostHandle, ValueType::I32],
                paths: vec![PathRecord {
                    contract_fingerprint: 0,
                    id: PathId::new(0),
                    root_ty: ValueType::HostHandle,
                    result_ty: ValueType::I32,
                    read_only: false,
                    debug_name: "Actor.health".to_owned(),
                }],
                ..Default::default()
            }],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    assert_eq!(path_artifact.verification.typed_path_fingerprints.len(), 1);
    assert_ne!(
        path_artifact.verification.typed_path_fingerprints[0].fingerprint,
        ArtifactFingerprint::empty()
    );
    assert_eq!(
        path_artifact.verification.typed_path_fingerprints,
        path_artifact.verification.loader.typed_path_fingerprints
    );
    let mut renamed = path_artifact.program.clone();
    renamed.modules[0].paths[0].debug_name = "diagnostic label only".into();
    let renamed = KbcArtifact::from_program(renamed, ArtifactBuildOptions::default()).unwrap();
    assert_eq!(
        path_artifact.verification.typed_path_fingerprints,
        renamed.verification.typed_path_fingerprints
    );
    let mut changed = renamed.program;
    changed.modules[0].paths[0].contract_fingerprint = 42;
    let changed = KbcArtifact::from_program(changed, ArtifactBuildOptions::default()).unwrap();
    assert_ne!(
        path_artifact.verification.typed_path_fingerprints,
        changed.verification.typed_path_fingerprints
    );
}

#[test]
fn rejects_previous_runtime_abis_even_when_loader_requests_them() {
    for version in 5..33 {
        let previous = format!("kagari-runtime-abi-v{version}");
        let artifact = KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![common::bytecode_ok("fn main() -> i32 { 1 }")],
            },
            ArtifactBuildOptions {
                runtime_abi_version: previous.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        for runtime_abi_version in [
            crate::bytecode::KAGARI_RUNTIME_ABI_VERSION,
            previous.as_str(),
        ] {
            let requirements = ArtifactCompatibility {
                runtime_abi_version: runtime_abi_version.into(),
                ..Default::default()
            };
            assert!(matches!(
                decoded.validate_for_loader(&requirements),
                Err(ArtifactValidationError::RuntimeAbiMismatch { .. })
            ));
        }
    }
}

#[test]
fn rejects_helper_abis_without_commit_fault_or_cancellation_status() {
    for previous in [
        "kagari-runtime-helper-abi-v3",
        "kagari-runtime-helper-abi-v4",
    ] {
        let artifact = KbcArtifact::from_program(
            crate::bytecode::BytecodeProgram {
                root: crate::bytecode::ModuleRef::new(0),
                modules: vec![common::bytecode_ok("fn main() -> i32 { 1 }")],
            },
            ArtifactBuildOptions {
                runtime_helper_abi_version: previous.into(),
                ..Default::default()
            },
        )
        .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        for version in [previous, crate::bytecode::KAGARI_RUNTIME_HELPER_ABI_VERSION] {
            let requirements = ArtifactCompatibility {
                runtime_helper_abi_version: version.into(),
                ..Default::default()
            };
            assert!(matches!(
                decoded.validate_for_loader(&requirements),
                Err(ArtifactValidationError::RuntimeHelperAbiMismatch { .. })
            ));
        }
    }
}

#[test]
fn rejects_incompatible_kbc_artifact_metadata_before_loading() {
    let module = common::bytecode_ok("fn main() -> i32 { 1 }");
    let mut artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![module],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    let requirements = ArtifactCompatibility {
        runtime_abi_version: "other-runtime".to_owned(),
        ..Default::default()
    };

    assert!(matches!(
        artifact.validate_for_loader(&requirements),
        Err(ArtifactValidationError::RuntimeAbiMismatch { .. })
    ));
    assert_eq!(
        artifact
            .validate_for_loader(&requirements)
            .unwrap_err()
            .code(),
        "KG_ARTIFACT_RUNTIME_ABI_MISMATCH"
    );

    let requirements = ArtifactCompatibility::default();
    artifact.program.modules[artifact.program.root.index()]
        .source_name
        .push_str("changed");
    assert!(matches!(
        artifact.validate_for_loader(&requirements),
        Err(ArtifactValidationError::ContentHashMismatch)
    ));
    assert_eq!(
        artifact
            .validate_for_loader(&requirements)
            .unwrap_err()
            .code(),
        "KG_ARTIFACT_CONTENT_HASH_MISMATCH"
    );

    let artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![common::bytecode_ok("fn main() -> i32 { 1 }")],
        },
        Default::default(),
    )
    .unwrap();
    let requirements = ArtifactCompatibility {
        dependency_fingerprints: Some(vec![DependencyFingerprint {
            module_id: ModuleIdentity::single_file("missing.kgr"),
            fingerprint: ArtifactFingerprint::of_str("expected dependency"),
        }]),
        ..Default::default()
    };
    assert!(matches!(
        artifact.validate_for_loader(&requirements),
        Err(ArtifactValidationError::DependencyFingerprintMismatch)
    ));
    assert_eq!(
        artifact
            .validate_for_loader(&requirements)
            .unwrap_err()
            .code(),
        "KG_ARTIFACT_DEPENDENCY_FINGERPRINT_MISMATCH"
    );
}

#[test]
fn verifier_rejects_malformed_register_local_and_control_flow_bytecode() {
    let mut invalid_register = common::bytecode_ok("fn main() -> i32 { 1 }");
    invalid_register.functions[0].instructions[0] = BytecodeInstruction::LoadConst {
        dst: Register::new(999),
        constant: crate::bytecode::ConstantOperand::I32(1),
    };
    assert!(matches!(
        verify_module(&invalid_register),
        Err(BytecodeVerificationError::InvalidRegister { .. })
    ));
    assert_eq!(
        verify_module(&invalid_register).unwrap_err().code(),
        "KG_BYTECODE_INVALID_REGISTER"
    );

    let mut invalid_local = common::bytecode_ok("fn main() -> i32 { val value = 1; value }");
    invalid_local.functions[0].instructions[1] = BytecodeInstruction::StoreLocal {
        local: LocalSlot::new(999),
        src: Register::new(0),
    };
    assert!(matches!(
        verify_module(&invalid_local),
        Err(BytecodeVerificationError::InvalidLocal { .. })
    ));
    assert_eq!(
        verify_module(&invalid_local).unwrap_err().code(),
        "KG_BYTECODE_INVALID_LOCAL"
    );

    let mut invalid_jump = common::bytecode_ok("fn main() -> i32 { if true { 1 } else { 2 } }");
    invalid_jump.functions[0]
        .metadata
        .control_flow_targets
        .push(JumpTarget::new(usize::MAX));
    assert!(matches!(
        verify_module(&invalid_jump),
        Err(BytecodeVerificationError::InvalidJumpTarget { .. })
    ));
    assert_eq!(
        verify_module(&invalid_jump).unwrap_err().code(),
        "KG_BYTECODE_INVALID_JUMP_TARGET"
    );
}

#[test]
fn verifier_rejects_type_inconsistent_bytecode() {
    let mut bytecode = common::bytecode_ok("fn main() -> i32 { 1 }");
    bytecode.functions[0].metadata.return_type = ValueType::Bool;
    bytecode.function_table[0].return_type = ValueType::Bool;
    bytecode.types.push(ValueType::Bool);

    assert!(matches!(
        verify_module(&bytecode),
        Err(BytecodeVerificationError::TypeMismatch {
            context: "return value",
            expected: ValueType::Bool,
            found: ValueType::I32,
            ..
        })
    ));
}

#[test]
fn stdlib_verifier_rejects_invalid_standard_intrinsic_signatures() {
    let mut bytecode = common::bytecode_ok(
        r#"
fn main(value: String) -> usize {
    value.len_chars()
}
"#,
    );
    let call = bytecode.functions[0]
        .instructions
        .iter_mut()
        .find_map(|instruction| {
            let BytecodeInstruction::Call { callee, .. } = instruction else {
                return None;
            };
            Some(callee)
        })
        .expect("expected standard intrinsic call");
    *call = CallTarget::StandardIntrinsic(StandardIntrinsic::MathSqrt);

    assert!(matches!(
        verify_module(&bytecode),
        Err(BytecodeVerificationError::TypeMismatch {
            context: "standard intrinsic argument",
            expected: ValueType::F64,
            found: ValueType::Str,
            ..
        })
    ));

    let artifact = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![bytecode],
        },
        ArtifactBuildOptions::default(),
    );
    assert!(matches!(
        artifact,
        Err(ArtifactValidationError::Bytecode(
            BytecodeVerificationError::TypeMismatch {
                context: "standard intrinsic argument",
                ..
            }
        ))
    ));
}

#[test]
fn verifier_rejects_invalid_aggregate_writes() {
    let module = BytecodeModule {
        types: vec![
            ValueType::Unit,
            ValueType::Bool,
            ValueType::I32,
            ValueType::HeapObject,
        ],
        structures: common::bytecode_ok("struct Point { var x: i32 }").structures,
        function_table: vec![crate::bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: "write_bad_field".to_owned(),
            params: Vec::new(),
            return_type: ValueType::Unit,
            effects: crate::module::EffectSet::aggregate_write(),
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            identity: None,
            name: "write_bad_field".to_owned(),
            parameter_count: 0,
            local_count: 0,
            register_count: 2,
            metadata: FunctionMetadata {
                return_type: ValueType::Unit,
                registers: vec![ValueType::HeapObject, ValueType::Bool],
                roots: crate::bytecode::RootSlotLayout {
                    registers: vec![Register::new(0)],
                    ..Default::default()
                },
                effects: crate::module::EffectSet::aggregate_write(),
                ..Default::default()
            },
            instructions: vec![
                BytecodeInstruction::WriteAggregateField {
                    base: Register::new(0),
                    field: FieldRef {
                        structure: StructId::new(0),
                        slot: 0,
                    },
                    value: Register::new(1),
                },
                BytecodeInstruction::Return(None),
            ],
        }],
        ..Default::default()
    };

    assert!(matches!(
        verify_module(&module),
        Err(BytecodeVerificationError::TypeMismatch {
            context: "aggregate field value",
            expected: ValueType::I32,
            found: ValueType::Bool,
            ..
        })
    ));
}

#[test]
fn verifier_rejects_unresolved_and_read_only_typed_paths() {
    let unresolved_path = BytecodeModule {
        types: vec![ValueType::HostHandle, ValueType::I32],
        paths: vec![PathRecord {
            contract_fingerprint: 0,
            id: PathId::new(0),
            root_ty: ValueType::HostHandle,
            result_ty: ValueType::I32,
            read_only: false,
            debug_name: "Actor.health".to_owned(),
        }],
        function_table: vec![crate::bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: "read_missing_path".to_owned(),
            params: vec![ValueType::HostHandle],
            return_type: ValueType::I32,
            effects: crate::module::EffectSet::path_read(),
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            identity: None,
            name: "read_missing_path".to_owned(),
            parameter_count: 1,
            local_count: 1,
            register_count: 2,
            metadata: FunctionMetadata {
                params: vec![ValueType::HostHandle],
                return_type: ValueType::I32,
                locals: vec![ValueType::HostHandle],
                registers: vec![ValueType::HostHandle, ValueType::I32],
                effects: crate::module::EffectSet::path_read(),
                ..Default::default()
            },
            instructions: vec![
                BytecodeInstruction::ReadPath {
                    dst: Register::new(1),
                    root_or_view: Register::new(0),
                    path: PathId::new(99),
                    dynamic_args: Vec::new(),
                },
                BytecodeInstruction::Return(Some(Register::new(1))),
            ],
        }],
        ..Default::default()
    };
    assert!(matches!(
        verify_module(&unresolved_path),
        Err(BytecodeVerificationError::InvalidPathId { .. })
    ));

    let read_only_path = BytecodeModule {
        types: vec![ValueType::Unit, ValueType::HostHandle, ValueType::I32],
        paths: vec![PathRecord {
            contract_fingerprint: 0,
            id: PathId::new(0),
            root_ty: ValueType::HostHandle,
            result_ty: ValueType::I32,
            read_only: true,
            debug_name: "Actor.id".to_owned(),
        }],
        function_table: vec![crate::bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: "write_readonly_path".to_owned(),
            params: vec![ValueType::HostHandle, ValueType::I32],
            return_type: ValueType::Unit,
            effects: crate::module::EffectSet::path_write(),
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            identity: None,
            name: "write_readonly_path".to_owned(),
            parameter_count: 2,
            local_count: 2,
            register_count: 2,
            metadata: FunctionMetadata {
                params: vec![ValueType::HostHandle, ValueType::I32],
                return_type: ValueType::Unit,
                locals: vec![ValueType::HostHandle, ValueType::I32],
                registers: vec![ValueType::HostHandle, ValueType::I32],
                effects: crate::module::EffectSet::path_write(),
                ..Default::default()
            },
            instructions: vec![
                BytecodeInstruction::SetPath {
                    root_or_view: Register::new(0),
                    path: PathId::new(0),
                    dynamic_args: Vec::new(),
                    value: Register::new(1),
                },
                BytecodeInstruction::Return(None),
            ],
        }],
        ..Default::default()
    };
    assert!(matches!(
        verify_module(&read_only_path),
        Err(BytecodeVerificationError::ReadOnlyPath { .. })
    ));
}

#[test]
fn verifier_rejects_malformed_debug_metadata() {
    let mut bytecode = common::bytecode_ok("fn main() -> i32 { 1 }");
    let function = &mut bytecode.functions[0];
    let mut point = function.metadata.debug.safe_debug_points[0].clone();
    point.instruction_offset = function.instructions.len();
    function.metadata.debug.safe_debug_points.push(point);

    assert!(matches!(
        verify_module(&bytecode),
        Err(BytecodeVerificationError::InvalidJumpTarget { .. })
    ));
}

#[test]
fn lowers_arithmetic_into_real_bytecode_instructions() {
    let bytecode = common::bytecode_ok("fn add(a: i32, b: i32) -> i32 { val c = a + b; c }");
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Binary {
            op: BinaryOp::Add,
            ..
        }
    )));
}

#[test]
fn flattens_branch_targets_to_instruction_offsets() {
    let bytecode = common::bytecode_ok("fn main() -> i32 { if true { 1 } else { 2 } }");
    let function = &bytecode.functions[0];

    let targets = function
        .instructions
        .iter()
        .filter_map(|instruction| match instruction {
            BytecodeInstruction::Branch {
                then_target,
                else_target,
                ..
            } => Some([then_target.index(), else_target.index()]),
            BytecodeInstruction::Jump { target } => Some([target.index(), target.index()]),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();

    assert!(!targets.is_empty());
    assert!(
        targets
            .iter()
            .all(|target| *target < function.instructions.len())
    );
}

#[test]
fn lowers_direct_function_calls_to_function_refs() {
    let bytecode = common::bytecode_ok(
        r#"
fn callee() -> i32 { 1 }
fn caller() -> i32 { callee() }
"#,
    );
    let function = &bytecode.functions[1];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::Function(_),
            ..
        }
    )));
}

#[test]
fn lowers_unary_and_short_circuit_expressions() {
    let bytecode = common::bytecode_ok("fn main() -> bool { !false && true }");
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Unary {
            op: UnaryOp::Not,
            ..
        }
    )));

    let branch_count = function
        .instructions
        .iter()
        .filter(|instruction| matches!(instruction, BytecodeInstruction::Branch { .. }))
        .count();
    assert!(branch_count >= 1);
}

#[test]
fn lowers_loops_and_loop_control_to_jumps() {
    let bytecode = common::bytecode_ok(
        r#"
fn main() -> () {
    while true { break; }
    loop { continue; }
}
"#,
    );
    let function = &bytecode.functions[0];

    let jump_count = function
        .instructions
        .iter()
        .filter(|instruction| matches!(instruction, BytecodeInstruction::Jump { .. }))
        .count();
    assert!(jump_count >= 3);

    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::Branch { .. }))
    );
}

#[test]
fn lowers_aggregate_and_access_instructions() {
    let bytecode = common::bytecode_ok(
        r#"
struct Point { var x: i32 }

fn main() -> () {
    val tuple = (1, 2);
    val array = [1, 2];
    val point = Point { x: 1 };
    tuple;
    array[0];
    point.x;
}
"#,
    );
    let function = &bytecode.functions[0];

    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::MakeTuple { .. }))
    );
    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::MakeArray { .. }))
    );
    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::MakeStruct { .. }))
    );
    assert!(
        function.instructions.iter().any(|instruction| matches!(
            instruction,
            BytecodeInstruction::ReadAggregateIndex { .. }
        ))
    );
    assert!(
        function.instructions.iter().any(|instruction| matches!(
            instruction,
            BytecodeInstruction::ReadAggregateField { .. }
        ))
    );
    assert!(
        bytecode
            .structures
            .iter()
            .flat_map(|layout| &layout.fields)
            .any(|field| field.name == "x")
    );
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::ReadAggregateField { field, .. }
            if bytecode.structures.get(field.structure.index()).and_then(|layout| layout.fields.get(field.slot as usize)).is_some_and(|record| record.name == "x")
    )));
}

#[test]
fn verifier_accepts_resolved_typed_path_instructions() {
    let mut module = BytecodeModule {
        types: vec![ValueType::HostHandle, ValueType::I32],
        paths: vec![PathRecord {
            contract_fingerprint: 0,
            id: PathId::new(0),
            root_ty: ValueType::HostHandle,
            result_ty: ValueType::I32,
            read_only: false,
            debug_name: "Actor.health".to_owned(),
        }],
        function_table: vec![crate::bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: "read_health".to_owned(),
            params: vec![ValueType::HostHandle],
            return_type: ValueType::I32,
            effects: crate::module::EffectSet::path_read(),
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            identity: None,
            name: "read_health".to_owned(),
            parameter_count: 1,
            local_count: 1,
            register_count: 2,
            metadata: FunctionMetadata {
                params: vec![ValueType::HostHandle],
                return_type: ValueType::I32,
                locals: vec![ValueType::HostHandle],
                registers: vec![ValueType::HostHandle, ValueType::I32],
                effects: crate::module::EffectSet::path_read(),
                ..Default::default()
            },
            instructions: vec![
                BytecodeInstruction::ReadPath {
                    dst: Register::new(1),
                    root_or_view: Register::new(0),
                    path: PathId::new(0),
                    dynamic_args: Vec::new(),
                },
                BytecodeInstruction::Return(Some(Register::new(1))),
            ],
        }],
        ..Default::default()
    };

    assert!(verify_module(&module).is_ok());
    module.paths[0].root_ty = ValueType::HeapObject;
    assert_eq!(
        verify_module(&module),
        Err(BytecodeVerificationError::InvalidPathLayout)
    );
}

#[test]
fn lowers_named_match_pattern_to_local_traffic() {
    let bytecode =
        common::bytecode_ok("fn main(value: i32) -> i32 { match value { bound => bound } }");
    let function = &bytecode.functions[0];

    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::StoreLocal { .. }))
    );
    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::LoadLocal { .. }))
    );
}

#[test]
fn lowers_type_of_builtin_to_runtime_helper_call() {
    let bytecode = common::bytecode_ok("fn main() -> String { type_of(7) }");
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectTypeOf),
            ..
        }
    )));
}

#[test]
fn reflection_helper_operands_are_checked_before_loading() {
    let valid = common::bytecode_ok("fn main() -> String { type_of(7) }");
    let mut wrong_arity = valid.clone();
    let call = wrong_arity.functions[0]
        .instructions
        .iter_mut()
        .find(|instruction| matches!(instruction, BytecodeInstruction::Call { .. }))
        .unwrap();
    let BytecodeInstruction::Call { args, .. } = call else {
        unreachable!()
    };
    args.clear();
    assert!(matches!(
        verify_module(&wrong_arity),
        Err(BytecodeVerificationError::InvalidOperation { .. })
    ));

    let mut wrong_result = valid.clone();
    let call = wrong_result.functions[0]
        .instructions
        .iter_mut()
        .find(|instruction| matches!(instruction, BytecodeInstruction::Call { .. }))
        .unwrap();
    let BytecodeInstruction::Call { dst, args, .. } = call else {
        unreachable!()
    };
    *dst = Some(args[0]);
    assert!(matches!(
        verify_module(&wrong_result),
        Err(BytecodeVerificationError::TypeMismatch { .. })
    ));

    let mut wrong_field_base = valid;
    let call = wrong_field_base.functions[0]
        .instructions
        .iter_mut()
        .find(|instruction| matches!(instruction, BytecodeInstruction::Call { .. }))
        .unwrap();
    let BytecodeInstruction::Call { callee, .. } = call else {
        unreachable!()
    };
    *callee = CallTarget::RuntimeHelper(RuntimeHelper::ReflectGetField("x".into()));
    assert!(matches!(
        verify_module(&wrong_field_base),
        Err(BytecodeVerificationError::TypeMismatch { .. })
    ));

    let mut wrong_index = common::bytecode_ok("fn main() -> [i32] { set_index([1], 0, 2) }");
    let call = wrong_index.functions[0]
        .instructions
        .iter_mut()
        .find(|instruction| {
            matches!(
                instruction,
                BytecodeInstruction::Call {
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetIndex),
                    ..
                }
            )
        })
        .unwrap();
    let BytecodeInstruction::Call { args, .. } = call else {
        unreachable!()
    };
    args[1] = args[0];
    assert!(matches!(
        verify_module(&wrong_index),
        Err(BytecodeVerificationError::InvalidOperation { .. })
    ));
}

#[test]
fn lowers_reflection_field_builtins_to_runtime_helper_calls() {
    let bytecode = common::bytecode_ok(
        r#"
struct Point { var x: i32 }

fn main() -> Point {
    val point = Point { x: 1 };
    val next = set_field(point, "x", 9);
    get_field(next, "x");
    next
}
"#,
    );
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetField(field)),
            ..
        } if field == "x"
    )));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectGetField(field)),
            ..
        } if field == "x"
    )));
}

#[test]
fn lowers_set_index_builtin_to_runtime_helper_call() {
    let bytecode = common::bytecode_ok(
        r#"
fn main(values: [i32]) -> [i32] {
    set_index(values, 0, 9)
}
"#,
    );
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetIndex),
            ..
        }
    )));
}

#[test]
fn lowers_place_assignments_to_aggregate_writes() {
    let bytecode = common::bytecode_ok(
        r#"
struct Point { var x: i32 }
struct Holder { var inner: Point }

fn main() -> i32 {
    var holder = Holder { inner: Point { x: 1 } };
    holder.inner.x = 7;
    var values = [1, 2];
    values[0] = 5;
    holder.inner.x + values[0]
}
"#,
    );
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::WriteAggregateField { field, .. }
            if bytecode.structures.get(field.structure.index()).and_then(|layout| layout.fields.get(field.slot as usize)).is_some_and(|record| record.name == "x")
    )));
    assert!(
        function.instructions.iter().any(|instruction| matches!(
            instruction,
            BytecodeInstruction::WriteAggregateIndex { .. }
        ))
    );
    assert!(!function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(
                RuntimeHelper::ReflectSetField(_) | RuntimeHelper::ReflectSetIndex
            ),
            ..
        }
    )));
    assert!(function.metadata.effects.writes_aggregate);
    assert!(!function.metadata.effects.calls);
}

#[test]
fn preserves_module_init_function_metadata_in_bytecode() {
    let bytecode = common::bytecode_ok(
        r#"
val boot = 1;

fn main() -> i32 { 1 }
"#,
    );

    assert!(bytecode.module_init.is_some());
}

#[test]
fn does_not_allocate_module_slots_for_const_items() {
    let bytecode = common::bytecode_ok(
        r#"
const BASE: i32 = 1;
const VALUE: i32 = BASE + 2;

fn main() -> i32 { VALUE }
"#,
    );
    let function = bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");

    assert!(bytecode.module_slots.is_empty());
    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::LoadConst { .. }))
    );
    assert!(
        !function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::LoadModule { .. }))
    );
}

#[test]
fn stdlib_lowers_standard_library_calls_to_bytecode_intrinsic_ids() {
    let bytecode = common::bytecode_ok(
        r#"
fn main() -> usize {
    val values = [1, 2];
    values.push(3);
    values.pop();
    values.len()
}
"#,
    );
    let function = bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayPush),
            ..
        }
    )));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayPop),
            ..
        }
    )));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayLen),
            ..
        }
    )));
}

#[test]
fn artifact_loader_rejects_invalid_struct_layouts_slots_and_initializers() {
    let original = common::bytecode_ok(
        "struct P { var x: i32, val fixed: bool } fn main() -> i32 { val p = P { fixed: true, x: 1 }; p.x = 2; p.x }",
    );
    let valid = KbcArtifact::from_program(
        crate::bytecode::BytecodeProgram {
            root: crate::bytecode::ModuleRef::new(0),
            modules: vec![original.clone()],
        },
        Default::default(),
    )
    .unwrap();
    for corruption in 0..7 {
        let mut module = original.clone();
        match corruption {
            0 => module.structures.push(module.structures[0].clone()),
            1 => module.structures[0].fields[0]
                .declaration
                .module
                .path
                .push("foreign".into()),
            2 => module.structures[0].fields[0].mutable = false,
            _ => {
                for instruction in module
                    .functions
                    .iter_mut()
                    .flat_map(|function| &mut function.instructions)
                {
                    match instruction {
                        BytecodeInstruction::MakeStruct {
                            structure, fields, ..
                        } => match corruption {
                            3 => *structure = StructId::new(999),
                            4 => {
                                fields.pop();
                            }
                            5 => fields.swap(0, 1),
                            _ => {}
                        },
                        BytecodeInstruction::ReadAggregateField { field, .. }
                            if corruption == 6 =>
                        {
                            field.slot = u32::MAX
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(
            KbcArtifact::from_program(
                crate::bytecode::BytecodeProgram {
                    root: crate::bytecode::ModuleRef::new(0),
                    modules: vec![module.clone()],
                },
                ArtifactBuildOptions::default(),
            )
            .is_err()
        );
        let mut corrupted = valid.clone();
        corrupted.program.modules[0] = module;
        let bytes = corrupted.to_bytes().unwrap();
        let artifact = KbcArtifact::from_bytes(&bytes).unwrap();
        assert!(
            matches!(
                artifact.validate_for_loader(&ArtifactCompatibility::default()),
                Err(ArtifactValidationError::Bytecode(_))
            ),
            "corruption {corruption}"
        );
    }
}

#[test]
fn executable_struct_fields_require_concrete_resolved_types() {
    use crate::module::abi::{AbiType, BuiltinType, NominalAbiType};
    let module = common::bytecode_ok(
        "struct Box<T> { val value: T } fn main() -> i32 { Box<i32> { value: 42 }.value }",
    );
    let declaration = module.structures[0].declaration.clone();
    for ty in [
        AbiType::Parameter {
            owner: declaration.clone(),
            position: 0,
        },
        AbiType::Struct(NominalAbiType {
            declaration,
            arguments: vec![AbiType::Builtin(BuiltinType::Bool)],
        }),
        AbiType::Builtin(BuiltinType::Bool),
    ] {
        let mut invalid = module.clone();
        invalid.structures[0].fields[0].ty = ty;
        assert!(verify_module(&invalid).is_err());
    }
}

#[test]
fn struct_instances_must_match_public_templates_locally_and_across_modules() {
    use crate::{
        bytecode::{BytecodeProgram, ModuleRef, verify_program},
        module::abi::{AbiType, BuiltinType},
    };
    let owner = common::bytecode_ok(
        "pub struct Box<T> { var values: [T] } fn main() -> i32 { Box<i32> { values: [42] }.values[0] }",
    );
    let mut importer = BytecodeModule {
        identity: ModuleIdentity::single_file("importer.kgr"),
        structures: owner.structures.clone(),
        dependencies: vec![ModuleRef::new(0)],
        ..Default::default()
    };
    let program = |importer| {
        let declaration_owner = BytecodeModule {
            identity: owner.identity.clone(),
            public_items: owner.public_items.clone(),
            ..Default::default()
        };
        BytecodeProgram {
            root: ModuleRef::new(1),
            modules: vec![declaration_owner, importer],
        }
    };
    verify_program(&program(importer.clone())).unwrap();
    for mutation in 0..4 {
        let mut invalid = owner.clone();
        match mutation {
            0 => {
                invalid.structures[0].fields[0].ty =
                    AbiType::Array(Box::new(AbiType::Builtin(BuiltinType::Bool)))
            }
            1 => invalid.structures[0].fields[0].mutable = false,
            2 => {
                invalid.structures[0].fields[0].name = "other".into();
                invalid.structures[0].fields[0]
                    .declaration
                    .path
                    .last_mut()
                    .unwrap()
                    .name = "other".into();
            }
            _ => invalid.structures[0].fields.clear(),
        }
        assert_eq!(
            verify_module(&invalid),
            Err(BytecodeVerificationError::InvalidStructLayout)
        );
        importer.structures = invalid.structures;
        // An imported layout can be internally valid without matching its owner.
        let mut standalone = importer.clone();
        standalone.dependencies.clear();
        verify_module(&standalone).unwrap();
        assert_eq!(
            verify_program(&program(importer.clone())),
            Err(BytecodeVerificationError::InvalidStructLayout)
        );
    }
}

#[test]
fn executable_layouts_reject_noncanonical_declaration_and_member_identities() {
    let module = common::bytecode_ok(
        "pub struct Item { val value: i32 } pub enum Token { Data(i32) } fn main() -> i32 { val token = Token::Data(1); Item { value: 42 }.value }",
    );
    for mutation in 0..6 {
        let mut invalid = module.clone();
        match mutation {
            0 => {
                invalid.structures[0].declaration.path[0].occurrence = 1;
                invalid.structures[0].fields[0].declaration.path[0].occurrence = 1;
            }
            1 => invalid.structures[0].fields[0].declaration.path[1].occurrence = 1,
            2 => {
                let parent = invalid.structures[0].declaration.path[0].clone();
                invalid.structures[0]
                    .declaration
                    .path
                    .insert(0, parent.clone());
                invalid.structures[0].fields[0]
                    .declaration
                    .path
                    .insert(0, parent);
            }
            3 => {
                invalid.enumerations[0].declaration.path[0].occurrence = 1;
                invalid.enumerations[0].variants[0].declaration.path[0].occurrence = 1;
            }
            4 => invalid.enumerations[0].variants[0].declaration.path[1].occurrence = 1,
            _ => {
                let parent = invalid.enumerations[0].declaration.path[0].clone();
                invalid.enumerations[0]
                    .declaration
                    .path
                    .insert(0, parent.clone());
                invalid.enumerations[0].variants[0]
                    .declaration
                    .path
                    .insert(0, parent);
            }
        }
        assert_eq!(
            verify_module(&invalid),
            Err(if mutation < 3 {
                BytecodeVerificationError::InvalidStructLayout
            } else {
                BytecodeVerificationError::InvalidEnumLayout
            })
        );
    }
}
