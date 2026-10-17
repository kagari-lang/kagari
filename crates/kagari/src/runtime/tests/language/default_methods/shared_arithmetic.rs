//! Shared arithmetic callable supply for primitive and explicit implementations.
use super::editable_artifact;
use crate::{BytecodeArtifact, engine::EngineConfig, test_support::execution};
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
use kagari_common::identity::{DefinitionKind, metadata::DefinitionMetadata};
use kagari_contract::{
    callable::witness::OperationWitness, native_import::callables::NativeCallableOrigin,
};
use kagari_runtime::{Runtime, RuntimeConfig, native::module::NativeModule, value::Value};
use kagari_source::source::SourceFile;
use kagari_types::{
    declaration::module::ModuleDecl, language::Protocol, scalar::BuiltinType, ty::Ty,
};
use kagari_vm::vm::Vm;

struct ArithmeticCase {
    trait_name: &'static str,
    method: &'static str,
    operator: &'static str,
    left: i32,
    right: Option<i32>,
}

impl ArithmeticCase {
    fn source(&self, custom: bool) -> String {
        let Self {
            trait_name,
            method,
            operator,
            left,
            right,
        } = self;
        let (bound, parameters, expression, implementation_parameters, implementation_body) =
            if right.is_some() {
                (
                    format!("{trait_name}<i32, Output = i32>"),
                    "value: T, rhs: i32",
                    format!("value {operator} rhs"),
                    "self, rhs: i32",
                    format!("self.value {operator} rhs"),
                )
            } else {
                (
                    format!("{trait_name}<Output = i32>"),
                    "value: T",
                    format!("{operator}value"),
                    "self",
                    format!("{operator}self.value"),
                )
            };
        let implementation_trait = if right.is_some() {
            format!("{trait_name}<i32>")
        } else {
            trait_name.to_string()
        };
        // A nominal receiver and scalar Output prevent an identity-shaped witness
        // from accidentally satisfying the explicit implementation contract.
        let receiver = if custom {
            format!(
                "struct Number {{ val value: i32 }}
                 impl {implementation_trait} for Number {{
                     type Output = i32;
                     fn {method}({implementation_parameters}) -> i32 {{ {implementation_body} }}
                 }}"
            )
        } else {
            String::new()
        };
        let left = if custom {
            format!("Number {{ value: {left} }}")
        } else {
            left.to_string()
        };
        let arguments = match right {
            Some(right) => format!("{left}, {right}"),
            None => left,
        };
        format!(
            "use std::ops::{trait_name};
             {receiver}
             trait Runner {{
                 fn run<T: {bound}>(self, {parameters}) -> i32 {{ {expression} }}
             }}
             struct Source {{}}
             impl Runner for Source {{}}
             fn main() -> i32 {{
                 val runner: Runner = Source {{}};
                 runner.run({arguments})
             }}"
        )
    }

    fn check(&self, custom: bool) {
        let mut config = EngineConfig::default();
        config.default_runtime.gc.collection_threshold = Some(1);
        let engine = execution::engine(config);
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("shared-arithmetic.kgr", self.source(custom)),
                Default::default(),
            )
            .unwrap_or_else(|error| {
                panic!(
                    "{} (custom={custom}) compilation failed: {error:?}",
                    self.trait_name
                )
            });
        execution::assert_returns_i32(
            &engine,
            artifact,
            &[execution::Route::Source, execution::Route::Artifact],
            42,
        );
    }
}

macro_rules! arithmetic_contract {
    ($name:ident, $trait:literal, $method:literal, $operator:literal, $left:expr, $right:expr) => {
        mod $name {
            use super::ArithmeticCase;

            const CASE: ArithmeticCase = ArithmeticCase {
                trait_name: $trait,
                method: $method,
                operator: $operator,
                left: $left,
                right: $right,
            };

            #[test]
            fn primitive() {
                CASE.check(false);
            }

            #[test]
            fn explicit_receiver() {
                CASE.check(true);
            }
        }
    };
}

arithmetic_contract!(neg, "Neg", "neg", "-", -42, None);
arithmetic_contract!(add, "Add", "add", "+", 20, Some(22));
arithmetic_contract!(sub, "Sub", "sub", "-", 65, Some(23));
arithmetic_contract!(mul, "Mul", "mul", "*", 6, Some(7));
arithmetic_contract!(div, "Div", "div", "/", 126, Some(3));
arithmetic_contract!(rem, "Rem", "rem", "%", 127, Some(85));

#[test]
fn shared_numeric_traps_preserve_effects_and_release_roots() {
    for (protocol, operator, left, right, expected_error) in [
        ("Neg", "-", "-128i8", None, "integer overflow"),
        ("Add", "+", "255u8", Some("1u8"), "integer overflow"),
        ("Div", "/", "42", Some("0"), "integer division by zero"),
        ("Rem", "%", "42", Some("0"), "integer remainder by zero"),
        ("Rem", "%", "-32768i16", Some("-1i16"), "integer overflow"),
    ] {
        let (bound, parameters, expression, arguments, expected_trace) = match right {
            Some(right) => (
                format!("{protocol}<T, Output = T>"),
                "left: T, right: T",
                format!("left {operator} right"),
                format!("operand(self, 1, {left}), operand(self, 2, {right})"),
                123,
            ),
            None => (
                format!("{protocol}<Output = T>"),
                "value: T",
                format!("{operator}value"),
                format!("operand(self, 1, {left})"),
                13,
            ),
        };
        let source = format!(
            r#"
use std::ops::{protocol};
struct State {{ var trace: i32, var target: i32 }}
fn operand<T>(state: State, digit: i32, value: T) -> T {{
    state.trace = state.trace * 10 + digit;
    value
}}
trait Runner {{
    fn run<T: {bound}>(self, state: State, {parameters}) -> T {{
        val pending = Vec::from([state]);
        state.trace = state.trace * 10 + 3;
        val result = {expression};
        pending[0].trace = pending[0].trace * 10 + 4;
        result
    }}
}}
struct Source {{}}
impl Runner for Source {{}}
trait Test {{
    fn run(self);
    fn trace(self) -> i32;
    fn target(self) -> i32;
}}
impl Test for State {{
    fn run(self) {{
        val runner: Runner = Source {{}};
        self.target = runner.run(self, {arguments}) as i32;
    }}
    fn trace(self) -> i32 {{ self.trace }}
    fn target(self) -> i32 {{ self.target }}
}}
pub fn make() -> Test {{ State {{ trace: 0, target: 7 }} }}
"#
        );
        let label = format!("{protocol}({left}, {right:?})");
        let engine = execution::engine(Default::default());
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("shared-arithmetic-trap.kgr", source),
                Default::default(),
            )
            .unwrap_or_else(|error| panic!("{label}: {error:?}"));
        let decoded = BytecodeArtifact::from_bytes(
            &artifact.records().to_bytes(artifact.definitions()).unwrap(),
        )
        .unwrap();
        let (definitions, artifact) = decoded.into_parts();
        let program =
            DefinitionMetadata::checked(definitions, artifact.program, &Default::default())
                .unwrap();
        let mut config = RuntimeConfig::default();
        config.gc.collection_threshold = Some(1);
        let mut runtime = Runtime::new(config);
        NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
        let loaded = runtime
            .load_program("shared-arithmetic-trap", program)
            .unwrap();
        let declaration = loaded
            .bytecode
            .trait_contracts
            .iter()
            .find(|contract| contract.abi.name == "Test")
            .unwrap()
            .declaration;
        let method = |name| ModuleDecl::method_id(declaration, name, loaded.definitions()).unwrap();
        let vm = Vm::new(runtime);
        let baseline = vm.runtime().collect_garbage().unwrap().live_objects;
        let root = vm.execute(&loaded, "make").unwrap().return_value;
        let state = root.value(vm.runtime().gc()).unwrap();
        let error = vm
            .invoke_interface_method(&state, &method("run"), &[])
            .unwrap_err();
        assert!(
            format!("{error:?}").contains(expected_error),
            "{label}: {error:?}"
        );
        // Only the host-retained state survives; adapter/shared-frame roots must
        // unwind even with a temporary collection live across the operation.
        assert_eq!(vm.runtime().gc().active_roots(), 1, "{label}");
        assert!(!vm.runtime().is_quarantined(), "{label}");
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(
            vm.invoke_interface_method(&state, &method("trace"), &[])
                .unwrap(),
            Value::I32(expected_trace),
            "{label}: operands commit once in order, but the post-operation effect does not"
        );
        assert_eq!(
            vm.invoke_interface_method(&state, &method("target"), &[])
                .unwrap(),
            Value::I32(7),
            "{label}: a trapping RHS must not commit its enclosing assignment"
        );
        drop(root);
        assert_eq!(vm.runtime().gc().active_roots(), 0, "{label}");
        assert_eq!(
            vm.runtime().collect_garbage().unwrap().live_objects,
            baseline,
            "{label}: temporary collections and interface receivers are released"
        );
    }
}

#[derive(Clone, Copy, Debug)]
enum OperatorForgery {
    MissingWitness,
    WrongTarget,
    AssociatedOutput,
    Operand,
    Member,
}

#[test]
fn shared_operator_contracts_reject_forgeries_without_source() {
    let engine = execution::engine(Default::default());
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "shared-arithmetic-witness.kgr",
                r#"
use std::ops::{Add, Sub};
trait Runner {
    fn add<T: Add<i32, Output = i32>>(self, left: T, right: i32) -> i32 { left + right }
    fn sub<T: Sub<i32, Output = i32>>(self, left: T, right: i32) -> i32 { left - right }
}
struct Source {}
impl Runner for Source {}
fn main() -> i32 {
    val runner: Runner = Source {};
    runner.add(20, 22) + runner.sub(1, 1)
}
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let decoded =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let (context, artifact) = editable_artifact(decoded);
    let definitions = context.snapshot();
    let subtraction = artifact
        .program
        .modules
        .iter()
        .flat_map(|module| &module.functions)
        .flat_map(|function| &function.instructions)
        .filter_map(|instruction| match instruction {
            BytecodeInstruction::Call {
                callee: CallTarget::InterfaceMethod { contract, .. },
                ..
            } => Some(contract.operations.as_slice()),
            BytecodeInstruction::Call {
                callee: CallTarget::Shared { contract, .. },
                ..
            } => Some(contract.operations.as_slice()),
            _ => None,
        })
        .flatten()
        .find_map(|operation| match operation {
            OperationWitness::Selected(selected)
                if Protocol::from_definition(
                    &selected.requirement.interface.declaration,
                    &definitions,
                ) == Some(Protocol::Sub) =>
            {
                Some(selected.clone())
            }
            _ => None,
        })
        .expect("a concrete subtraction witness with the same physical signature as addition");
    assert_eq!(subtraction.origin, NativeCallableOrigin::ProtocolAdapter);
    for mutation in [
        OperatorForgery::MissingWitness,
        OperatorForgery::WrongTarget,
        OperatorForgery::AssociatedOutput,
        OperatorForgery::Operand,
        OperatorForgery::Member,
    ] {
        let mut program = artifact.program.clone();
        match mutation {
            OperatorForgery::MissingWitness | OperatorForgery::WrongTarget => {
                let operations = program
                    .modules
                    .iter_mut()
                    .flat_map(|module| &mut module.functions)
                    .flat_map(|function| &mut function.instructions)
                    .find_map(|instruction| {
                        let operations = match instruction {
                            BytecodeInstruction::Call {
                                callee: CallTarget::InterfaceMethod { contract, .. },
                                ..
                            } => &mut contract.operations,
                            BytecodeInstruction::Call {
                                callee: CallTarget::Shared { contract, .. },
                                ..
                            } => &mut contract.operations,
                            _ => return None,
                        };
                        operations
                            .iter()
                            .any(|operation| {
                                matches!(operation, OperationWitness::Selected(selected)
                                if Protocol::from_definition(
                                    &selected.requirement.interface.declaration,
                                    &definitions,
                                ) == Some(Protocol::Add))
                            })
                            .then_some(operations)
                    })
                    .expect("a concrete addition witness supplied to the shared default");
                assert_eq!(operations.len(), 1);
                if matches!(mutation, OperatorForgery::MissingWitness) {
                    operations.clear();
                } else {
                    let OperationWitness::Selected(selected) = &mut operations[0] else {
                        unreachable!()
                    };
                    assert_eq!(selected.signature, subtraction.signature);
                    // A real, same-signature Sub adapter cannot stand in for Add.
                    selected.instance = subtraction.instance.clone();
                }
            }
            OperatorForgery::AssociatedOutput
            | OperatorForgery::Operand
            | OperatorForgery::Member => {
                let required = program
                    .modules
                    .iter_mut()
                    .flat_map(|module| &mut module.functions)
                    .filter_map(|function| function.metadata.semantic.protocol_adapter.as_mut())
                    .find(|required| {
                        Protocol::from_definition(&required.interface.declaration, &definitions)
                            == Some(Protocol::Add)
                    })
                    .expect("the callee's independently checked primitive Add adapter");
                match mutation {
                    OperatorForgery::AssociatedOutput => {
                        let output = definitions
                            .lookup_child(
                                required.interface.declaration,
                                DefinitionKind::AssociatedType,
                                "Output",
                                0,
                            )
                            .unwrap();
                        assert_eq!(
                            required.interface.associated_types[&output],
                            Ty::Builtin(BuiltinType::I32)
                        );
                        required
                            .interface
                            .associated_types
                            .insert(output, Ty::Builtin(BuiltinType::Bool));
                    }
                    OperatorForgery::Operand => {
                        required.interface.arguments[0] = Ty::Builtin(BuiltinType::Bool);
                    }
                    OperatorForgery::Member => {
                        required.member = subtraction.requirement.member;
                    }
                    _ => unreachable!(),
                }
            }
        }
        // Reconstruct verified executable data, not source diagnostics or stale
        // fingerprints: each forgery must fail the source-free contract verifier.
        assert!(
            BytecodeArtifact::from_program(program, Default::default(), &context).is_err(),
            "accepted operator forgery: {mutation:?}"
        );
    }
}
