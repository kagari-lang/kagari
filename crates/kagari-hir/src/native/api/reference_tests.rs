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

    let analysis = analyze_source(
        &SourceFile::new(
            "fields.kgr",
            r#"
        struct Stats { var hp: i32 }
        struct Player { var stats: Stats, val locked: Stats, var name: String }
        fn inspect(stats: Ref<Stats>) -> i32 { stats.hp }
        fn heal(stats: RefMut<Stats>) -> i32 { stats.hp += 1; stats.hp }
        fn fields(player: RefMut<Player>) -> i32 {
            val current = player.stats;
            player.stats = Stats { hp: 5 };
            player.name = "changed";
            heal(current) + inspect(player.locked)
        }
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
    for source in [
        "fn reject(p: Ref<Player>) { p.stats.hp = 1; }",
        "fn reject(p: RefMut<Player>) -> Stats { p.stats }",
        "fn reject(p: RefMut<Player>) -> RefMut<Stats> { p.stats }",
        "fn reject(p: RefMut<Player>) { val saved = [p.stats]; }",
        "fn reject(p: RefMut<Player>) { val saved = || p.stats.hp; }",
        "fn reject(p: RefMut<Player>) -> i32 { heal(p.locked) }",
        "fn reject(p: RefMut<Player>) { val saved = p.values; }",
    ] {
        let analysis = analyze_source(
            &SourceFile::new(
                "field-reject.kgr",
                format!(
                    r#"
            struct Stats {{ var hp: i32 }}
            struct Player {{ var stats: Stats, val locked: Stats, var values: [i32] }}
            fn heal(stats: RefMut<Stats>) -> i32 {{ 1 }}
            {source}
        "#
                ),
            ),
            catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(
            analysis.diagnostics().iter().any(|d| matches!(
                d.kind,
                DiagnosticKind::InvalidAssignmentTarget { .. }
                    | DiagnosticKind::ArgumentTypeMismatch { .. }
                    | DiagnosticKind::ReturnTypeMismatch { .. }
                    | DiagnosticKind::InvalidReference { .. }
                    | DiagnosticKind::InvalidClosureCapture { .. }
            )),
            "{source}: {:?}",
            analysis.diagnostics()
        );
    }

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

#[test]
fn scoped_reference_applications_reject_returned_views_at_the_call() {
    for (declaration, call) in [
        ("fn identity<T>(value: T) -> T { value }", "identity(view)"),
        (
            "impl Player { fn identity<T>(self, value: T) -> T { value } }",
            "owned.identity(view)",
        ),
        (
            "trait Echo { fn echo<T>(self, value: T) -> T; } impl Echo for Player { fn echo<T>(self, value: T) -> T { value } }",
            "owned.echo(view)",
        ),
    ] {
        let source = format!(
            "struct Player {{ var hp: i32 }} {declaration} fn probe(view: Ref<Player>, owned: Player) -> i32 {{ {call}.hp }}"
        );
        let analysis = analyze_source(
            &SourceFile::new("applied-reference.kgr", source.clone()),
            catalog::shared().unwrap(),
        )
        .unwrap();
        let diagnostics = analysis.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:?}");
        assert!(matches!(
            diagnostics[0].kind,
            DiagnosticKind::InvalidReference {
                reason: "a scoped view cannot be returned"
            }
        ));
        let span = diagnostics[0].span.unwrap();
        assert_eq!(span.start, source.rfind(call).unwrap());
        assert_eq!(span.end, span.start + call.len());
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn scoped_reference_applications_check_parameter_targets_for_diverging_arguments() {
    let source = "fn inspect<T>(value: Ref<T>) {} fn probe() { inspect::<i32>(loop {}); }";
    let analysis = analyze_source(
        &SourceFile::new("applied-reference-target.kgr", source),
        catalog::shared().unwrap(),
    )
    .unwrap();
    let diagnostics = analysis.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(matches!(
        diagnostics[0].kind,
        DiagnosticKind::InvalidReference { .. }
    ));
    let span = diagnostics[0].span.unwrap();
    assert_eq!(&source[span.start..span.end], "inspect::<i32>(loop {})");
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn scoped_reference_applications_preserve_inputs_and_callable_values() {
    let analysis = analyze_source(
        &SourceFile::new(
            "applied-reference-inputs.kgr",
            r#"
        struct Player { var hp: i32 }
        fn identity<T>(value: T) -> T { value }
        fn discard<T>(value: T) {}
        fn invoke<T>(callback: fn(T) -> i32, value: T) -> i32 { callback(value) }
        fn callback() -> fn(Ref<Player>) -> i32 {
            identity(|value: Ref<Player>| value.hp)
        }
        fn probe(value: RefMut<Player>) -> i32 {
            discard(value);
            val inspect: fn(Ref<Player>) -> i32 = |view| view.hp;
            val saved = identity(inspect);
            saved(value) + invoke(inspect, value) + callback()(value)
        }
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
    assert!(analysis.into_codegen().is_ok());
}
