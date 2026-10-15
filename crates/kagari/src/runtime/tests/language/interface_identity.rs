use crate::test_support::execution;

use crate::{BytecodeArtifact, engine::EngineConfig, error::EmbeddingError};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BinaryOp, BytecodeInstruction, CallTarget},
    program::verify_program,
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_contract::types::{InterfaceBridge, PublicItem};
use kagari_runtime::{Runtime, RuntimeConfig, native::module::NativeModule, value::Value};
use kagari_source::source::SourceFile;
use kagari_types::{declaration::module::ModuleDecl, ty::Ty};
use kagari_vm::vm::Vm;

#[test]
fn concrete_identity_is_independent_of_equality_and_interface_views() {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = execution::engine(config);
    let source = r#"
trait Read { fn read(self) -> i32; }
trait Child: Read {}
struct Item { var value: i32 }
struct Other { val value: i32 }
impl Read for Item { fn read(self) -> i32 { self.value } }
impl Child for Item {}
impl Read for Other { fn read(self) -> i32 { self.value } }
fn boxed(value: Item) -> Child { value }
fn same_objects<T>(a: Vec<T>, b: Vec<T>) -> bool { a === b }
fn main() -> i32 {
    val item = Item { value: 42 };
    val child: Child = boxed(item);
    val parent: Read = child;
    val again: Read = item;
    val separate = Item { value: 42 };
    val other = Other { value: 42 };
    val joined: Read = if true { child } else { parent };
    item.value = 42;
    if !(item === item && item !== separate && item !== other) { return 1; }
    if item === other || again.read() != joined.read() { return 2; }
    val objects = Vec::from([item]);
    if !same_objects(objects, objects) { return 3; }
    parent.read()
}
"#;
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("interface-identity.kgr", source),
            Default::default(),
        )
        .unwrap();
    execution::assert_returns_i32(
        &engine,
        artifact,
        &[
            execution::Route::Source,
            execution::Route::Artifact,
            execution::Route::ArtifactJit,
        ],
        42,
    );
}

#[test]
fn identity_operand_traps_preserve_effect_order_and_release_interface_roots() {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = execution::engine(config);
    let source = r#"
trait Read { fn read(self) -> i32; }
trait Child: Read {}
struct State { var trace: i32 }
impl Read for State { fn read(self) -> i32 { self.trace } }
impl Child for State {}
trait Test {
    fn compare(self, left: i32, right: i32, negated: bool) -> bool;
    fn alias(self, kind: i32) -> bool;
    fn trace(self) -> i32;
}
impl Test for State {
    fn compare(self, left: i32, right: i32, negated: bool) -> bool { compare(self, left, right, negated) }
    fn alias(self, kind: i32) -> bool { alias(self, kind) }
    fn trace(self) -> i32 { self.trace }
}
pub fn make() -> Test { State { trace: 0 } }
fn operand(state: State, digit: i32, kind: i32) -> State {
    state.trace = state.trace * 10 + digit;
    val child: Child = state;
    val parent: Read = child;
    parent.read();
    if kind != 0 { val zero = 0; 1 / zero; }
    state
}
fn compare(state: State, left: i32, right: i32, negated: bool) -> bool {
    val result = if negated {
        operand(state, 1, left) !== operand(state, 2, right)
    } else {
        operand(state, 1, left) === operand(state, 2, right)
    };
    state.trace = state.trace * 10 + 3;
    result
}
fn alias(state: State, kind: i32) -> bool {
    val child = operand(state, 1, kind);
    child === state
}
"#;
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("interface-identity-traps.kgr", source),
            Default::default(),
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let (definitions, artifact) = artifact.into_parts();
    let program =
        DefinitionMetadata::checked(definitions, artifact.program, &Default::default()).unwrap();
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = Runtime::new(config);
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    let loaded = runtime.load_program("interface-identity", program).unwrap();
    let declaration = loaded
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Test")
        .unwrap()
        .declaration;
    let method = |name| ModuleDecl::method_id(declaration, name, loaded.definitions()).unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    let baseline = vm.runtime().collect_garbage().unwrap().live_objects;
    for kind in 1..=1 {
        for (entry, left, right, negated, expected_trace) in [
            ("compare", kind, 0, false, 1),
            ("compare", 0, kind, false, 12),
            ("compare", kind, kind, true, 1),
            ("alias", kind, kind, false, 1),
        ] {
            let root = vm.execute(&loaded, "make").unwrap().return_value;
            let state = root.value(vm.runtime().gc()).unwrap();
            let arguments = if entry == "alias" {
                vec![Value::I32(kind)]
            } else {
                vec![Value::I32(left), Value::I32(right), Value::Bool(negated)]
            };
            let error = vm
                .invoke_interface_method(&state, &method(entry), &arguments)
                .unwrap_err();
            assert!(
                format!("{error:?}").contains("division by zero"),
                "{error:?}"
            );
            assert_eq!(vm.runtime().gc().active_roots(), 1);
            assert!(!vm.runtime().is_quarantined());
            assert_eq!(
                vm.invoke_interface_method(&state, &method("trace"), &[])
                    .unwrap(),
                Value::I32(expected_trace)
            );
            drop(root);
            assert_eq!(vm.runtime().gc().active_roots(), 0);
            assert_eq!(
                vm.runtime().collect_garbage().unwrap().live_objects,
                baseline,
                "{entry}, kind={kind}, left={left}, right={right}"
            );
        }
    }
}

#[test]
fn values_interfaces_and_unknown_generic_categories_reject_identity_operators() {
    let engine = execution::engine(EngineConfig::default());
    for source in [
        "fn main() -> bool { 1 === 1 }",
        "fn main() -> bool { \"x\" !== \"x\" }",
        "fn main() -> bool { (1, 2) === (1, 2) }",
        "enum Choice { One } fn main() -> bool { Choice::One === Choice::One }",
        "trait A {} trait B {} fn compare(a: A, b: B) -> bool { a === b }",
        "trait A {} fn compare(a: A, b: A) -> bool { a !== b }",
        "trait A {} struct Item {} impl A for Item {} fn compare(a: A, b: Item) -> bool { a === b }",
        "trait A {} struct Item {} impl A for Item {} fn compare(a: Item, b: A) -> bool { a === b }",
        "fn compare<T>(a: T, b: T) -> bool { a === b }",
        "fn compare(a: fn() -> i32, b: fn() -> i32) -> bool { a === b }",
    ] {
        let Err(EmbeddingError::Diagnostics { diagnostics }) = engine.compile_to_artifact(
            SourceFile::new("invalid-identity.kgr", source),
            Default::default(),
        ) else {
            panic!("identity operands must fail source checking: {source}");
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "KG_TYPE_BINARY_OPERAND_TYPE_MISMATCH"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn portable_comparisons_cannot_bypass_static_object_or_protocol_rules() {
    let engine = execution::engine(EngineConfig::default());
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "comparison-proof.kgr",
                r#"
trait Read { fn read(self) -> i32; }
struct Item { val value: i32 }
impl Read for Item { fn read(self) -> i32 { self.value } }
impl PartialEq for Item { fn eq(self, other: Self) -> bool { false } }
fn main() -> bool { val a: Read = Item { value: 1 }; a.read() == a.read() }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let decoded =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap()
            .import_into(&context, &Default::default())
            .unwrap()
            .into_records();
    for (identity, concrete) in [(true, false), (false, false), (false, true)] {
        let mut program = decoded.program.clone();
        let function = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .find(|function| function.name == "main")
            .unwrap();
        let (boxed, object) = function
            .instructions
            .iter()
            .find_map(|instruction| match instruction {
                BytecodeInstruction::MakeInterface { dst, value, .. } => Some((*dst, *value)),
                _ => None,
            })
            .unwrap();
        let operand = if concrete { object } else { boxed };
        let instruction = function
            .instructions
            .iter_mut()
            .find(|instruction| {
                matches!(
                    instruction,
                    BytecodeInstruction::Binary {
                        op: BinaryOp::Eq,
                        ..
                    }
                )
            })
            .unwrap();
        let BytecodeInstruction::Binary { op, lhs, rhs, .. } = instruction else {
            unreachable!()
        };
        *op = if identity {
            BinaryOp::IdentityEq
        } else {
            BinaryOp::Eq
        };
        *lhs = operand;
        *rhs = operand;
        assert!(
            KbcArtifact::from_program(program, Default::default(), &context).is_err(),
            "identity={identity}, concrete={concrete}"
        );
    }
}

#[test]
fn portable_unused_signatures_require_dynamic_compatibility() {
    let engine = execution::engine(EngineConfig::default());
    let source = "pub trait Read { fn read(self) -> i32; } pub fn unused(value: Read) -> i32 { 42 } fn main() -> i32 { 42 }";
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("unused-interface.kgr", source),
            Default::default(),
        )
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let mut records = artifact
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    // Populate all implicit references before mutating the declared trait surface.
    records = KbcArtifact::from_program(records.program, Default::default(), &context).unwrap();
    let contract = records
        .program
        .modules
        .iter_mut()
        .flat_map(|module| &mut module.public_items)
        .find_map(|item| match item {
            PublicItem::Trait(contract) if contract.name == "Read" => Some(contract),
            _ => None,
        })
        .unwrap();
    let Ty::SelfType(owner) = contract.methods[0].params[0].ty else {
        panic!("trait receiver")
    };
    contract.methods[0].return_type = Ty::SelfType(owner);
    assert!(
        KbcArtifact::from_program(records.program.clone(), Default::default(), &context).is_err()
    );
    let bytes = records.to_bytes(&context.snapshot()).unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes).unwrap();
    assert!(verify_program(&decoded.records().program, decoded.definitions()).is_err());
    assert!(matches!(
        engine.compile_to_artifact(
            SourceFile::new(
                "invalid-unused.kgr",
                source.replace("fn read(self) -> i32", "fn read(self) -> Self")
            ),
            Default::default()
        ),
        Err(EmbeddingError::Diagnostics { .. })
    ));
}

#[test]
fn intrinsic_interface_bridges_retain_checked_protocol_targets() {
    let engine = execution::engine(EngineConfig::default());
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "intrinsic-bridge.kgr",
                r#"
use std::fmt::Debug;
fn forged(value: i32) -> String { "forged" }
fn main() -> String { val view: Debug = 42; view.debug() }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let mut program = artifact
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records()
        .program;
    let module = program.modules.iter_mut().find(|module| module.public_items.iter().any(|item| matches!(item, PublicItem::InterfaceTable(table) if table.bridge == Some(InterfaceBridge::Intrinsic)))).unwrap();
    let forged = module
        .functions
        .iter()
        .find(|function| function.name == "forged")
        .unwrap()
        .id;
    let bridge = module
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::InterfaceTable(table)
                if table.bridge == Some(InterfaceBridge::Intrinsic) =>
            {
                Some(table.declaration)
            }
            _ => None,
        })
        .unwrap();
    let definitions = context.snapshot();
    let function = module
        .functions
        .iter_mut()
        .find(|function| {
            function.identity.as_ref().is_some_and(|identity| {
                definitions.parent(identity.declaration) == Ok(Some(bridge))
            })
        })
        .unwrap();
    let instruction = function
        .instructions
        .iter_mut()
        .find(|instruction| matches!(instruction, BytecodeInstruction::Call { .. }))
        .unwrap();
    let BytecodeInstruction::Call { callee, .. } = instruction else {
        unreachable!()
    };
    *callee = CallTarget::Function(forged);
    assert!(KbcArtifact::from_program(program, Default::default(), &context).is_err());
}
