use crate::{
    bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir, tests::common,
};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, CallTarget},
    program::verify_program,
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_contract::{
    callable::witness::OperationWitness,
    native_import::callables::NativeCallableOrigin,
    types::{PublicItem, proofs::ProofCatalog},
};
use kagari_types::{
    integer::IntegerOp,
    language::{Protocol, adapter::adapter_contract},
    scalar::BuiltinType,
    ty::Ty,
};

#[test]
fn shared_associated_output_preserves_primitive_and_distinct_custom_results() {
    let mut failures = vec![];
    for (name, source) in [
        (
            "primitive Neg",
            r#"use std::ops::Neg;
            trait Runner { fn run<T: Neg>(self, value: T) -> T::Output { -value } }
            impl Runner for i32 {}
            fn main() -> i8 { val runner: Runner = 0; runner.run(-42i8) }"#,
        ),
        (
            "bound and unbound primitive Neg in one program",
            r#"use std::ops::Neg;
            trait Runner {
                fn inferred<T: Neg>(self, value: T) -> T::Output { -value }
                fn fixed<T: Neg<Output = T>>(self, value: T) -> T { -value }
            }
            impl Runner for i32 {}
            fn main() -> i8 {
                val runner: Runner = 0;
                runner.inferred(-21i8) + runner.fixed(-21i8)
            }"#,
        ),
        (
            "custom Neg",
            r#"use std::ops::Neg;
            struct Number { val value: i32 }
            impl Neg for Number {
                type Output = i32;
                fn neg(self) -> i32 { -self.value }
            }
            trait Runner { fn run<T: Neg>(self, value: T) -> T::Output { -value } }
            impl Runner for i32 {}
            fn main() -> i32 {
                val runner: Runner = 0;
                runner.run(Number { value: -42 })
            }"#,
        ),
        (
            "ordinary associated trait",
            r#"trait Project { type Output; fn project(self) -> Self::Output; }
            struct Number { val value: i32 }
            impl Project for Number {
                type Output = i32;
                fn project(self) -> i32 { self.value }
            }
            trait Runner {
                fn run<T: Project>(self, value: T) -> T::Output { value.project() }
            }
            impl Runner for i32 {}
            fn main() -> i32 {
                val runner: Runner = 0;
                runner.run(Number { value: 42 })
            }"#,
        ),
    ] {
        let checked = common::program_ok(source);
        match lower_program_to_mir(&checked, &Default::default()) {
            Ok(mir) => {
                let context = DefinitionContext::new().unwrap();
                let program =
                    common::adopt_bytecode(&context, lower_program_to_bytecode(&mir).unwrap());
                let artifact =
                    KbcArtifact::from_program(program, Default::default(), &context).unwrap();
                let decoded =
                    KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
                        .map(|metadata| common::adopt(&context, metadata))
                        .unwrap();
                decoded
                    .validate_for_loader(&Default::default(), &context)
                    .unwrap();
            }
            Err(error) => failures.push(format!("{name}: {error:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn shared_arithmetic_preserves_source_width_and_checks_callable_evidence() {
    let context = DefinitionContext::new().unwrap();
    for (protocol, operator, operation) in [
        (Protocol::Neg, "-", IntegerOp::CheckedSub),
        (Protocol::Add, "+", IntegerOp::CheckedAdd),
        (Protocol::Sub, "-", IntegerOp::CheckedSub),
        (Protocol::Mul, "*", IntegerOp::CheckedMul),
        (Protocol::Div, "/", IntegerOp::CheckedDiv),
        (Protocol::Rem, "%", IntegerOp::CheckedRem),
    ] {
        let (bound, parameters, expression, arguments) = if protocol == Protocol::Neg {
            ("Neg<Output = T>".into(), "a: T", "-a".into(), "6i8")
        } else {
            (
                format!("{}<T, Output = T>", protocol.name()),
                "a: T, b: T",
                format!("a {operator} b"),
                "6i8, 3i8",
            )
        };

        let inferred_bound = if protocol == Protocol::Neg {
            "Neg".into()
        } else {
            format!("{}<T>", protocol.name())
        };
        let source = format!(
            "use std::ops::{};
             trait Runner {{
                 fn run<T: {bound}>(self, {parameters}) -> T {{ {expression} }}
                 fn inferred<T: {inferred_bound}>(self, {parameters}) -> T::Output {{ {expression} }}
             }}
             impl Runner for i32 {{}}
             fn main() -> i8 {{
                 val runner: Runner = 0;
                 runner.run({arguments}) + runner.inferred({arguments})
             }}",
            protocol.name(),
        );
        let program = common::adopt_bytecode(&context, common::bytecode_ok(&source));
        let function = program.modules[program.root.index()]
            .functions
            .iter()
            .find(|function| {
                function
                    .metadata
                    .semantic
                    .protocol_adapter
                    .as_ref()
                    .is_some_and(|required| !required.interface.associated_types.is_empty())
            })
            .expect("primitive arithmetic adapter");
        assert!(function.instructions.iter().any(|instruction| matches!(
            instruction,
            BytecodeInstruction::Numeric { operation: numeric, .. }
                if numeric.op == operation
                    && numeric.input == BuiltinType::I8
                    && numeric.rhs == Some(BuiltinType::I8)
        )));
        assert!(function.instructions.iter().all(|instruction| !matches!(
            instruction,
            BytecodeInstruction::Binary { .. } | BytecodeInstruction::Unary { .. }
        )));

        let definitions = context.snapshot();
        let required = function
            .metadata
            .semantic
            .protocol_adapter
            .as_ref()
            .unwrap();
        let contract = program
            .modules
            .iter()
            .filter(|module| module.identity == protocol.module())
            .flat_map(|module| &module.public_items)
            .find_map(|item| match item {
                PublicItem::Trait(contract) if contract.name == protocol.name() => Some(contract),
                _ => None,
            })
            .unwrap();
        let proof = ProofCatalog::new(
            vec![],
            [],
            [(required.interface.declaration, contract)],
            [],
            &definitions,
            &Default::default(),
        )
        .unwrap();
        let expected = adapter_contract(required, &definitions).unwrap().1;
        assert_eq!(
            proof
                .implicit_callable_signature(required, &[], &Default::default())
                .unwrap(),
            Some(expected.clone()),
        );
        let mut inferred_output = required.clone();
        inferred_output.interface.associated_types.clear();
        assert_eq!(
            proof
                .implicit_callable_signature(&inferred_output, &[], &Default::default())
                .unwrap(),
            Some(expected),
        );
        for mutation in 0..8 {
            let mut forged = required.clone();
            match mutation {
                0 => {
                    let output = forged
                        .interface
                        .associated_types
                        .values_mut()
                        .next()
                        .unwrap();
                    *output = Ty::Builtin(BuiltinType::I32);
                }
                1 => {
                    forged.interface.associated_types.clear();
                    forged
                        .interface
                        .associated_types
                        .insert(forged.member, forged.receiver.clone());
                }
                2 => forged.interface.arguments.push(forged.receiver.clone()),
                3 => {
                    if protocol == Protocol::Neg {
                        forged.interface.arguments.push(forged.receiver.clone());
                    } else {
                        forged.interface.arguments[0] = Ty::Builtin(BuiltinType::I32);
                    }
                }
                4 => forged.arguments.push(forged.receiver.clone()),
                5 => forged.member = *forged.interface.associated_types.keys().next().unwrap(),
                6 => {
                    forged.member = context
                        .intern_child(
                            forged.interface.declaration,
                            DefinitionKind::Method,
                            "forged",
                            0,
                        )
                        .unwrap();
                }
                7 => {
                    // Keep the signature shape consistent: only the existing
                    // primitive proof can reject this nonnumeric receiver.
                    forged.receiver = Ty::Builtin(BuiltinType::Bool);
                    forged.interface.arguments.fill(forged.receiver.clone());
                    for output in forged.interface.associated_types.values_mut() {
                        *output = forged.receiver.clone();
                    }
                    assert!(adapter_contract(&forged, &definitions).is_some());
                }
                _ => unreachable!(),
            }
            assert!(
                proof
                    .implicit_callable_signature(&forged, &[], &Default::default())
                    .unwrap()
                    .is_none(),
                "{protocol:?}: forged requirement {mutation}",
            );
        }

        let mut missing = program.clone();
        let operations = missing
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| match instruction {
                BytecodeInstruction::Call {
                    callee: CallTarget::InterfaceMethod { contract, .. },
                    ..
                } if contract.operations.iter().any(|operation| matches!(
                    operation,
                    OperationWitness::Selected(call) if call.origin == NativeCallableOrigin::ProtocolAdapter
                )) => Some(&mut contract.operations),
                _ => None,
            })
            .unwrap();
        operations.clear();
        assert!(verify_program(&missing, &context.snapshot()).is_err());
    }
}

#[test]
fn narrow_arithmetic_retains_source_width_without_assert_expansion() {
    let context = DefinitionContext::new().unwrap();
    for (name, input) in [
        ("i8", BuiltinType::I8),
        ("i16", BuiltinType::I16),
        ("u8", BuiltinType::U8),
        ("u16", BuiltinType::U16),
        ("u32", BuiltinType::U32),
    ] {
        for (operator, expected) in [
            ("+", IntegerOp::CheckedAdd),
            ("-", IntegerOp::CheckedSub),
            ("*", IntegerOp::CheckedMul),
            ("/", IntegerOp::CheckedDiv),
            ("%", IntegerOp::CheckedRem),
        ] {
            let source = format!(
                "fn calc(a: {name}, b: {name}) -> {name} {{ a {operator} b }}
                 fn main() -> {name} {{ calc(6{name}, 3{name}) }}"
            );
            let program = common::adopt_bytecode(&context, common::bytecode_ok(&source));
            let function = program.modules[program.root.index()]
                .functions
                .iter()
                .find(|function| function.name == "calc")
                .unwrap();
            let operations = function
                .instructions
                .iter()
                .filter_map(|instruction| match instruction {
                    BytecodeInstruction::Numeric { operation, .. } => Some(operation),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(operations.len(), 1, "{source}");
            assert_eq!(operations[0].op, expected);
            assert_eq!(operations[0].input, input);
            assert_eq!(operations[0].rhs, Some(input));
            assert!(function.instructions.iter().all(|instruction| !matches!(
                instruction,
                BytecodeInstruction::Call { .. } | BytecodeInstruction::Binary { .. }
            )));

            let mut malformed = program.clone();
            let function = malformed.modules[malformed.root.index()]
                .functions
                .iter_mut()
                .find(|function| function.name == "calc")
                .unwrap();
            let operation = function
                .instructions
                .iter_mut()
                .find_map(|instruction| match instruction {
                    BytecodeInstruction::Numeric { operation, .. } => Some(operation),
                    _ => None,
                })
                .unwrap();
            let original = *operation;
            operation.rhs = Some(BuiltinType::F32);
            assert!(verify_program(&malformed, &context.snapshot()).is_err());
            let operation = malformed.modules[malformed.root.index()]
                .functions
                .iter_mut()
                .find(|function| function.name == "calc")
                .unwrap()
                .instructions
                .iter_mut()
                .find_map(|instruction| match instruction {
                    BytecodeInstruction::Numeric { operation, .. } => Some(operation),
                    _ => None,
                })
                .unwrap();
            *operation = original;
            operation.input = BuiltinType::I64;
            operation.rhs = Some(BuiltinType::I64);
            assert!(verify_program(&malformed, &context.snapshot()).is_err());
        }
    }
}

#[test]
fn narrow_negation_uses_the_checked_source_domain() {
    let context = DefinitionContext::new().unwrap();
    for (name, input) in [("i8", BuiltinType::I8), ("i16", BuiltinType::I16)] {
        let program = common::adopt_bytecode(
            &context,
            common::bytecode_ok(&format!(
                "fn neg(a: {name}) -> {name} {{ -a }} fn main() -> {name} {{ neg(6{name}) }}"
            )),
        );
        let function = program.modules[program.root.index()]
            .functions
            .iter()
            .find(|function| function.name == "neg")
            .unwrap();
        assert!(function.instructions.iter().any(|instruction| matches!(
            instruction,
            BytecodeInstruction::Numeric { operation, .. }
                if operation.op == IntegerOp::CheckedSub && operation.input == input
        )));
        assert!(function.instructions.iter().all(|instruction| !matches!(
            instruction,
            BytecodeInstruction::Call { .. } | BytecodeInstruction::Unary { .. }
        )));
    }
}
