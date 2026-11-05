//! Temporary HA2 parity boundary: existing compilation and explicit operation admission.
//! HA3 removes the old path after switching the entire source-body consumer closure.

use super::*;
use crate::source::{
    elaborate::model::{
        CheckedCallTarget, CheckedExprKind, CheckedPlaceKind, Conversion, ValueBinding,
    },
    lower::instances::MirLoweringOptions,
    program::lower_program_to_mir,
};
use kagari_abi::representation::ValueType;
use kagari_contract::representation::semantic_representation;
use kagari_hir::{
    analysis::AnalysisDatabase,
    compile_input::{body::bindings::BindingId, program::CheckedCompilation},
    program::CheckedProgram,
    typeck::scalar::ScalarValue,
    types::{TypeId, semantic::lower_type},
};
use kagari_mir::instruction::{CallTarget as MirCallTarget, Constant, Instruction};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::collections::HashSet;

fn checked(text: &str) -> CheckedProgram {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("elaborate.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(kagari_stdlib::catalog::shared().unwrap());
    analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap()
}

#[test]
fn existing_contracts_expand_without_resolving_source_again() {
    for text in [
        "fn main() -> i32 { val (left, right) = loop {}; 0 }",
        "fn main() -> i32 { -(loop {}) }",
        "fn main() -> Result<i32, i32> { (loop {})? }",
        "fn relay(value: Result<i32, i32>) -> Result<i32, i32> { Ok(value? + 1) }",
        include_str!("../../../tests/fixtures/language_foundation/scalar-language.kgr"),
        include_str!(
            "../../../tests/fixtures/language_foundation/collection-interface-iteration.kgr"
        ),
        include_str!(
            "../../../tests/fixtures/language_foundation/foundational-static-methods-and-bounds.kgr"
        ),
    ] {
        let program = checked(text);
        lower_program_to_mir(&program, &MirLoweringOptions::default()).unwrap();
        assert_expands(&program);
    }
}

#[test]
fn synthetic_conversions_and_nested_callables_keep_separate_roots() {
    let program = checked(
        r#"
struct Number { val value: i32 }
impl Fn<(i32,)> for Number {
    type Output = i32;
    fn call(self, args: (i32,)) -> i32 { self.value + args[0] }
}
fn convert() -> fn(i32) -> i32 { Number { value: 2 } }
fn captured(x: i32) -> fn(i32) -> i32 { |y: i32| x + y }
fn mutable(x: i32) -> fn() -> i32 { var x = x; || { x = x + 1; x } }
"#,
    );
    let legacy = lower_program_to_mir(&program, &MirLoweringOptions::default()).unwrap();
    let compilation = CheckedCompilation::from_program(&program, &Default::default()).unwrap();
    let module = compilation
        .modules()
        .iter()
        .find(|module| module.handle() == compilation.root())
        .unwrap();
    let legacy_module = legacy
        .modules()
        .iter()
        .find(|candidate| &candidate.identity == module.source().module_identity())
        .unwrap();
    let mut conversion_seen = false;
    let mut closure_seen = false;
    let mut cell_seen = false;
    for input in module.bodies() {
        let body = elaborate(input, &Default::default()).unwrap();
        for (index, operation) in body.operations.iter().enumerate() {
            match &operation.kind {
                CheckedExprKind::Convert {
                    value,
                    conversion: Conversion::Callable { .. },
                } => {
                    assert_ne!(value.0, index);
                    assert_eq!(body.operation(*value).source, operation.source);
                    assert_eq!(body.operation(*value).span, operation.span);
                    conversion_seen = true;
                }
                CheckedExprKind::Closure { key, captures, .. } => {
                    assert!(!captures.is_empty());
                    let nested = module.body(*key).unwrap();
                    assert_eq!(nested.key().arena, input.key().arena);
                    assert_ne!(nested.key(), input.key());
                    let nested = elaborate(nested, &Default::default()).unwrap();
                    assert!(!nested.operations.is_empty());
                    assert_eq!(nested.captures.len(), captures.len());
                    let legacy_closure = legacy_module
                        .functions
                        .iter()
                        .find(|function| {
                            function.debug.source_span == operation.span
                                && !function.debug.captured_bindings.is_empty()
                        })
                        .unwrap();
                    assert_eq!(legacy_closure.debug.captured_bindings.len(), captures.len());
                    for (capture_index, (capture, incoming)) in
                        captures.iter().zip(&nested.captures).enumerate()
                    {
                        let legacy_capture = &legacy_closure.debug.captured_bindings[capture_index];
                        assert_eq!(legacy_capture.name, capture.name);
                        assert_eq!(legacy_capture.span, capture.span);
                        assert_eq!(
                            legacy_closure.params[capture_index].ty,
                            if capture.by_cell {
                                ValueType::HeapObject
                            } else {
                                ValueType::I32
                            }
                        );
                        assert_eq!(capture.id, incoming.id);
                        assert_eq!(capture.name, "x");
                        assert_eq!(capture.name, incoming.name);
                        assert_eq!(capture.span, incoming.span);
                        assert_eq!(capture.ty, incoming.ty);
                        assert_eq!(capture.by_cell, incoming.by_cell);
                        assert!(
                            module.source().text()[capture.span.start..capture.span.end]
                                .contains("x")
                        );
                        if let BindingId::Local(local) = capture.id {
                            assert!(capture.by_cell, "the shadowing var captures by cell");
                            assert!(body.cell_locals.contains(&local));
                            assert!(nested.cell_locals.contains(&local));
                            cell_seen = true;
                        } else {
                            assert!(
                                !capture.by_cell,
                                "the original parameter is captured by value"
                            );
                        }
                    }
                    closure_seen = true;
                }
                _ => {}
            }
        }
    }
    assert!(conversion_seen);
    assert!(closure_seen);
    assert!(cell_seen);
}

/// Reuses semantic-owner fixtures while the production lowerer is still the comparison path.
pub(crate) fn assert_expands(program: &CheckedProgram) {
    let compilation = CheckedCompilation::from_program(program, &Default::default()).unwrap();
    for module in compilation.modules() {
        let mut pending = module.bodies().collect::<Vec<_>>();
        let mut visited = HashSet::new();
        while let Some(input) = pending.pop() {
            if !visited.insert(input.key()) {
                continue;
            }
            let body = elaborate(input, &Default::default())
                .unwrap_or_else(|error| panic!("{:?}: {error:?}", input.key()));
            for operation in &body.operations {
                assert_eq!(
                    input.expression_span(operation.source),
                    Some(operation.span)
                );
                assert!(!operation.ty.is_unresolved());
                if let CheckedExprKind::Closure { key, .. }
                | CheckedExprKind::AsyncBlock { key, .. } = &operation.kind
                {
                    pending.push(module.body(*key).expect("admitted nested callable"));
                }
            }
        }
    }
}

#[test]
fn imported_constants_and_static_places_keep_canonical_value_contracts() {
    let program = checked(
        r#"
mod data { pub const OFFSET: i32 = 4; pub static var VALUE: i32 = OFFSET; }
use self::data::OFFSET as STEP;
use self::data::VALUE as CURRENT;
fn main() -> i32 { CURRENT += STEP; CURRENT }
"#,
    );
    let legacy = lower_program_to_mir(&program, &Default::default()).unwrap();
    let compilation = CheckedCompilation::from_program(&program, &Default::default()).unwrap();
    let module = compilation
        .modules()
        .iter()
        .find(|module| module.handle() == compilation.root())
        .unwrap();
    let input = module
        .bodies()
        .find(|input| {
            input
                .enclosing_signature()
                .is_some_and(|signature| signature.name == "main")
        })
        .unwrap();
    let body = elaborate(input, &Default::default()).unwrap();
    let legacy_module = legacy
        .modules()
        .iter()
        .find(|candidate| &candidate.identity == module.source().module_identity())
        .unwrap();
    let slot = legacy_module
        .module_slots
        .iter()
        .find(|slot| slot.contract.name == "VALUE")
        .unwrap();
    let identity = legacy_module
        .definitions()
        .resolve(slot.contract.declaration)
        .unwrap()
        .to_path();
    let mut read = false;
    let mut constant = false;
    for operation in &body.operations {
        match &operation.kind {
            CheckedExprKind::Read(ValueBinding::Static(declaration)) => {
                assert_eq!(
                    input
                        .environment()
                        .definitions()
                        .resolve(*declaration)
                        .unwrap()
                        .to_path(),
                    identity
                );
                assert_eq!(
                    &module.source().text()[operation.span.start..operation.span.end],
                    "CURRENT"
                );
                read = true;
            }
            CheckedExprKind::Constant(ScalarValue::Integer { value: 4, .. }) => constant = true,
            _ => {}
        }
    }
    assert!(read && constant);
    let target = body
        .places
        .values()
        .find_map(|place| match place.kind {
            CheckedPlaceKind::Binding(ValueBinding::Static(declaration)) => Some(declaration),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        input
            .environment()
            .definitions()
            .resolve(target)
            .unwrap()
            .to_path(),
        identity
    );
    let main = legacy_module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    assert!(
        main.blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(|instruction| matches!(
                instruction,
                Instruction::LoadConst {
                    constant: Constant::I32(4),
                    ..
                }
            ))
    );
    assert!(
        main.blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(|instruction| matches!(instruction, Instruction::StoreModule { .. }))
    );
}

#[test]
fn selected_calls_keep_receiver_roles_and_applied_signatures() {
    let program = checked(
        r#"
trait Read { fn read(self, extra: i32) -> i32; }
struct Number { val value: i32, val callback: fn(i32) -> i32 }
impl Number { fn add(self, extra: i32) -> i32 { self.value + extra } }
impl Read for Number { fn read(self, extra: i32) -> i32 { self.value + extra } }
fn bounded<T: Read>(value: T) -> i32 { value.read(3) }
fn main() -> i32 {
    val number = Number { value: 4, callback: |x: i32| x };
    val first = number.add(1);
    val second = number.callback(2);
    val third = bounded(number);
    first + second + third
}
"#,
    );
    let legacy = lower_program_to_mir(&program, &Default::default()).unwrap();
    let compilation = CheckedCompilation::from_program(&program, &Default::default()).unwrap();
    let module = compilation
        .modules()
        .iter()
        .find(|module| module.handle() == compilation.root())
        .unwrap();
    let emitted_module = legacy
        .modules()
        .iter()
        .find(|emitted| &emitted.identity == module.source().module_identity())
        .unwrap();
    let definitions = module.environment().definitions();
    let source = module.source().text();
    let mut seen = HashSet::new();
    for input in module.bodies() {
        let body = elaborate(input, &Default::default()).unwrap();
        for operation in &body.operations {
            let CheckedExprKind::Call(call) = &operation.kind else {
                continue;
            };
            let span = operation.span;
            let spelling = &source[span.start..span.end];
            let emitted = emitted_module
                .functions
                .iter()
                .flat_map(|function| {
                    function.blocks.iter().flat_map(move |block| {
                        block
                            .instructions
                            .iter()
                            .zip(&block.instruction_spans)
                            .filter_map(move |(instruction, origin)| match instruction {
                                // Legacy debug metadata can use the containing statement's extent.
                                Instruction::Call { dst, callee, args }
                                    if origin.start <= span.start && origin.end >= span.end =>
                                {
                                    Some((function, dst, callee, args))
                                }
                                _ => None,
                            })
                    })
                })
                .collect::<Vec<_>>();
            let [(caller, dst, target, args)] = emitted.as_slice() else {
                panic!("expected one emitted call for {spelling}: {emitted:?}");
            };
            assert_eq!(dst.unwrap().ty, ValueType::I32);
            assert_eq!(
                call.signature.return_type,
                TypeId::Builtin(BuiltinType::I32)
            );
            assert_eq!(args.len(), call.signature.params.len());
            let operand_spellings = call
                .operands
                .iter()
                .map(|id| {
                    let span = input.expression_span(*id).unwrap();
                    &source[span.start..span.end]
                })
                .collect::<Vec<_>>();
            match &call.target {
                CheckedCallTarget::Direct {
                    declaration,
                    arguments,
                } => {
                    let MirCallTarget::Function(instance) = target else {
                        panic!("direct call: {target:?}");
                    };
                    let callee = &emitted_module.functions[instance.index()];
                    assert_eq!(
                        definitions.resolve(*declaration).unwrap().to_path(),
                        legacy
                            .definitions()
                            .resolve(callee.instance.declaration)
                            .unwrap()
                            .to_path()
                    );
                    assert_eq!(arguments.len(), callee.instance.arguments.len());
                    for (argument, emitted) in arguments.iter().zip(&callee.instance.arguments) {
                        let (TypeId::Struct(argument), Ty::Struct(emitted)) = (argument, emitted)
                        else {
                            panic!("expected the concrete Number application");
                        };
                        assert_eq!(
                            definitions.resolve(argument.declaration).unwrap().to_path(),
                            legacy
                                .definitions()
                                .resolve(emitted.declaration)
                                .unwrap()
                                .to_path()
                        );
                    }

                    for (ty, argument) in call.signature.params.iter().zip(args.iter()) {
                        assert_eq!(semantic_representation(&lower_type(ty)), argument.ty);
                    }
                    assert_eq!(
                        operand_spellings,
                        if spelling == "number.add(1)" {
                            vec!["number", "1"]
                        } else {
                            vec!["number"]
                        }
                    );
                }
                CheckedCallTarget::Value { callee } => {
                    let callee_span = input.expression_span(*callee).unwrap();
                    assert_eq!(
                        &source[callee_span.start..callee_span.end],
                        "number.callback"
                    );
                    assert!(!call.operands.contains(callee));
                    assert_eq!(operand_spellings, ["2"]);
                    assert_eq!(call.signature.params, [TypeId::Builtin(BuiltinType::I32)]);
                    let MirCallTarget::Closure {
                        params,
                        return_type,
                        ..
                    } = target
                    else {
                        panic!("callable field: {target:?}");
                    };
                    assert_eq!(params, &[ValueType::I32]);
                    assert_eq!(*return_type, ValueType::I32);
                }
                CheckedCallTarget::Method {
                    method,
                    interface,
                    receiver,
                    arguments,
                } => {
                    let selected = input
                        .environment()
                        .aggregates()
                        .trait_method(method)
                        .unwrap();
                    assert_eq!(selected.name, "read");
                    assert_eq!(selected.owner, interface.declaration);
                    assert_eq!(operand_spellings, ["value", "3"]);
                    assert!(arguments.is_empty());
                    let TypeId::Generic(parameter) = receiver else {
                        panic!("bound receiver lost binder");
                    };
                    assert_eq!(parameter.position, 0);
                    assert_eq!(
                        definitions.resolve(parameter.owner).unwrap().to_path(),
                        legacy
                            .definitions()
                            .resolve(caller.instance.declaration)
                            .unwrap()
                            .to_path()
                    );
                    assert_eq!(&call.signature.params[0], receiver);
                    assert_eq!(call.signature.params[1], TypeId::Builtin(BuiltinType::I32));
                    // Only the old instance planner substitutes Number and selects its implementation.
                    let MirCallTarget::Function(instance) = target else {
                        panic!("instantiated bound call: {target:?}");
                    };
                    assert_eq!(emitted_module.functions[instance.index()].name, "read");
                    assert_eq!(
                        args.iter().map(|argument| argument.ty).collect::<Vec<_>>(),
                        [ValueType::HeapObject, ValueType::I32]
                    );
                }
            }
            assert!(seen.insert(spelling.to_owned()));
        }
    }
    assert_eq!(
        seen,
        [
            "number.add(1)",
            "number.callback(2)",
            "bounded(number)",
            "value.read(3)"
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    );
}
