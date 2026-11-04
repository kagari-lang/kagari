//! Separate lifecycle and independently retained inline-module allocation probes.

use kagari_syntax::ast::{item::Item, module::ModuleSyntax};
use serde_json::{Value, json};

use crate::{allocation, parse_source, workloads::Workload};

pub fn measure(workload: &Workload, inline: bool) -> Value {
    // Preallocate probe infrastructure so only parser-owned allocations are counted.
    let mut parses = Vec::with_capacity(workload.sources.len());
    allocation::begin();
    for source in &workload.sources {
        parses.push(parse_source(workload, source));
    }
    let parsed = allocation::snapshot();
    let view = inline.then(|| {
        let root = parses[0].syntax();
        let Some(Item::ModuleDef(module)) = root.items().next() else {
            panic!("inline workload starts with a module");
        };
        ModuleSyntax::Inline(module.block().expect("inline body"))
    });
    let retained_view = allocation::snapshot();
    parses.clear();
    let released_parse = allocation::snapshot();
    if let Some(view) = &view {
        assert_eq!(view.items().count(), 4);
        assert!(view.content_range().is_some());
    }
    drop(view);
    let released = allocation::finish();
    assert_eq!(
        released.live_bytes, 0,
        "all parse/view allocations released"
    );
    json!({
        "name": workload.name, "bytes": workload.bytes(), "blake3": workload.hash(),
        "parsed": parsed, "with_view": retained_view,
        "parse_released": released_parse, "released": released,
    })
}

pub fn report(workloads: &[Workload], inline: &Workload) -> Value {
    // Warm TLS, allocator, parser and JSON infrastructure before diagnostic windows.
    for workload in workloads {
        drop(measure(workload, false));
    }
    drop(measure(inline, true));
    let mut rows = workloads
        .iter()
        .map(|workload| measure(workload, false))
        .collect::<Vec<_>>();
    rows.push(measure(inline, true));
    // Repeat complete lifecycles; retained bytes must remain identical, with zero net growth.
    for _ in 0..3 {
        for (workload, reference) in workloads.iter().chain([inline]).zip(&rows) {
            assert_eq!(
                &measure(workload, workload.name == "inline_retention"),
                reference
            );
        }
    }
    json!({"schema": 1, "diagnostic_only": true, "lifecycles": 4, "workloads": rows})
}
