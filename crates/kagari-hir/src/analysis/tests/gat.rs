use crate::analysis::*;
use crate::types::*;
use kagari_stdlib::catalog as foundation_catalog;
use kagari_types::scalar::BuiltinType;

#[test]
fn generic_associated_types_preserve_member_arguments_and_normalize_static_calls() {
    let source = SourceFile::new(
        "gat.kgr",
        r#"
trait Family { type Item<T>; fn make<T>(self, value: T) -> Self::Item<T>; }
struct Number {}
impl Family for Number { type Item<U> = U; fn make<V>(self, value: V) -> V { value } }
fn make<T: Family>(x: T) -> T::Item<i32> { x.make(42) }
fn main() -> i32 { make(Number {}) }
"#,
    );
    let result = crate::analyze_source(&source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis");
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    let facts = result.facts();
    let function = facts
        .typed
        .signatures
        .functions()
        .iter()
        .find(|function| {
            function.name == "make" && !function.params.iter().any(|param| param.name == "self")
        })
        .unwrap();
    assert!(
        matches!(&function.return_type, TypeId::Projection { arguments, .. } if arguments == &[TypeId::Builtin(BuiltinType::I32)])
    );
}
