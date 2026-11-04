//! Representation migration benchmark. See docs/typed-ast-design.md.

mod allocation;
mod layout;
mod memory;
mod workloads;

#[global_allocator]
static ALLOCATOR: allocation::Allocator = allocation::Allocator;

use std::{
    env,
    hint::black_box,
    path::Path,
    time::{Duration, Instant},
};

use kagari_common::cancellation::CancellationToken;
use kagari_hir::lower::lower_module;
use kagari_source::source::SourceFile;
use kagari_syntax::{
    ast::{module::ModuleSyntax, views::ExprView, visit::NodeRef},
    lexer::lex,
    parser::{Parse, ParseLimits, parse, parse_declarations},
};
use serde_json::{Value, json};

use crate::workloads::Workload;

fn parse_source(workload: &Workload, source: &SourceFile) -> Parse {
    if workload.declarations {
        parse_declarations(
            source,
            ParseLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap()
    } else {
        parse(source)
    }
}

fn selected_expressions(module: &ModuleSyntax) -> Vec<ExprView<'_>> {
    NodeRef::ModuleBody(module.body())
        .descendants()
        .filter_map(NodeRef::expression)
        .filter(|expr| {
            matches!(
                expr,
                ExprView::BinaryExpr(_) | ExprView::CallExpr(_) | ExprView::IfExpr(_)
            )
        })
        .collect()
}

// Stable logical field work, independent of raw node kinds/counts. Discovery is
// deliberately outside this microbenchmark; parse_hir includes actual traversal.
fn read_fields(expressions: &[ExprView<'_>]) -> usize {
    expressions
        .iter()
        .map(|expr| match expr {
            ExprView::BinaryExpr(node) => {
                11 + usize::from(node.lhs().is_some())
                    + 2 * usize::from(node.operator().is_some())
                    + 3 * usize::from(node.rhs().is_some())
            }
            ExprView::CallExpr(node) => {
                23 + usize::from(node.callee().is_some()) + 3 * node.args().count()
            }
            ExprView::IfExpr(node) => {
                37 + usize::from(node.condition().is_some())
                    + 2 * usize::from(node.then_branch().is_some())
                    + 3 * usize::from(node.else_branch().is_some())
            }
            _ => unreachable!("selected field workload"),
        })
        .sum()
}

fn verify(workload: &Workload) -> Value {
    let mut diagnostics = 0;
    let mut tokens = 0;
    let mut fields = 0;
    let mut functions = 0;
    for source in &workload.sources {
        let lexical = lex(source.text());
        let mut position = 0;
        for token in &lexical {
            assert_eq!(token.span.start, position, "lexical coverage");
            assert!(
                source
                    .text()
                    .get(token.span.start..token.span.end)
                    .is_some()
            );
            position = token.span.end;
        }
        assert_eq!(position, source.text().len());
        tokens += lexical.len();
        let parsed = parse_source(workload, source);
        assert_eq!(parsed.module().text(), source.text(), "lossless parse");
        diagnostics += parsed.diagnostics().len();
        fields += read_fields(&selected_expressions(&parsed.module()));
        if !workload.erroneous && !workload.declarations {
            functions += lower_module(source).module.functions.len();
        }
    }
    assert_eq!(
        diagnostics > 0,
        workload.erroneous,
        "{}: {diagnostics} diagnostics",
        workload.name
    );
    if let Some(count) = match workload.name {
        "small" => Some(1),
        "medium" | "trivia" => Some(256),
        "large" => Some(4096),
        _ => None,
    } {
        assert_eq!(functions, count);
        assert_eq!(fields, 107 * count);
    }
    json!({"diagnostics": diagnostics, "lexical_tokens": tokens, "field_checksum": fields, "functions": functions})
}

fn batch(
    workload: &Workload,
    stage: &str,
    iterations: usize,
    fields: &[ExprView<'_>],
) -> (f64, f64) {
    let start = Instant::now();
    let mut construct = Duration::ZERO;
    let mut destroy = Duration::ZERO;
    for _ in 0..iterations {
        if stage == "field_read" {
            black_box(read_fields(black_box(fields)));
            continue;
        }
        for source in &workload.sources {
            match stage {
                "lex_drop" => drop(black_box(lex(black_box(source.text())))),
                "parse_drop" => drop(black_box(parse_source(workload, black_box(source)))),
                "parse_hir_drop" => drop(black_box(lower_module(black_box(source)))),
                "parse_and_drop_split" => {
                    let before = Instant::now();
                    let parsed = black_box(parse_source(workload, black_box(source)));
                    let built = Instant::now();
                    drop(parsed);
                    let dropped = Instant::now();
                    construct += built.duration_since(before);
                    destroy += dropped.duration_since(built);
                }
                _ => unreachable!("known stage"),
            }
        }
    }
    if stage == "parse_and_drop_split" {
        // A separate diagnostic timing pass; never derive lifecycle time by subtraction.
        return (
            construct.as_nanos() as f64 / iterations as f64,
            destroy.as_nanos() as f64 / iterations as f64,
        );
    }
    (start.elapsed().as_nanos() as f64 / iterations as f64, 0.0)
}

fn measure(
    workload: &Workload,
    stage: &str,
    fields: &[ExprView<'_>],
    warmups: usize,
    samples: usize,
) -> Value {
    let probe = batch(workload, stage, 1, fields).0.max(1.0);
    // Field reads are short enough to expose process scheduling variation at
    // 20 ms. Freeze longer batches on both images before production cutover.
    let target_ns = if stage == "field_read" {
        100_000_000.0
    } else {
        20_000_000.0
    };
    let iterations = (target_ns / probe).ceil().clamp(1.0, 100_000.0) as usize;
    for _ in 0..warmups {
        black_box(batch(workload, stage, iterations, fields));
    }
    let timings = (0..samples)
        .map(|_| batch(workload, stage, iterations, fields))
        .collect::<Vec<_>>();
    let construct = timings.iter().map(|time| time.0).collect::<Vec<_>>();
    let drop_times = (stage == "parse_and_drop_split")
        .then(|| timings.iter().map(|time| time.1).collect::<Vec<_>>());
    json!({"stage": stage, "iterations": iterations, "ns_per_workload": construct, "drop_ns_per_workload": drop_times})
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    assert!(
        args.iter().all(|arg| matches!(
            arg.as_str(),
            "--check" | "--memory" | "--inputs" | "--layout"
        )),
        "usage: ast_baseline [--check | --memory | --inputs | --layout]"
    );
    let check_only = args.iter().any(|arg| arg == "--check");
    if args.iter().any(|arg| arg == "--layout") {
        println!("{}", layout::report());
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workloads = workloads::load(&root);
    if args.iter().any(|arg| arg == "--inputs") {
        let rows = workloads
            .iter()
            .chain([&workloads::inline_retention()])
            .map(|workload| {
                json!({"name": workload.name, "blake3": workload.hash(),
                "sources": workload.sources.iter().map(|source| source.text()).collect::<Vec<_>>()})
            })
            .collect::<Vec<_>>();
        println!("{}", json!(rows));
        return;
    }
    if args.iter().any(|arg| arg == "--memory") {
        println!(
            "{}",
            memory::report(&workloads, &workloads::inline_retention())
        );
        return;
    }
    let mut rows = Vec::new();
    for workload in &workloads {
        let evidence = verify(workload);
        let mut stages = Vec::new();
        if !check_only {
            for stage in ["lex_drop", "parse_drop", "parse_and_drop_split"] {
                stages.push(measure(workload, stage, &[], 3, 11));
            }
            if !workload.erroneous && !workload.declarations {
                stages.push(measure(workload, "parse_hir_drop", &[], 3, 11));
                let modules = workload
                    .sources
                    .iter()
                    .map(|source| parse_source(workload, source).module())
                    .collect::<Vec<_>>();
                let fields = modules
                    .iter()
                    .flat_map(selected_expressions)
                    .collect::<Vec<_>>();
                if !fields.is_empty() {
                    stages.push(measure(workload, "field_read", &fields, 3, 11));
                }
            }
            assert_eq!(verify(workload), evidence, "post-measurement correctness");
        }
        rows.push(json!({"name": workload.name, "files": workload.sources.len(), "bytes": workload.bytes(), "blake3": workload.hash(), "evidence": evidence, "stages": stages}));
    }
    println!(
        "{}",
        json!({"schema": 1, "warmups": 3, "samples": 11, "check_only": check_only, "workloads": rows})
    );
}
