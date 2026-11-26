//! A cold script Future, then its parked continuation, must own its captured values.
use super::{Fixture, slice};
use crate::engine::builder::KagariEngineBuilder;
use kagari_abi::representation::ValueType;
use kagari_bytecode::instruction::BytecodeInstruction;
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_gc::heap::HeapStats;
use kagari_runtime::{
    error::{RuntimeErrorKind, reason::ErrorReason},
    execution::owned::DriveResult,
    host::{HostObjectId, HostPathAdapter, HostRootHandle, HostSchemaEpoch},
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        completion::{Completion, CompletionStatus},
        declarations::FunctionDecl,
        payload::Opaque,
        storage::NativeStorage,
        typed::NativeContext,
        types::Type,
    },
    session::owned::OwnedExecution,
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_types::{
    arithmetic::ArithmeticError,
    declaration::{
        FieldDef,
        host::{
            HostFieldPolicy, HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
    },
    scalar::{BuiltinType, error::ScalarError},
    ty::Ty,
    visibility::Visibility,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, atomic::Ordering},
};

// Check the decoded factory operands: a scalar computed early or an enclosing
// script object would exercise a different retention contract.
fn assert_direct_capture(f: &Fixture, expected: ValueType, semantic: impl Fn(&Ty) -> bool) {
    let program = f.program.program().bytecode();
    assert!(program.source_package().is_none());
    let module = &program.modules()[program.root().index()];
    let found = module.functions.iter().any(|factory| {
        factory.instructions.iter().any(|op| {
            let BytecodeInstruction::MakeFuture { arguments, .. } = op else {
                return false;
            };
            arguments.iter().any(|slot| {
                let index = slot.index();
                factory.metadata.slots[index] == expected
                    && factory
                        .metadata
                        .semantic
                        .slots
                        .get(&index)
                        .is_some_and(&semantic)
            })
        })
    });
    assert!(
        found,
        "missing direct {expected:?} capture in decoded factory"
    );
}

fn park(f: &mut Fixture, argument: RootedValue) -> (OwnedExecution, Completion<i32>) {
    let starts = f.starts.load(Ordering::SeqCst);
    let future = f.invoke("cold", &[argument]);
    f.runtime.collect_garbage().unwrap();
    assert_eq!(f.starts.load(Ordering::SeqCst), starts);
    let execution = f.start_future(&future);
    drop(future);
    match f.runtime.drive(&execution, slice()).unwrap() {
        DriveResult::Waiting => {}
        other => panic!("expected the script to park at request: {other:?}"),
    }
    assert_eq!(f.starts.load(Ordering::SeqCst), starts + 1);
    assert_eq!(f.inputs.lock().unwrap().last(), Some(&13));
    f.runtime.collect_garbage().unwrap();
    let mut endpoints = f.sent.lock().unwrap();
    assert_eq!(endpoints.len(), 1);
    (execution, endpoints.pop().unwrap())
}

fn finish(f: &mut Fixture, execution: OwnedExecution) -> NativeResult<RootedValue> {
    match f.runtime.drive(&execution, slice()).unwrap() {
        DriveResult::Complete(result) => result,
        other => panic!("expected the resumed invocation to finish: {other:?}"),
    }
}

fn assert_released(f: &mut Fixture, baseline: &HeapStats) {
    f.runtime.collect_garbage().unwrap();
    let after = f.runtime.gc_stats();
    assert_eq!(after.root_registrations, baseline.root_registrations);
    assert_eq!(after.objects, baseline.objects);
    assert!(f.runtime.execution_root().is_none());
}

kagari_runtime::native_data! {
    struct Token { value: i32 }
}

fn install_token(engine: &mut KagariEngineBuilder) {
    let mut module = ModuleBuilder::new("sample", engine.declarations()).unwrap();
    let mut token = module.define_type("Token").unwrap();
    token
        .native_storage(NativeStorage::data::<Token>())
        .unwrap();
    let token = token.finish().unwrap().apply([]).unwrap();
    let make = module
        .define_function(
            FunctionDecl::new("make")
                .parameter("seed", Type::i32())
                .returns(token.clone()),
        )
        .unwrap();
    module
        .bind(
            make,
            |cx: &mut NativeContext<'_>, (seed,): (i32,)| -> NativeResult<Opaque<Token>> {
                cx.create_opaque(Token { value: seed })
            },
        )
        .unwrap();
    let read = module
        .define_function(
            FunctionDecl::new("read")
                .parameter("token", token)
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            read,
            |cx: &mut NativeContext<'_>, (token,): (Opaque<Token>,)| {
                token.read(cx, |token| Ok(token.value))
            },
        )
        .unwrap();
    engine.install(module.finish().unwrap()).unwrap();
}

#[test]
fn managed_native_capture_survives_gc_and_releases_roots_on_trap_and_cancel() {
    let mut f = Fixture::with_source(
        include_str!("../../../../../tests/fixtures/embedding/async/retained-native.kgr"),
        Default::default(),
        install_token,
    );
    assert_direct_capture(&f, ValueType::HeapObject, |ty| {
        matches!(ty, Ty::NativeObject(_))
    });
    f.runtime.collect_garbage().unwrap();
    let baseline = f.runtime.gc_stats();
    let (execution, response) = park(&mut f, 37.into());
    assert_eq!(response.complete(Ok(5)), CompletionStatus::Accepted);
    assert_eq!(finish(&mut f, execution).unwrap().as_i32(), Some(42));
    assert_released(&mut f, &baseline);

    let (execution, response) = park(&mut f, i32::MAX.into());
    assert_eq!(response.complete(Ok(1)), CompletionStatus::Accepted);
    let trap = finish(&mut f, execution).unwrap_err();
    assert_eq!(
        trap.reason(),
        &ErrorReason::Scalar(ScalarError::from(ArithmeticError::Overflow))
    );
    assert_released(&mut f, &baseline);

    let (execution, response) = park(&mut f, 19.into());
    execution.cancel();
    assert_eq!(
        finish(&mut f, execution).unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(f.cancels.load(Ordering::SeqCst), 1);
    assert_eq!(response.complete(Ok(7)), CompletionStatus::Stale);
    assert_released(&mut f, &baseline);
}

const A: HostObjectId = HostObjectId(17);
const B: HostObjectId = HostObjectId(23);

#[derive(Default)]
struct Players {
    scores: HashMap<HostObjectId, i32>,
    reads: Vec<HostObjectId>,
}

fn install_players(engine: &mut KagariEngineBuilder, players: Arc<Mutex<Players>>) {
    let mut module = ModuleBuilder::new("game", engine.declarations()).unwrap();
    let mut player = module.define_type("Player").unwrap();
    player
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadOnly,
        })
        .unwrap();
    player
        .field(
            DefinitionMetadata::checked(
                DefinitionContext::new().unwrap().snapshot(),
                FieldDef {
                    name: "hp".into(),
                    ty: Ty::Builtin(BuiltinType::I32),
                    mutable: false,
                    visibility: Visibility::Public,
                    host_policy: Some(HostFieldPolicy {
                        readable: true,
                        path_access: PathAccess::ReadOnly,
                    }),
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let player = player.finish().unwrap();
    let hp = player
        .definitions()
        .lookup_child(player.id(), DefinitionKind::Field, "hp", 0)
        .unwrap();
    let path = module
        .define_path(
            DefinitionMetadata::checked(
                player.definitions().clone(),
                HostPathDeclaration {
                    root: player.id(),
                    segments: vec![HostPathSegmentDeclaration::Field(hp)],
                    access: PathAccess::ReadOnly,
                    schema_epoch: 0,
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    module
        .bind_path(
            &path,
            HostPathAdapter::new().with_read(move |_, path| {
                let id = path.root.object_id();
                let mut players = players.lock().unwrap();
                players.reads.push(id);
                Ok(ValueView::from(players.scores[&id]))
            }),
        )
        .unwrap();
    engine.install(module.finish().unwrap()).unwrap();
}

fn park_player(f: &mut Fixture, root: HostRootHandle) -> (OwnedExecution, Completion<i32>) {
    let value = f.runtime.host_root_value(root).unwrap();
    park(f, value)
}

#[test]
fn resumed_host_capture_resolves_original_player_or_traps_before_adapter() {
    let players = Arc::new(Mutex::new(Players::default()));
    let mut f = Fixture::with_source(
        include_str!("../../../../../tests/fixtures/embedding/async/retained-player.kgr"),
        Default::default(),
        |engine| install_players(engine, players.clone()),
    );
    assert_direct_capture(&f, ValueType::HostHandle, |ty| matches!(ty, Ty::Host(_)));
    let ty = f.runtime.types().get_by_name("game::Player").unwrap().id;
    let epoch = HostSchemaEpoch::new(0);
    let a = f.runtime.register_host_root(A, ty, epoch).unwrap();
    f.runtime.collect_garbage().unwrap();
    let baseline = f.runtime.gc_stats();
    players.lock().unwrap().scores.insert(A, 11);
    let (execution, response) = park_player(&mut f, a);
    assert!(players.lock().unwrap().reads.is_empty());
    players.lock().unwrap().scores.insert(A, 37);
    assert_eq!(response.complete(Ok(5)), CompletionStatus::Accepted);
    assert_eq!(finish(&mut f, execution).unwrap().as_i32(), Some(42));
    assert_eq!(players.lock().unwrap().reads, [A]);
    assert_released(&mut f, &baseline);

    let (execution, response) = park_player(&mut f, a);
    f.runtime.retire_host_root(a).unwrap();
    players.lock().unwrap().scores.remove(&A);
    let b = f.runtime.register_host_root(B, ty, epoch).unwrap();
    players.lock().unwrap().scores.insert(B, 91);
    assert_eq!(response.complete(Ok(5)), CompletionStatus::Accepted);
    let error = finish(&mut f, execution).unwrap_err();
    assert!(
        matches!(error.reason(), ErrorReason::TypedPathValidation(detail) if detail.contains("host root") && detail.contains("not registered")),
        "{error}"
    );
    assert_eq!(
        players.lock().unwrap().reads,
        [A],
        "stale access reached a player adapter"
    );
    assert_released(&mut f, &baseline);

    let (execution, response) = park_player(&mut f, b);
    assert_eq!(response.complete(Ok(5)), CompletionStatus::Accepted);
    assert_eq!(finish(&mut f, execution).unwrap().as_i32(), Some(96));
    assert_eq!(players.lock().unwrap().reads, [A, B]);
    assert_released(&mut f, &baseline);
}
