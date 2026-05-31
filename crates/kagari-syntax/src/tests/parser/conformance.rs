use kagari_common::{DiagnosticKind, Severity};

use crate::{
    ast::{Expr, Item},
    tests::common,
};

#[test]
fn parses_unit_type_and_unit_value() {
    let module = common::parse_ok("fn noop() -> () { () }");
    let function = common::first_function(&module);

    let return_type = function.return_type().expect("expected unit return type");
    assert_eq!(
        return_type
            .tuple_type()
            .expect("expected tuple type")
            .element_types()
            .count(),
        0
    );

    let body = function.body().expect("expected function body");
    match body.tail_expr().expect("expected unit tail expression") {
        Expr::TupleExpr(tuple) => assert_eq!(tuple.elements().count(), 0),
        other => panic!("unexpected unit expression: {other:?}"),
    }
}

#[test]
fn parses_spec_valid_trait_interface_type_paths() {
    let module = common::parse_ok(
        r#"
fn apply(effect: effects::SkillEffect) -> crate::Result {
    effect
}
"#,
    );
    let function = common::first_function(&module);
    let params = function
        .param_list()
        .expect("expected parameter list")
        .params()
        .collect::<Vec<_>>();

    assert_eq!(
        params[0].ty().and_then(|ty| ty.name_text()).as_deref(),
        Some("effects::SkillEffect")
    );
    assert_eq!(
        function
            .return_type()
            .and_then(|ty| ty.name_text())
            .as_deref(),
        Some("crate::Result")
    );
}

#[test]
fn parses_generic_items_traits_impls_and_enum_payloads() {
    let module = common::parse_ok(
        r#"
pub struct PlayerInfo<T> {
    val id: T,
    pub var title: String,
}

enum Maybe<T> {
    None,
    Some(T),
}

trait Display {
    fn to_string(self) -> String;
}

impl<T> Display for PlayerInfo<T>
where T: Display + Clone
{
    pub fn to_string(self) -> String {
        self.title
    }
}

fn identity<T: Clone>(value: T) -> T
where T: Display + Clone
{
    value
}
"#,
    );

    let items = module.items().collect::<Vec<_>>();
    assert_eq!(items.len(), 5);

    match &items[0] {
        Item::StructDef(struct_def) => {
            assert!(struct_def.is_pub());
            assert_eq!(struct_def.name_text().as_deref(), Some("PlayerInfo"));
            assert_eq!(
                struct_def
                    .generic_params()
                    .expect("expected generic params")
                    .params()
                    .count(),
                1
            );
        }
        other => panic!("unexpected first item: {other:?}"),
    }

    match &items[1] {
        Item::EnumDef(enum_def) => {
            let variants = enum_def
                .variant_list()
                .expect("expected variants")
                .variants()
                .collect::<Vec<_>>();
            assert_eq!(variants.len(), 2);
            assert!(variants[0].payload_types().is_none());
            assert_eq!(
                variants[1]
                    .payload_types()
                    .expect("expected tuple payload")
                    .types()
                    .count(),
                1
            );
        }
        other => panic!("unexpected second item: {other:?}"),
    }

    match &items[2] {
        Item::TraitDef(trait_def) => {
            assert_eq!(trait_def.name_text().as_deref(), Some("Display"));
            let methods = trait_def.methods().collect::<Vec<_>>();
            assert_eq!(methods.len(), 1);
            assert_eq!(methods[0].name_text().as_deref(), Some("to_string"));
            assert_eq!(
                methods[0]
                    .param_list()
                    .expect("expected method params")
                    .params()
                    .count(),
                1
            );
            assert!(methods[0].body().is_none());
        }
        other => panic!("unexpected third item: {other:?}"),
    }

    match &items[3] {
        Item::ImplBlock(impl_block) => {
            assert_eq!(
                impl_block
                    .generic_params()
                    .expect("expected impl generics")
                    .params()
                    .count(),
                1
            );
            assert_eq!(
                impl_block
                    .where_clause()
                    .expect("expected where clause")
                    .predicates()
                    .count(),
                1
            );
            let methods = impl_block.methods().collect::<Vec<_>>();
            assert_eq!(methods.len(), 1);
            assert!(methods[0].is_pub());
            assert!(methods[0].body().is_some());
        }
        other => panic!("unexpected fourth item: {other:?}"),
    }

    match &items[4] {
        Item::FnDef(function) => {
            assert_eq!(function.name_text().as_deref(), Some("identity"));
            assert!(function.generic_params().is_some());
            assert!(function.where_clause().is_some());
        }
        other => panic!("unexpected fifth item: {other:?}"),
    }
}

#[test]
fn invalid_syntax_diagnostics_explain_spec_replacements() {
    let parse = common::parse("fn main() { let mut value = 1; }");
    assert_eq!(parse.diagnostics().len(), 1);
    assert_eq!(parse.diagnostics()[0].severity, Severity::Error);
    assert_eq!(
        parse.diagnostics()[0].kind,
        DiagnosticKind::InvalidLetBinding
    );
    assert_eq!(
        parse.diagnostics()[0].to_string(),
        "Error: `let` bindings are not valid; use `val` or `var` at 12..15"
    );

    let parse = common::parse("fn apply(effect: dyn Effect) {}");
    assert_eq!(parse.diagnostics().len(), 1);
    assert_eq!(parse.diagnostics()[0].severity, Severity::Error);
    assert_eq!(
        parse.diagnostics()[0].kind,
        DiagnosticKind::InvalidDynTraitSyntax
    );
    assert_eq!(
        parse.diagnostics()[0].to_string(),
        "Error: `dyn` interface type syntax is not valid; use the trait name directly at 17..20"
    );

    let parse = common::parse("struct Player { hp: i32 }");
    assert_eq!(parse.diagnostics().len(), 1);
    assert_eq!(parse.diagnostics()[0].severity, Severity::Error);
    assert_eq!(
        parse.diagnostics()[0].kind,
        DiagnosticKind::ExpectedFieldBinding
    );
    assert_eq!(
        parse.diagnostics()[0].to_string(),
        "Error: expected `val` or `var` before field name at 16..18"
    );
}
