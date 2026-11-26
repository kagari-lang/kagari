use crate::{
    bytecode::lower_program_to_bytecode,
    source::program::lower_program_to_mir,
    tests::{common, standard::standard_modules},
};
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
    metadata::DefinitionMetadata, table::DefinitionId,
};
use kagari_mir::codec::{decode_program, encode_program};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, Param, TypeDef, TypeDefKind,
        host::{
            HostCallPolicy, HostFunctionEffects, HostPassingStyle, HostReflectionPolicy,
            HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::{ImplDecl, ModuleDecl},
        path::{
            HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration,
            HostVirtualSegmentDeclaration, NativePathOperation,
            NativePathOperationKind::{self, Add, Read, Set},
        },
    },
    scalar::BuiltinType,
    ty::Ty,
};
use std::sync::Arc;

fn entry(
    id: DefinitionId,
    name: &str,
    root: DefinitionId,
    receiver: &str,
    kind: NativePathOperationKind,
) -> FnDecl {
    let write = kind != NativePathOperationKind::Read;
    let mut params = vec![
        Param {
            name: receiver.into(),
            mutable: false,
            ty: Ty::Host(root),
        },
        Param {
            name: "key".into(),
            mutable: false,
            ty: Ty::Builtin(BuiltinType::U64),
        },
    ];
    if write {
        params.push(Param {
            name: "value".into(),
            mutable: false,
            ty: Ty::Builtin(BuiltinType::I32),
        });
    }
    FnDecl {
        name: name.into(),
        implementation: CallableImplementation::Native(id),
        generic_params: vec![],
        bounds: vec![],
        method_policy: Default::default(),
        host_policy: Some(HostCallPolicy {
            passing: vec![HostPassingStyle::Owned; params.len()],
            effects: HostFunctionEffects {
                may_mutate_host_state: write,
                may_trap: true,
                ..Default::default()
            },
        }),
        params,
        return_type: Ty::Builtin(if kind == NativePathOperationKind::Set {
            BuiltinType::Unit
        } else {
            BuiltinType::I32
        }),
    }
}

#[test]
fn native_path_calls_require_only_the_selected_path_and_its_schema_closure() {
    let context = DefinitionContext::new().unwrap();
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("example".into()),
        path: vec!["roots".into()],
    });
    for name in ["Stats", "Spare", "Player"] {
        module.types.push(TypeDef {
            name: name.into(),
            kind: TypeDefKind::Host(HostTypePolicy {
                ownership: HostTypeOwnership::HostRoot,
                reflection: HostReflectionPolicy::Metadata,
                path_access: PathAccess::ReadWrite,
            }),
            fields: vec![],
            variants: vec![],
            generic_params: vec![],
            bounds: vec![],
        });
    }
    let defs = module.collect_definitions(&context).unwrap();
    let owner = defs.lookup_root(&module.identity).unwrap();
    let player = module
        .definition(DefinitionKind::Struct, "Player", &defs)
        .unwrap();
    let stats = module
        .definition(DefinitionKind::Struct, "Stats", &defs)
        .unwrap();
    let spare = module
        .definition(DefinitionKind::Struct, "Spare", &defs)
        .unwrap();
    let read = context
        .intern_child(owner, DefinitionKind::Function, "score", 0)
        .unwrap();
    let set = context
        .intern_child(owner, DefinitionKind::Function, "reset", 0)
        .unwrap();
    let imp = context
        .intern_child(owner, DefinitionKind::Impl, "", 0)
        .unwrap();
    let add = context
        .intern_child(imp, DefinitionKind::Method, "score", 0)
        .unwrap();
    let indexed = |name: &str, collection| HostPathDeclaration {
        root: player,
        segments: vec![
            HostPathSegmentDeclaration::Virtual(HostVirtualSegmentDeclaration {
                name: name.into(),
                result: Ty::Host(collection),
                access: PathAccess::ReadWrite,
            }),
            HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                slot: 0,
                collection: Ty::Host(collection),
                index: Ty::Builtin(BuiltinType::U64),
                result: Ty::Builtin(BuiltinType::I32),
                access: PathAccess::ReadWrite,
            }),
        ],
        access: PathAccess::ReadWrite,
        schema_epoch: 7,
    };
    let path = indexed("stats", stats);
    let other = indexed("other", spare);
    module.paths = vec![other.clone(), path.clone()];

    module.functions = vec![
        entry(read, "score", player, "player", Read),
        entry(set, "reset", player, "player", Set),
    ];
    module.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: None,
        for_type: Ty::Host(player),
        methods: vec![entry(add, "score", player, "self", Add)],
    });
    for (id, kind, path) in [(read, Read, &path), (add, Add, &path), (set, Set, &other)] {
        module.path_operations.insert(
            id,
            NativePathOperation {
                kind,
                path: path.clone(),
            },
        );
    }
    let expected = module.path_operations.clone();
    let owner = module.identity.clone();
    let mut providers = standard_modules();
    providers.push(Arc::new(
        DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap(),
    ));
    let checked = common::program_with_modules(
        include_str!("../../../../tests/fixtures/native-path-operation.kgr"),
        providers,
    );
    let mir = lower_program_to_mir(&common::compilation(&checked), &Default::default()).unwrap();
    // Bytecode selection must need no HIR or registration lookup after MIR was exported.
    let bytes = encode_program(&mir, &Default::default()).unwrap();
    let restored = decode_program(&bytes, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&restored, &Default::default()).unwrap();
    let program = common::adopt_bytecode(&context, bytecode.into_unverified());
    let provider = program
        .modules
        .iter()
        .find(|module| module.identity == owner)
        .unwrap();
    for (id, operation) in expected {
        let decl = provider
            .native_declarations
            .iter()
            .find(|d| d.declaration == id)
            .unwrap();
        assert_eq!(decl.path_operation.as_ref(), Some(&operation));
    }
    let caller = &program.modules[program.root.index()];
    assert_eq!(caller.path_declarations, [path]);
    assert_eq!(
        caller
            .external_types
            .iter()
            .map(|ty| ty.declaration)
            .collect::<Vec<_>>(),
        [player, stats]
    );
    assert!(
        caller.paths.is_empty(),
        "ordinary calls do not use path opcodes"
    );
    let relay = caller.functions.iter().find(|f| f.name == "relay").unwrap();
    let calls: Vec<_> = relay
        .instructions
        .iter()
        .filter_map(|instruction| match instruction {
            BytecodeInstruction::Call {
                callee: CallTarget::Native(index),
                dst: Some(dst),
                args,
            } => Some((&caller.native_imports[index.index()], *dst, args.as_slice())),
            _ => None,
        })
        .collect();
    let [
        (read_call, read_result, [read_root, read_key]),
        (add_call, _, [add_root, add_key, rhs]),
    ] = calls.as_slice()
    else {
        panic!("expected the nested read and add calls: {calls:?}");
    };
    assert_eq!(
        [
            read_call.instance.declaration,
            add_call.instance.declaration
        ],
        [read, add]
    );
    for slot in [read_root, add_root] {
        assert_eq!(
            relay.metadata.semantic.slots.get(&slot.index()),
            Some(&Ty::Host(player))
        );
    }
    for slot in [read_key, add_key] {
        assert_eq!(
            relay.metadata.semantic.slots.get(&slot.index()),
            Some(&Ty::Builtin(BuiltinType::U64))
        );
    }
    assert_eq!(read_result, rhs);
    assert_eq!(
        add_call.signature.params,
        [
            Ty::Host(player),
            Ty::Builtin(BuiltinType::U64),
            Ty::Builtin(BuiltinType::I32)
        ]
    );
}
