//! Observable language tests shared by source, portable artifact and JIT routes.
//! Expectations describe language results, never instruction or arena layouts.
use kagari_common::SourceFile;
use kagari_ir::bytecode::{ArtifactBuildOptions, ArtifactCompatibility, KbcArtifact};
use kagari_jit_cranelift::CraneliftBackend;
use kagari_runtime::{
    CapabilitySet, LanguageProfile, Runtime, RuntimeConfig, SecurityContext, value::Value,
};

use crate::{Vm, tests::common::compile_test_bytecode};

#[derive(Clone, Copy, Debug)]
enum Route {
    Source,
    Artifact,
    Jit,
}

struct Case {
    name: &'static str,
    source: &'static str,
    expected: Value,
}

fn run(case: &Case, route: Route) {
    let compiled = compile_test_bytecode(case.source);
    let module = match route {
        Route::Source | Route::Jit => compiled,
        Route::Artifact => {
            let bytes = KbcArtifact::from_module(compiled, ArtifactBuildOptions::default())
                .to_bytes()
                .unwrap();
            let decoded = KbcArtifact::from_bytes(&bytes).unwrap();
            decoded
                .validate_for_loader(&ArtifactCompatibility::default())
                .unwrap();
            decoded.module
        }
    };
    let mut runtime = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_jit: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                jit: true,
                ..Default::default()
            },
        },
        ..Default::default()
    });
    let loaded = runtime.load_module(case.name, module).unwrap();
    let mut vm = Vm::new(runtime);
    let report = match route {
        Route::Jit => {
            vm.execute_with_backend(&loaded, "main", &mut CraneliftBackend::for_host().unwrap())
        }
        _ => vm.execute(&loaded, "main"),
    }
    .unwrap_or_else(|error| panic!("{} ({route:?}): {error:?}", case.name));
    assert_eq!(
        report.return_value, case.expected,
        "{} ({route:?})",
        case.name
    );
}

#[test]
fn language_contract_routes_preserve_values_and_aliases() {
    let cases = [
        Case {
            name: "scalar",
            source: "fn main() -> i32 { (2 + 3) * 4 }",
            expected: Value::I32(20),
        },
        Case {
            name: "alias",
            source: "struct P { var n: i32 } fn main() -> i32 { val a = P { n: 1 }; val b = a; b.n = 7; a.n }",
            expected: Value::I32(7),
        },
        Case {
            name: "object_identity",
            source: "struct P { var n: i32 } fn main() -> bool { val a = P { n: 1 }; val b = P { n: 1 }; a == b }",
            expected: Value::Bool(false),
        },
        Case {
            name: "short_circuit",
            source: "fn fail() -> bool { val a = [1]; a[2] == 0 } fn main() -> bool { false && fail() }",
            expected: Value::Bool(false),
        },
    ];
    for case in &cases {
        for route in [Route::Source, Route::Artifact, Route::Jit] {
            run(case, route);
        }
    }
}

#[test]
fn language_contract_rejects_const_rebinding() {
    let source = SourceFile::new("const_write.kgr", "const N: i32 = 1; fn main() { N = 2; }");
    let ast = kagari_syntax::parse_module(&source).unwrap();
    let diagnostics = kagari_hir::analyze_module(&ast).unwrap_err();
    assert!(
        diagnostics.iter().any(|d| matches!(d.kind,
        kagari_common::DiagnosticKind::InvalidAssignmentTarget { ref reason }
        if reason == "`const` item cannot be reassigned")),
        "{diagnostics:?}"
    );
}
