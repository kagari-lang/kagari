//! Temporary HA2 parity boundary: existing compilation and explicit operation admission.
//! HA3 removes the old path after switching the entire source-body consumer closure.

use super::*;
use crate::source::{
    elaborate::model::{CheckedExprKind, Conversion},
    lower::instances::MirLoweringOptions,
    program::lower_program_to_mir,
};
use kagari_hir::{
    analysis::AnalysisDatabase, compile_input::program::CheckedCompilation, program::CheckedProgram,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

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
"#,
    );
    lower_program_to_mir(&program, &MirLoweringOptions::default()).unwrap();
    let compilation = CheckedCompilation::from_program(&program, &Default::default()).unwrap();
    let module = compilation
        .modules()
        .iter()
        .find(|module| module.handle() == compilation.root())
        .unwrap();
    let mut conversion_seen = false;
    let mut closure_seen = false;
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
                    closure_seen = true;
                }
                _ => {}
            }
        }
    }
    assert!(conversion_seen);
    assert!(closure_seen);
}

/// Reuses semantic-owner fixtures while the production lowerer is still the comparison path.
pub(crate) fn assert_expands(program: &CheckedProgram) {
    let compilation = CheckedCompilation::from_program(program, &Default::default()).unwrap();
    for module in compilation.modules() {
        for input in module.bodies() {
            let body = elaborate(input, &Default::default())
                .unwrap_or_else(|error| panic!("{:?}: {error:?}", input.key()));
            for operation in &body.operations {
                assert_eq!(
                    input.expression_span(operation.source),
                    Some(operation.span)
                );
                assert!(!operation.ty.is_unresolved());
            }
        }
    }
}
