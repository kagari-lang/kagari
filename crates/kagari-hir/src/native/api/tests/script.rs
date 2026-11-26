//! Complete embedded definitions are source-owned, not reconstructed default stubs.

use crate::{
    hir::{expr::ExprKind, item::function::FunctionImplementation},
    lower::lower_module,
    resolver::collect::resolve_names,
    tests::common::check_module,
};
use kagari_common::identity::map::DefinitionContext;
use kagari_source::source::SourceFile;

#[test]
fn embedded_script_definitions_keep_bodies_fields_and_native_binding_separate() {
    let context = DefinitionContext::new().unwrap();
    let source = SourceFile::new(
        "embedded.kgr",
        include_str!("../../../../tests/fixtures/native/embedded-script.kgr"),
    );
    let lowered = lower_module(&source);
    assert_eq!(lowered.module.structs[0].fields.len(), 1);
    assert!(lowered.module.traits[0].methods[0].has_default);
    let get = lowered.module.traits[0].methods[0].function;
    let body = lowered.module.functions[get.index()]
        .implementation
        .script_body()
        .unwrap();
    assert!(matches!(
        lowered
            .module
            .expr(lowered.module.block(body).tail_expr.unwrap())
            .kind,
        ExprKind::Literal(_)
    ));
    let read = lowered
        .module
        .functions
        .iter()
        .find(|function| function.name == "read")
        .unwrap();
    let body = read.implementation.script_body().unwrap();
    assert!(matches!(
        lowered
            .module
            .expr(lowered.module.block(body).tail_expr.unwrap())
            .kind,
        ExprKind::Binary { .. }
    ));
    let method = lowered.module.impls[0].methods[0].function;
    let body = lowered.module.functions[method.index()]
        .implementation
        .script_body()
        .unwrap();
    assert!(matches!(
        lowered
            .module
            .expr(lowered.module.block(body).tail_expr.unwrap())
            .kind,
        ExprKind::Field { .. }
    ));
    let natives = lowered
        .module
        .functions
        .iter()
        .filter(|function| matches!(function.implementation, FunctionImplementation::Native(_)))
        .collect::<Vec<_>>();
    assert_eq!(natives.len(), 1);
    assert_eq!(natives[0].name, "native_echo");
    let names = resolve_names(&lowered).unwrap().into_checked().unwrap();
    let checked = check_module(&context, &lowered, &names);
    assert!(
        checked.diagnostics().is_empty(),
        "{:?}",
        checked.diagnostics()
    );
}
