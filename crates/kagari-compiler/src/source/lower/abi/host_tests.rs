use super::collect_module_abi;
use crate::tests::common::compilation;
use crate::tests::standard::standard_modules;
use crate::{
    bytecode::lower_program_to_bytecode,
    source::{
        elaborate::{
            elaborate,
            model::{CheckedExprKind, CheckedPlaceKind},
        },
        lower::{MirLoweringError, module_contract},
        program::{SourceProgramError, lower_program_to_mir},
    },
};
use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, ArtifactCompatibility, KbcArtifact},
    program::verified::VerifiedBytecodeProgram,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
    metadata::DefinitionMetadata, table::DefinitionId,
};
use kagari_contract::types::{PublicItem, verify};
use kagari_hir::{analysis::AnalysisDatabase, types::TypeId};
use kagari_mir::{
    codec::{decode_program, encode_program},
    function::MirFunction,
    instruction::{CallTarget, Instruction, Terminator},
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
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::sync::Arc;

#[test]
fn registered_host_type_does_not_authorize_a_new_native_entry() {
    let cancel = Default::default();
    let context = DefinitionContext::new().unwrap();
    let mut provider = ModuleDecl::new(ModuleIdentity {
        package: PackageId("game".into()),
        path: vec![],
    });
    provider.types.push(TypeDef {
        name: "Player".into(),
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
    let definitions = provider.collect_definitions(&context).unwrap();
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(vec![Arc::new(
        DefinitionMetadata::checked(definitions, provider, &cancel).unwrap(),
    )]);
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "bounce.kgr",
            "use game::Player; native fn bounce(p: Player) -> Player;".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = database.snapshot(sources.snapshot(), &cancel).unwrap();
    let checked = snapshot.check_program(root, &cancel).unwrap();
    let input = compilation(&checked);
    let root = input.module(input.root()).unwrap();
    let Err(MirLoweringError::MissingHostPassingPolicy(entry)) = module_contract(root, &cancel)
    else {
        panic!("policy-free contract extraction must fail")
    };
    assert_eq!(&entry.module, root.source().module_identity());
    assert_eq!(entry.path.last().unwrap().name, "bounce");
    let Err(SourceProgramError::Lowering {
        error: MirLoweringError::MissingHostPassingPolicy(path),
        ..
    }) = lower_program_to_mir(&input, &Default::default())
    else {
        panic!("MIR lowering must reject the same Native entry")
    };
    assert_eq!(path, entry);
}

// This fixture has one `if stop` and no loops. Walk the chosen branch rather
// than treating basic-block storage order as execution order.
fn relay_instructions(function: &MirFunction, stop: bool) -> Vec<&Instruction> {
    let mut instructions = Vec::new();
    let mut next = function.entry;
    for _ in &function.blocks {
        let block = &function.blocks[next.index()];
        instructions.extend(&block.instructions);
        next = match block.terminator.as_ref().unwrap() {
            Terminator::Jump(target) => *target,
            Terminator::Branch {
                then_block,
                else_block,
                ..
            } => {
                if stop {
                    *then_block
                } else {
                    *else_block
                }
            }
            Terminator::Return(_) => return instructions,
            other => panic!("unexpected relay terminator: {other:?}"),
        };
    }
    panic!("relay did not return")
}

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
    let hp = context
        .snapshot()
        .lookup_child(player, DefinitionKind::Field, "hp", 0)
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
    let mut baseline = module.types[0].fields[1].clone();
    baseline.name = "baseline".into();
    baseline.visibility = Visibility::Public;
    module.types[0].fields.push(baseline);
    module.collect_definitions(&context).unwrap();
    let baseline = context
        .snapshot()
        .lookup_child(player, DefinitionKind::Field, "baseline", 0)
        .unwrap();
    // Only the complete path has an adapter: reading an intermediate view is not equivalent.
    module.paths.push(HostPathDeclaration {
        root: player,
        segments: vec![
            HostPathSegmentDeclaration::Field(baseline),
            HostPathSegmentDeclaration::Field(hp),
        ],
        access: PathAccess::ReadWrite,
        schema_epoch: 17,
    });
    let metadata = Arc::new(
        DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap(),
    );
    let mut providers = standard_modules();
    providers.push(metadata.clone());
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(providers);
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "external-abi.kgr",
            include_str!("../../../../tests/fixtures/registered-field-path.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let input = compilation(&checked);
    let provider = input
        .modules()
        .iter()
        .find(|module| module.source().module_identity() == &metadata.records().identity)
        .unwrap();
    let definitions = provider.environment().definitions();
    let abi = collect_module_abi(provider).unwrap();
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
        definitions.resolve(parent).unwrap().to_path(),
        metadata.definitions().resolve(player).unwrap().to_path()
    );
    verify::validate(
        &abi.public_items,
        &metadata.records().identity,
        &Default::default(),
        definitions,
    )
    .unwrap();
    // This exercises the previously failing recursive external dependency through
    // both portable products and rejects schema copies that disagree with providers.
    let mir = lower_program_to_mir(&input, &Default::default()).unwrap();
    let registered = metadata
        .import_into(&input.definition_context(), &Default::default())
        .unwrap();
    let expected = &registered.records().paths[0];
    let expanded = |owner: DefinitionId, fields: &[(DefinitionId, TypeId)]| {
        assert_eq!(owner, expected.root);
        assert_eq!(
            fields
                .iter()
                .map(|(field, _)| HostPathSegmentDeclaration::Field(*field))
                .collect::<Vec<_>>(),
            expected.segments
        );
        assert_eq!(fields[0].1, TypeId::Host(owner));
        assert_eq!(fields[1].1, TypeId::Builtin(BuiltinType::I32));
    };
    let mut expanded_paths = 0;
    for module in input.modules() {
        for input in module.bodies() {
            let body = elaborate(input, &Default::default()).unwrap();

            for value in &body.operations {
                if let CheckedExprKind::HostRead(path) = &value.kind {
                    assert_eq!(
                        &module.source().text()[value.span.start..value.span.end],
                        "players[index()].baseline.hp"
                    );
                    expanded(path.owner, &path.fields);
                    expanded_paths += 1;
                }
            }
            for (id, place) in &body.places {
                if let CheckedPlaceKind::Host(path) = &place.kind {
                    let span = input.place_span(*id).unwrap();
                    assert_eq!(
                        &module.source().text()[span.start..span.end],
                        "players[index()].baseline.hp"
                    );
                    expanded(path.owner, &path.fields);
                    expanded_paths += 1;
                }
            }
        }
    }
    assert_eq!(expanded_paths, 2);
    let caller = mir
        .modules()
        .iter()
        .find(|m| &m.identity == mir.root())
        .unwrap();
    let relay = caller.functions.iter().find(|f| f.name == "relay").unwrap();
    let fingerprint = expected
        .contract(&caller.external_types, mir.definitions())
        .unwrap()
        .fingerprint(mir.definitions())
        .unwrap();
    let steps = |stop| {
        let mut steps = Vec::new();
        let mut captured_array = None;
        let mut destination = None;
        for instruction in relay_instructions(relay, stop) {
            if let Some(path) = instruction.path_reference() {
                assert_eq!(path.declaration.as_ref(), Some(expected));
                assert_eq!(path.contract_fingerprint, fingerprint);
            }
            match instruction {
                Instruction::LoadLocal { dst, local }
                    if *local == relay.params[0].local && captured_array.is_none() =>
                {
                    captured_array = Some(*dst);
                    steps.push("capture players");
                }
                Instruction::Call {
                    callee: CallTarget::Function(id),
                    ..
                } => steps.push(caller.functions[id.index()].name.as_str()),
                Instruction::ReadAggregateIndex { dst, base, .. } => {
                    assert_eq!(Some(*base), captured_array);
                    destination = Some(*dst);
                    steps.push("index read");
                }
                Instruction::Call {
                    callee: CallTarget::Native(import),
                    ..
                } => {
                    assert_eq!(
                        mir.definitions()
                            .resolve(import.instance.declaration)
                            .unwrap()
                            .last()
                            .unwrap()
                            .name,
                        "index"
                    );
                    steps.push("index read");
                }
                Instruction::ModifyPath { root_or_view, .. } => {
                    assert_eq!(Some(*root_or_view), destination);
                    steps.push("modify");
                }
                Instruction::ReadPath { .. } => steps.push("read"),
                _ => (),
            }
        }
        steps
    };
    assert_eq!(steps(true), ["capture players", "index"]);
    assert_eq!(
        steps(false),
        [
            "capture players",
            "index",
            "delta",
            "index read",
            "modify",
            "index",
            "index read",
            "read"
        ]
    );

    let wire = encode_program(&mir, &Default::default()).unwrap();
    let restored = decode_program(&wire, &Default::default()).unwrap();
    assert_eq!(
        wire,
        encode_program(&restored, &Default::default()).unwrap()
    );
    let bytecode = lower_program_to_bytecode(&restored, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let bytecode = bytecode
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    VerifiedBytecodeProgram::new(bytecode.clone(), &context, &Default::default()).unwrap();
    let artifact = KbcArtifact::from_program(
        bytecode.clone(),
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
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
        .validate_for_loader(
            &ArtifactCompatibility::default(),
            &context,
            &Default::default(),
        )
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
    assert!(VerifiedBytecodeProgram::new(forged_bytecode, &context, &Default::default()).is_err());

    // A valid adapter for the same leaf on a different chain cannot implement this operation.
    let mut wrong_chain = metadata.records().clone();
    wrong_chain.paths[0].segments.remove(0);
    let mut providers = standard_modules();
    providers.push(Arc::new(
        DefinitionMetadata::checked(context.snapshot(), wrong_chain, &Default::default()).unwrap(),
    ));
    database.set_native_modules(providers);
    let snapshot = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    assert!(matches!(
        lower_program_to_mir(&compilation(&checked), &Default::default()),
        Err(SourceProgramError::Lowering {
            error: MirLoweringError::MissingBinding("no registered path for checked Host fields"),
            ..
        })
    ));
}
