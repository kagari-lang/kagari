use crate::{
    hir::{
        ExprKind, FunctionKind, Item, PatternKind, PlaceKind, StmtKind, TypeKind, Visibility,
        Writeability,
    },
    tests::common,
};

#[test]
fn lowers_items_into_hir_module() {
    let lowered = common::lower_ok(
        r#"
struct Player { val id: i32, var hp: i32 }
enum Color { Red, Blue }
fn main(value: i32) -> i32 { value }
"#,
    );

    assert_eq!(lowered.module.items.len(), 3);
    assert!(matches!(lowered.module.items[0], Item::Struct(_)));
    assert!(matches!(lowered.module.items[1], Item::Enum(_)));
    assert!(matches!(lowered.module.items[2], Item::Function(_)));

    assert_eq!(lowered.module.structs[0].name, "Player");
    assert_eq!(lowered.module.structs[0].fields.len(), 2);
    assert_eq!(
        lowered.module.structs[0].fields[0].writeability,
        Writeability::Val
    );
    assert_eq!(
        lowered.module.structs[0].fields[1].writeability,
        Writeability::Var
    );
    assert_eq!(lowered.module.enums[0].name, "Color");
    assert_eq!(lowered.module.functions[0].name, "main");
    assert_eq!(
        lowered.module.functions[0].params[0].writeability,
        Writeability::Val
    );
}

#[test]
fn lowers_module_trait_and_impl_namespace_items() {
    let lowered = common::lower_ok(
        r#"
mod gameplay;

trait Display {
    fn show(self) -> String;
}

struct Player {
    val name: String,
}

impl Display for Player {
    fn show(self) -> String {
        self.name
    }
}
"#,
    );

    assert_eq!(lowered.module.modules.len(), 1);
    assert_eq!(lowered.module.modules[0].name, "gameplay");
    assert!(!lowered.module.modules[0].inline);

    assert_eq!(lowered.module.traits.len(), 1);
    assert_eq!(lowered.module.traits[0].name, "Display");
    assert_eq!(lowered.module.traits[0].methods.len(), 1);
    let trait_method = &lowered.module.traits[0].methods[0];
    assert_eq!(trait_method.name, "show");
    let trait_function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.id == trait_method.function)
        .expect("expected lowered trait method function");
    assert!(matches!(trait_function.kind, FunctionKind::TraitMethod));
    assert_eq!(trait_function.params[0].name, "self");

    assert_eq!(lowered.module.impls.len(), 1);
    assert_eq!(
        lowered.module.impls[0].trait_ref.as_deref(),
        Some("Display")
    );
    let for_type = lowered.module.impls[0]
        .for_type
        .expect("expected impl target type");
    assert!(matches!(
        lowered.module.type_ref(for_type).kind,
        TypeKind::Named(ref name) if name == "Player"
    ));
    assert_eq!(lowered.module.impls[0].methods.len(), 1);
    let impl_method = &lowered.module.impls[0].methods[0];
    assert_eq!(impl_method.name, "show");
    let impl_function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.id == impl_method.function)
        .expect("expected lowered impl method function");
    assert!(matches!(impl_function.kind, FunctionKind::ImplMethod));
    assert_eq!(impl_function.params[0].name, "self");

    assert!(matches!(lowered.module.items[0], Item::Module(_)));
    assert!(matches!(lowered.module.items[1], Item::Trait(_)));
    assert!(matches!(lowered.module.items[2], Item::Struct(_)));
    assert!(matches!(lowered.module.items[3], Item::Impl(_)));
}

#[test]
fn lowers_function_body_expressions_and_statements() {
    let lowered = common::lower_ok(
        "fn main(value: i32) -> i32 { val next: i32 = value + 1; match next { _ => next } }",
    );
    let function = &lowered.module.functions[0];

    assert_eq!(function.params.len(), 1);
    assert!(matches!(
        &lowered.module.type_ref(function.params[0].ty).kind,
        TypeKind::Named(name) if name == "i32"
    ));

    let block = lowered.module.block(function.body);
    let stmt = lowered.module.stmt(block.statements[0]);
    match &stmt.kind {
        StmtKind::Binding {
            local: _,
            writeability,
            name,
            ty,
            initializer,
        } => {
            assert_eq!(*writeability, Writeability::Val);
            assert_eq!(name, "next");
            assert!(matches!(
                ty.map(|ty| &lowered.module.type_ref(ty).kind),
                Some(TypeKind::Named(name)) if name == "i32"
            ));
            assert!(matches!(
                &lowered.module.expr(*initializer).kind,
                ExprKind::Binary { .. }
            ));
        }
        other => panic!("unexpected stmt kind: {other:?}"),
    }

    let tail_expr = block.tail_expr.expect("expected tail expr");
    match &lowered.module.expr(tail_expr).kind {
        ExprKind::Match { arms, .. } => {
            assert_eq!(arms.len(), 1);
            assert!(matches!(
                lowered.module.pattern(arms[0].pattern).kind,
                PatternKind::Wildcard
            ));
        }
        other => panic!("unexpected tail expr: {other:?}"),
    }

    let assign = common::lower_ok("fn main(value: i32) -> i32 { value = 1; value }");
    let assign_block = assign.module.block(assign.module.functions[0].body);
    match &assign.module.stmt(assign_block.statements[0]).kind {
        StmtKind::Assign { target, .. } => {
            assert!(matches!(
                assign.module.place(*target).kind,
                PlaceKind::Name(ref name) if name == "value"
            ));
        }
        other => panic!("unexpected assign stmt kind: {other:?}"),
    }

    let nested = common::lower_ok(
        r#"
struct Point { var x: i32 }
struct Holder { var inner: Point }

fn main() {
    var holder = Holder { inner: Point { x: 1 } };
    holder.inner.x = 2;
}
"#,
    );
    let block = nested.module.block(nested.module.functions[0].body);
    match &nested.module.stmt(block.statements[1]).kind {
        StmtKind::Assign { target, .. } => {
            assert!(matches!(
                &nested.module.place(*target).kind,
                PlaceKind::Field { name, .. } if name == "x"
            ));
        }
        other => panic!("unexpected nested assign stmt kind: {other:?}"),
    }
}

#[test]
fn lowers_var_binding() {
    let lowered = common::lower_ok("fn main() { var value = 1; value = 2; }");
    let function = &lowered.module.functions[0];
    let block = lowered.module.block(function.body);
    let stmt = lowered.module.stmt(block.statements[0]);

    match &stmt.kind {
        StmtKind::Binding {
            writeability, name, ..
        } => {
            assert_eq!(*writeability, Writeability::Var);
            assert_eq!(name, "value");
        }
        other => panic!("unexpected stmt kind: {other:?}"),
    }
}

#[test]
fn lowers_top_level_statements_into_module_init_function() {
    let lowered = common::lower_ok(
        r#"
val boot = 1;

fn main() -> i32 { 1 }
"#,
    );

    let module_init = lowered
        .module
        .module_init
        .expect("expected implicit module init function");
    let function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.id == module_init)
        .expect("expected module init function in function list");

    assert_eq!(function.name, "__module_init__");
    assert!(matches!(function.kind, FunctionKind::ModuleInit));

    let block = lowered.module.block(function.body);
    assert_eq!(block.statements.len(), 1);
    assert!(matches!(
        lowered.module.stmt(block.statements[0]).kind,
        StmtKind::Binding { .. }
    ));
}

#[test]
fn lowers_top_level_tail_expression_into_module_init_result() {
    let lowered = common::lower_ok(
        r#"
val boot = 1;

boot + 1
"#,
    );

    let module_init = lowered
        .module
        .module_init
        .expect("expected implicit module init function");
    let function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.id == module_init)
        .expect("expected module init function in function list");

    let block = lowered.module.block(function.body);
    let tail_expr = block.tail_expr.expect("expected module init tail expr");
    assert!(matches!(
        lowered.module.expr(tail_expr).kind,
        ExprKind::Binary { .. }
    ));
}

#[test]
fn lowers_const_items_and_exports() {
    let lowered = common::lower_ok(
        r#"
pub const VERSION: i32 = 1;
"#,
    );

    assert_eq!(lowered.module.items.len(), 1);
    assert_eq!(lowered.module.exports.len(), 1);

    let const_item = &lowered.module.consts[0];
    assert_eq!(const_item.name, "VERSION");
    assert!(matches!(const_item.visibility, Visibility::Public));
    assert!(matches!(
        lowered.module.expr(const_item.initializer).kind,
        ExprKind::Literal(_)
    ));

    assert!(lowered.module.statics.is_empty());
}
