//! Temporary HA2 parity boundary: existing compilation and explicit operation admission.
//! HA3 removes the old path after switching the entire source-body consumer closure.

use super::*;
use crate::source::{
    elaborate::model::{CheckedExprKind, Conversion},
    lower::instances::MirLoweringOptions,
    program::lower_program_to_mir,
};
use kagari_abi::representation::ValueType;
use kagari_hir::{
    analysis::AnalysisDatabase,
    compile_input::{body::bindings::BindingId, program::CheckedCompilation},
    program::CheckedProgram,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
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
