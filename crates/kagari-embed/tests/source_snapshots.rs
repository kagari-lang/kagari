use std::sync::Arc;

use kagari_common::{SourceFile, source_database::SourceLayer};
use kagari_embed::{ArtifactOptions, CompileOptions, EmbeddingError, KagariEngine};
use kagari_hir::analysis::CancellationToken;
use kagari_runtime::LanguageProfile;

#[test]
fn module_rebinding_changes_analysis_and_artifacts_without_changing_text() {
    use kagari_common::identity::{ModuleIdentity, PackageId};
    let engine = KagariEngine::default();
    let first = ModuleIdentity {
        package: PackageId("a".into()),
        path: vec!["main".into()],
    };
    let second = ModuleIdentity {
        package: PackageId("b".into()),
        path: vec!["main".into()],
    };
    let name = "memory://identity.kgr";
    let token = CancellationToken::default();
    engine.bind_module(name, first.clone()).unwrap();
    let id = engine
        .set_source(name, "fn main() -> i32 { 42 }".into(), SourceLayer::Base)
        .unwrap();
    let old_source = engine.source_snapshot();
    let old = engine
        .compile_snapshot(old_source.clone(), id, Default::default(), &token)
        .unwrap();
    engine.bind_module(name, second.clone()).unwrap();
    let current_source = engine.source_snapshot();
    let new = engine
        .compile_snapshot(current_source.clone(), id, Default::default(), &token)
        .unwrap();
    assert_eq!(old.module_identity(), &first);
    assert_eq!(new.module_identity(), &second);
    let artifact = engine.emit_bytecode(&new, Default::default()).unwrap();
    assert_eq!(
        artifact.program.modules[artifact.program.root.index()].identity,
        second
    );
    assert_eq!(
        artifact.header.module_identity,
        artifact.program.modules[artifact.program.root.index()].identity
    );
    let old_again = engine
        .compile_snapshot(old_source, id, Default::default(), &token)
        .unwrap();
    assert_eq!(old_again.module_identity(), &first);
    let current_again = engine
        .analyze(current_source, Default::default(), &token)
        .unwrap();
    assert_eq!(
        current_again
            .file(id)
            .unwrap()
            .result()
            .facts()
            .lowered
            .source
            .module_identity(),
        &second
    );
    assert_eq!(
        current_again
            .file(id)
            .unwrap()
            .result()
            .facts()
            .typed
            .reused_bodies,
        0
    );
}

#[test]
fn reused_signature_and_body_facts_emit_the_same_artifact_as_fresh_analysis() {
    let engine = KagariEngine::default();
    let token = CancellationToken::default();
    let unchanged = "fn echo<T>(value: T) -> T { value } struct P { var n: i32 } fn b() -> i32 { val p: P = P { n: echo(a()) }; p.n += 1; val xs: MutableArray<i32> = [p.n]; xs.push(2); match 2147483647 { 2147483647 => -2147483648, _ => xs[0] } }";
    let id = engine
        .set_source(
            "memory://reuse.kgr",
            format!("fn a() -> i32 {{ 1 }} {unchanged}"),
            SourceLayer::Base,
        )
        .unwrap();
    engine
        .analyze(engine.source_snapshot(), Default::default(), &token)
        .unwrap();
    let edited = format!(
        "fn a() -> i32 {{ val shifted: (i32, MutableArray<i32>) = (10, [20]); shifted[0] + shifted[1][0] + 30 }} {unchanged}"
    );
    engine
        .set_source("memory://reuse.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let snapshot = engine.source_snapshot();
    let analysis = engine
        .analyze(snapshot.clone(), Default::default(), &token)
        .unwrap();
    assert!(analysis.file(id).unwrap().signatures_reused());
    assert_eq!(
        analysis
            .file(id)
            .unwrap()
            .result()
            .facts()
            .typed
            .reused_bodies,
        2
    );
    let reused = engine
        .compile_snapshot(snapshot, id, Default::default(), &token)
        .unwrap();
    let fresh_engine = KagariEngine::default();
    let fresh = fresh_engine
        .compile_source(
            SourceFile::new("memory://reuse.kgr", edited),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        engine
            .emit_bytecode(&reused, Default::default())
            .unwrap()
            .to_bytes()
            .unwrap(),
        fresh_engine
            .emit_bytecode(&fresh, Default::default())
            .unwrap()
            .to_bytes()
            .unwrap()
    );
}

#[test]
fn compilation_and_tools_share_overlay_revision_and_profile() {
    let engine = KagariEngine::default();
    let token = CancellationToken::default();
    let id = engine
        .set_source(
            "memory://main.kgr",
            "fn main() -> i32 { 1 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let original = engine.source_snapshot();
    engine
        .set_source(
            "memory://main.kgr",
            "fn main() -> i32 { 2 }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let edited = engine.source_snapshot();
    let current = engine
        .analyze(edited.clone(), LanguageProfile::default(), &token)
        .unwrap();
    let old = engine
        .analyze(original.clone(), LanguageProfile::default(), &token)
        .unwrap();
    assert!(old.revision() < current.revision());
    let again = engine
        .analyze(edited, LanguageProfile::default(), &token)
        .unwrap();
    assert!(
        Arc::ptr_eq(current.file(id).unwrap(), again.file(id).unwrap()),
        "an older query must not replace the current cache"
    );

    // Supplying new base text cannot silently bypass an editor's overlay.
    let checked = engine
        .compile_source(
            SourceFile::new("memory://main.kgr", "fn main() -> i32 { 3 }"),
            CompileOptions::default(),
        )
        .unwrap();
    let artifact = engine
        .emit_bytecode(&checked, ArtifactOptions::default())
        .unwrap();
    let from_snapshot = engine
        .compile_snapshot(
            engine.source_snapshot(),
            id,
            CompileOptions::default(),
            &token,
        )
        .unwrap();
    assert_eq!(
        artifact.to_bytes().unwrap(),
        engine
            .emit_bytecode(&from_snapshot, ArtifactOptions::default())
            .unwrap()
            .to_bytes()
            .unwrap()
    );
    assert!(
        engine
            .source_snapshot()
            .file(id)
            .unwrap()
            .text()
            .contains("{ 2 }")
    );
    engine.close_overlay("memory://main.kgr").unwrap();
    assert!(
        engine
            .source_snapshot()
            .file(id)
            .unwrap()
            .text()
            .contains("{ 3 }")
    );
    assert!(original.file(id).unwrap().text().contains("{ 1 }"));

    token.cancel();
    assert!(matches!(
        engine.compile_snapshot(original, id, CompileOptions::default(), &token),
        Err(EmbeddingError::Cancelled)
    ));
}

#[test]
fn profile_changes_do_not_reuse_a_previously_accepted_result() {
    let engine = KagariEngine::default();
    let id = engine
        .set_source(
            "memory://reflect.kgr",
            "fn main() { type_of(1); }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let token = CancellationToken::default();
    let permissive = LanguageProfile {
        allow_reflection: true,
        ..LanguageProfile::default()
    };
    let allowed = engine
        .analyze(engine.source_snapshot(), permissive, &token)
        .unwrap();
    let restricted = engine
        .analyze(engine.source_snapshot(), LanguageProfile::default(), &token)
        .unwrap();
    assert!(!Arc::ptr_eq(
        allowed.file(id).unwrap(),
        restricted.file(id).unwrap()
    ));
    assert!(
        restricted.file(id).unwrap().result().diagnostics().len()
            > allowed.file(id).unwrap().result().diagnostics().len()
    );
}

#[test]
fn diagnostics_carry_the_source_revision_that_produced_them() {
    let engine = KagariEngine::default();
    let id = engine
        .set_source(
            "memory://bad.kgr",
            "fn main() { missing; }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = engine.source_snapshot();
    let Err(EmbeddingError::Diagnostics { diagnostics }) = engine.compile_snapshot(
        snapshot.clone(),
        id,
        CompileOptions::default(),
        &CancellationToken::default(),
    ) else {
        panic!("expected source diagnostics")
    };
    let span = diagnostics
        .iter()
        .find_map(|diagnostic| diagnostic.span)
        .unwrap();
    assert!(snapshot.contains(span));
    engine
        .set_source(
            "memory://bad.kgr",
            "fn main() {}".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    assert!(!engine.source_snapshot().contains(span));
}

#[test]
fn parser_budget_reports_revision_owned_limits_before_codegen() {
    let engine = KagariEngine::default();
    engine.set_parse_limits(kagari_embed::ParseLimits {
        max_diagnostics: 0,
        ..Default::default()
    });
    let id = engine
        .set_source(
            "memory://limited.kgr",
            "fn main() -> i32 { 42 } @ fn hidden() {}".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let source = engine.source_snapshot();
    let Err(EmbeddingError::Diagnostics { diagnostics }) =
        engine.compile_snapshot(source.clone(), id, Default::default(), &Default::default())
    else {
        panic!("limited source must not reach code generation");
    };
    let limit = diagnostics
        .iter()
        .find(|d| d.code == "KG_COMPILE_LIMIT_EXCEEDED")
        .expect("structured parser limit");
    assert!(source.contains(limit.span.expect("revision-owned position")));
    let facts = engine
        .analyze(source, LanguageProfile::default(), &Default::default())
        .unwrap();
    assert!(
        facts
            .file(id)
            .unwrap()
            .result()
            .facts()
            .declarations
            .iter()
            .any(|d| d.name == "main")
    );
    engine
        .compile_source(
            SourceFile::new("memory://valid.kgr", "fn main() -> i32 { 42 }"),
            Default::default(),
        )
        .unwrap();
}

#[test]
fn nesting_budget_changes_invalidate_same_revision_analysis() {
    let engine = KagariEngine::default();
    let id = engine
        .set_source(
            "memory://nested.kgr",
            "fn main() -> i32 { (((42))) }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let source = engine.source_snapshot();
    let before = engine
        .analyze(
            source.clone(),
            LanguageProfile::default(),
            &Default::default(),
        )
        .unwrap();
    assert!(before.check_program(id, &Default::default()).is_ok());
    engine.set_parse_limits(kagari_embed::ParseLimits {
        max_nesting: 3,
        ..Default::default()
    });
    let Err(EmbeddingError::Diagnostics { diagnostics }) =
        engine.compile_snapshot(source.clone(), id, Default::default(), &Default::default())
    else {
        panic!("excessive nesting must be rejected before code generation");
    };
    let limit = diagnostics
        .iter()
        .find(|d| d.code == "KG_COMPILE_LIMIT_EXCEEDED")
        .unwrap();
    assert!(source.contains(limit.span.unwrap()));
    assert!(before.check_program(id, &Default::default()).is_ok());
    engine.set_parse_limits(Default::default());
    engine
        .compile_snapshot(source, id, Default::default(), &Default::default())
        .unwrap();
}

#[test]
fn deep_iterative_source_is_rejected_with_queryable_prefix() {
    for max_tree_depth in [24, kagari_embed::ParseLimits::default().max_tree_depth] {
        let engine = KagariEngine::default();
        engine.set_parse_limits(kagari_embed::ParseLimits {
            max_tree_depth,
            ..Default::default()
        });
        let id = engine
            .set_source(
                "memory://chain.kgr",
                format!(
                    "fn good() -> i32 {{ 42 }} fn main() -> i32 {{ 1{} }}",
                    " + 1".repeat(2_000)
                ),
                SourceLayer::Base,
            )
            .unwrap();
        let source = engine.source_snapshot();
        let analysis = engine
            .analyze(
                source.clone(),
                LanguageProfile::default(),
                &Default::default(),
            )
            .unwrap();
        assert!(
            analysis
                .file(id)
                .unwrap()
                .result()
                .facts()
                .declarations
                .iter()
                .any(|d| d.name == "good")
        );
        let Err(EmbeddingError::Diagnostics { diagnostics }) =
            engine.compile_snapshot(source.clone(), id, Default::default(), &Default::default())
        else {
            panic!("deep source reached codegen")
        };
        let limit = diagnostics
            .iter()
            .find(|d| d.code == "KG_COMPILE_LIMIT_EXCEEDED")
            .unwrap();
        assert!(source.contains(limit.span.unwrap()));
    }
}

#[test]
fn default_parser_limits_retain_queryable_facts_across_recursive_syntax() {
    let depth = 2_000;
    let cases = [
        format!(
            "fn main() {{ {}1{} }}",
            "(".repeat(depth),
            ")".repeat(depth)
        ),
        format!("fn main() {{ {}true }}", "!".repeat(depth)),
        format!(
            "fn main() {{ {}1{} }}",
            "[".repeat(depth),
            "]".repeat(depth)
        ),
        format!(
            "fn main(x: {}i32{}) {{}}",
            "[".repeat(depth),
            "]".repeat(depth)
        ),
        format!(
            "fn main(x: {}i32{}) {{}}",
            "Box<".repeat(depth),
            ">".repeat(depth)
        ),
        format!(
            "{} fn inner() {{}} {}",
            "mod nested {".repeat(depth),
            "}".repeat(depth)
        ),
        format!(
            "use {}item{};",
            "nested::{".repeat(depth),
            "}".repeat(depth)
        ),
        format!(
            "fn main() {{ {}1{} }}",
            "if true {".repeat(depth),
            "}".repeat(depth)
        ),
        format!("fn main() {{ {} 1 }}", "if true {} else ".repeat(depth)),
        format!(
            "fn main() {{ match 1 {{ {}_{} => 1 }} }}",
            "(".repeat(depth),
            ",)".repeat(depth)
        ),
    ];
    for (index, suffix) in cases.into_iter().enumerate() {
        let engine = KagariEngine::default();
        let text = format!("fn good(value: i32) -> i32 {{ value }} {suffix}");
        let id = engine
            .set_source("memory://deep.kgr", text, SourceLayer::Base)
            .unwrap();
        let source = engine.source_snapshot();
        let analysis = engine
            .analyze(
                source.clone(),
                LanguageProfile::default(),
                &Default::default(),
            )
            .unwrap();
        let file = analysis.file(id).unwrap();
        assert_eq!(
            file.type_at("fn good(value: i32) -> i32 { ".len()),
            Some(kagari_hir::types::TypeId::Builtin(
                kagari_hir::types::BuiltinType::I32
            )),
            "case {index}: preceding function body remains typed",
        );
        assert!(
            file.result()
                .facts()
                .declarations
                .iter()
                .any(|d| d.name == "good"),
            "case {index}"
        );
        assert!(
            file.result().diagnostics().iter().any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::CompileLimitExceeded { .. }
            )),
            "case {index}"
        );
        assert!(
            analysis.check_program(id, &Default::default()).is_err(),
            "case {index}"
        );
        assert!(
            matches!(
                engine.compile_snapshot(source, id, Default::default(), &Default::default()),
                Err(EmbeddingError::Diagnostics { .. })
            ),
            "case {index}"
        );
    }
}

#[test]
fn const_budgets_share_validation_and_evaluation_and_invalidate_cached_results() {
    use kagari_embed::ConstLimits;
    let engine = KagariEngine::default();
    let id = engine
        .set_source(
            "memory://const-budget.kgr",
            "const ANSWER: i32 = 42; fn main() -> i32 { ANSWER }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let source = engine.source_snapshot();
    engine.set_const_limits(ConstLimits {
        max_steps: 2,
        max_depth: 1,
    });
    let complete = engine
        .analyze(source.clone(), Default::default(), &Default::default())
        .unwrap();
    assert!(complete.check_program(id, &Default::default()).is_ok());
    let declaration = complete
        .file(id)
        .unwrap()
        .result()
        .facts()
        .declarations
        .iter()
        .find(|d| d.name == "main")
        .unwrap();
    let kagari_hir::declarations::DeclarationId::Definition(owner) = &declaration.id else {
        panic!("function definition")
    };
    let old_body = engine
        .body(source.clone(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert!(old_body.diagnostics().is_empty());
    engine.set_const_limits(ConstLimits {
        max_steps: 1,
        max_depth: 1,
    });
    let limited = engine
        .analyze(source.clone(), Default::default(), &Default::default())
        .unwrap();
    let new_body = engine
        .body(source.clone(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert!(!Arc::ptr_eq(&old_body, &new_body));
    assert!(old_body.diagnostics().is_empty());
    assert!(new_body.diagnostics().iter().any(|d| matches!(
        d.kind,
        kagari_common::DiagnosticKind::CompileLimitExceeded { .. }
    )));
    let diagnostics = limited.file(id).unwrap().result().diagnostics();
    assert_eq!(
        diagnostics
            .iter()
            .filter(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::CompileLimitExceeded {
                    resource: "const steps",
                    limit: 1
                }
            ))
            .count(),
        1
    );
    assert!(
        limited
            .file(id)
            .unwrap()
            .result()
            .facts()
            .typed
            .const_values
            .is_empty()
    );
    assert!(limited.check_program(id, &Default::default()).is_err());
    assert!(complete.check_program(id, &Default::default()).is_ok());
    let Err(EmbeddingError::Diagnostics { diagnostics }) =
        engine.compile_snapshot(source.clone(), id, Default::default(), &Default::default())
    else {
        panic!("limited const reached codegen")
    };
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "KG_COMPILE_LIMIT_EXCEEDED" && source.contains(d.span.unwrap()))
    );
    engine.set_const_limits(ConstLimits {
        max_steps: 2,
        max_depth: 1,
    });
    engine
        .compile_snapshot(source, id, Default::default(), &Default::default())
        .unwrap();
}

#[test]
fn const_budget_counts_short_circuit_work_and_rejects_deep_dependencies() {
    use kagari_embed::ConstLimits;
    let engine = KagariEngine::default();
    engine.set_const_limits(ConstLimits {
        max_steps: 5,
        max_depth: 2,
    });
    engine
        .compile_source(
            SourceFile::new(
                "memory://short.kgr",
                "const VALUE: bool = false && true; fn main() -> bool { VALUE }",
            ),
            Default::default(),
        )
        .unwrap();
    assert!(
        engine
            .compile_source(
                SourceFile::new(
                    "memory://full.kgr",
                    "const VALUE: bool = true && true; fn main() -> bool { VALUE }"
                ),
                Default::default()
            )
            .is_err()
    );
    let engine = KagariEngine::default();
    let mut text = String::from("fn good(value: i32) -> i32 { value } ");
    for i in 0..1_000 {
        text.push_str(&format!("const C{i}: i32 = C{}; ", i + 1));
    }
    text.push_str("const C1000: i32 = 42;");
    let id = engine
        .set_source("memory://dependencies.kgr", text, SourceLayer::Base)
        .unwrap();
    let analysis = engine
        .analyze(
            engine.source_snapshot(),
            Default::default(),
            &Default::default(),
        )
        .unwrap();
    let file = analysis.file(id).unwrap();
    assert!(file.result().diagnostics().iter().any(|d| matches!(
        d.kind,
        kagari_common::DiagnosticKind::CompileLimitExceeded {
            resource: "const depth",
            limit: 64
        }
    )));
    assert_eq!(
        file.type_at("fn good(value: i32) -> i32 { ".len()),
        Some(kagari_hir::types::TypeId::Builtin(
            kagari_hir::types::BuiltinType::I32
        ))
    );
    assert!(analysis.check_program(id, &Default::default()).is_err());
}

#[test]
fn zero_const_budget_accepts_no_consts_and_cancellation_remains_distinct() {
    let engine = KagariEngine::default();
    engine.set_const_limits(kagari_embed::ConstLimits {
        max_steps: 0,
        max_depth: 0,
    });
    engine
        .compile_source(
            SourceFile::new("memory://empty-const.kgr", "fn main() -> i32 { 42 }"),
            Default::default(),
        )
        .unwrap();
    let id = engine
        .set_source(
            "memory://one-const.kgr",
            "const VALUE: i32 = 42;".into(),
            SourceLayer::Base,
        )
        .unwrap();
    assert!(matches!(
        engine.compile_snapshot(
            engine.source_snapshot(),
            id,
            Default::default(),
            &Default::default()
        ),
        Err(EmbeddingError::Diagnostics { .. })
    ));
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        engine.analyze(engine.source_snapshot(), Default::default(), &cancel),
        Err(EmbeddingError::Cancelled)
    ));
}

#[test]
fn invalid_const_types_cannot_bypass_validation_budget() {
    use kagari_common::DiagnosticKind;
    for max_steps in [0, 1, 3] {
        let engine = KagariEngine::default();
        engine.set_const_limits(kagari_embed::ConstLimits {
            max_steps,
            max_depth: 64,
        });
        let mut text = String::from("fn good(value: i32) -> i32 { value } ");
        for index in 0..100 {
            text.push_str(&format!("const C{index}: [i32] = [1]; "));
        }
        let id = engine
            .set_source(
                "memory://invalid-consts.kgr",
                text.clone(),
                SourceLayer::Base,
            )
            .unwrap();
        let analysis = engine
            .analyze(
                engine.source_snapshot(),
                Default::default(),
                &Default::default(),
            )
            .unwrap();
        let file = analysis.file(id).unwrap();
        let diagnostics = file.result().diagnostics();
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| matches!(d.kind, DiagnosticKind::InvalidConstInitializer { .. }))
                .count(),
            max_steps
        );
        let limits: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                matches!(
                    d.kind,
                    DiagnosticKind::CompileLimitExceeded {
                        resource: "const steps",
                        ..
                    }
                )
            })
            .collect();
        assert_eq!(limits.len(), 1);
        let span = limits[0].span.unwrap();
        assert_eq!(&text[span.start..span.end], "[1]");
        assert!(span.start > text.find(&format!("const C{max_steps}:")).unwrap());
        assert_eq!(
            file.type_at("fn good(value: i32) -> i32 { ".len()),
            Some(kagari_hir::types::TypeId::Builtin(
                kagari_hir::types::BuiltinType::I32
            ))
        );
        assert!(analysis.check_program(id, &Default::default()).is_err());
    }
}
