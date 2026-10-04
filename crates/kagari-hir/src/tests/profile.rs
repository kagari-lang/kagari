use crate::{analyze_source, typeck::table::CallTarget};
use kagari_source::source::SourceFile;
use kagari_stdlib::catalog as foundation_catalog;

#[test]
fn generic_type_binding_does_not_acquire_same_named_trait_permissions() {
    let module = SourceFile::new(
        "profile.kgr",
        "trait Show { fn value(self) -> i32; } fn identity<Show>(value: Show) -> Show { value }",
    );
    analyze_source(&module, foundation_catalog::shared())
        .expect("installed declaration analysis")
        .into_codegen()
        .expect("generic binding shadows trait spelling in type scope");
}

#[test]
fn same_named_user_functions_are_not_reflection_helpers() {
    let module = SourceFile::new(
        "profile.kgr",
        "fn type_of(value: i32) -> i32 { value + 1 } fn main() -> i32 { type_of(41) }",
    );
    let checked = analyze_source(&module, foundation_catalog::shared())
        .expect("installed declaration analysis")
        .into_codegen()
        .expect("resolved user function does not need reflection permission");
    let main = &checked.lowered.module.functions[1];
    let call = checked
        .lowered
        .module
        .block(main.body.unwrap())
        .tail_expr
        .unwrap();
    assert_eq!(
        checked
            .typed
            .type_table
            .call_resolution(call)
            .unwrap()
            .target,
        CallTarget::Function(checked.lowered.module.functions[0].id)
    );
}

#[test]
fn reflection_uses_language_types_without_a_permission_profile() {
    let module = SourceFile::new("reflection.kgr", "fn main() -> String { type_of(7) }");
    analyze_source(&module, foundation_catalog::shared())
        .expect("installed declaration analysis")
        .into_codegen()
        .expect("reflection is a language operation");
}

#[test]
fn static_trait_bounds_do_not_require_interface_value_permission() {
    let module = SourceFile::new(
        "bounds.kgr",
        "trait Show { fn show(self) -> i32; } fn inline<T: Show>(value: T) -> i32 { value.show() } fn predicate<T>(value: T) -> i32 where T: Show { value.show() }",
    );
    let analysis = analyze_source(&module, foundation_catalog::shared())
        .expect("installed declaration analysis");
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );
}

#[test]
fn interface_values_do_not_require_permission() {
    let module = SourceFile::new(
        "interface.kgr",
        "trait Show { fn show(self) -> String; } fn render(value: Show) -> String { value.show() }",
    );
    analyze_source(&module, foundation_catalog::shared())
        .expect("installed declaration analysis")
        .into_codegen()
        .expect("interface values are statically checked");
}
