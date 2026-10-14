use crate::analyze_source;
use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};
use kagari_stdlib::catalog;

#[test]
fn scoped_reference_targets_forward_without_copy_or_capability_upgrade() {
    let analysis = analyze_source(
        &SourceFile::new(
            "refs.kgr",
            r#"
        struct Player { var hp: i32 }
        fn inspect(value: Ref<Player>) -> i32 { 1 }
        fn heal(value: RefMut<Player>) -> i32 { 2 }
        fn forward(value: Ref<Player>) -> i32 { inspect(value) }
        fn inferred(value: Ref<Player>) -> i32 {
            val local: Ref<_> = value;
            inspect(local)
        }
        fn forward_mut(value: RefMut<Player>) -> i32 { heal(value) }
        fn read_mut(value: RefMut<Player>) -> i32 { inspect(value) }
        fn narrowed(value: RefMut<Player>) -> i32 {
            val shared: Ref<Player> = value;
            inspect(shared)
        }
        fn generic<T>(value: Ref<T>) -> i32 { 3 }
        fn apply(value: Ref<Player>) -> i32 { generic(value) }
        fn apply_mut(value: RefMut<Player>) -> i32 { generic(value) }
    "#,
        ),
        catalog::shared().unwrap(),
    )
    .unwrap();
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );

    for call in ["heal(value)", "inspect(Player { hp: 1 })"] {
        let analysis = analyze_source(
            &SourceFile::new(
                "upgrade.kgr",
                format!(
                    r#"
            struct Player {{ var hp: i32 }}
            fn inspect(value: Ref<Player>) -> i32 {{ 1 }}
            fn heal(value: RefMut<Player>) -> i32 {{ 2 }}
            fn reject(value: Ref<Player>) -> i32 {{ {call} }}
        "#
                ),
            ),
            catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(!analysis.diagnostics().is_empty(), "{call}");
    }
}

#[test]
fn scoped_reference_escape_storage_targets_and_captures_reject() {
    for source in [
        "fn wrong(value: Ref<i32>) {}",
        "fn wrong(value: Ref<Vec<i32>>) {}",
        "fn wrong(value: Ref<Ref<Player>>) {}",
        "fn wrong(value: Vec<Ref<Player>>) {}",
        "struct Saved { var value: Ref<Player> }",
        "enum Saved { View(Ref<Player>) }",
        "fn wrong(value: Ref<Player>) -> Ref<Player> { value }",
        "fn wrong(value: Ref<Player>) { val saved = [value]; }",
        "fn wrong(value: Ref<Player>) { val saved = (value, 1); }",
    ] {
        let analysis = analyze_source(
            &SourceFile::new(
                "escape.kgr",
                format!("struct Player {{ var hp: i32 }} {source}"),
            ),
            catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(
            analysis
                .diagnostics()
                .iter()
                .any(|d| matches!(d.kind, DiagnosticKind::InvalidReference { .. })),
            "{source}: {:?}",
            analysis.diagnostics()
        );
    }
    for (source, asynchronous) in [
        (
            "fn wrong(value: Ref<Player>) { val saved = || value; }",
            false,
        ),
        ("async fn wrong(value: RefMut<Player>) -> i32 { 1 }", true),
    ] {
        let analysis = analyze_source(
            &SourceFile::new(
                "capture.kgr",
                format!("struct Player {{ var hp: i32 }} {source}"),
            ),
            catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(
            analysis.diagnostics().iter().any(|d| match d.kind {
                DiagnosticKind::InvalidAsyncCapture { .. } => asynchronous,
                DiagnosticKind::InvalidClosureCapture { .. } => !asynchronous,
                _ => false,
            }),
            "{source}: {:?}",
            analysis.diagnostics()
        );
    }
}
