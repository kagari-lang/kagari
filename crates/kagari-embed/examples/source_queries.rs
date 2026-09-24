//! Run with `cargo run -p kagari-embed --example source_queries`.
use kagari_common::{
    identity::{ModuleIdentity, PackageId},
    line_index::PositionEncoding,
    source_database::SourceLayer,
};
use kagari_embed::KagariEngine;

fn main() -> kagari_embed::CompileResult<()> {
    let engine = KagariEngine::default();
    // Bound parser recovery for incomplete editor input. A limit error prevents
    // compilation while retaining the parsed prefix for navigation.
    engine.set_parse_limits(kagari_embed::ParseLimits {
        max_diagnostics: 64,
        max_nesting: 32,
        max_tree_depth: 96,
    });
    engine.set_const_limits(kagari_embed::ConstLimits {
        max_steps: 10_000,
        max_depth: 32,
    });
    let source_name = "editor://game/main.kgr";
    engine.bind_module(
        source_name,
        ModuleIdentity {
            package: PackageId("game".into()),
            path: vec!["main".into()],
        },
    )?;
    // An erroneous neighbor does not prevent navigation in the correct function.
    let text = "struct Point { var x: i32 }\r\nfn bad() { missing() }\r\nfn good(value: i32) -> i32 { val answer = value + 1; answer }\r\nfn read(p: Point) -> i32 { p.x }\r\ntrait Show { fn show(self) -> i32; }\r\nfn inspect<T: Show>(value: T) -> i32 { value.show() }\r\nenum Mode { Ready, Running(Point, [String]) }\r\nfn mode(p: Point) -> Mode { Mode::Running(p, [\"active\"]) }";
    let text = &format!(
        "{text}\r\nfn kind(p: Point) -> String {{ type_of(p) }}\r\nimpl Show for Point {{ fn show(self) -> i32 {{ self.x }} }}\r\nfn broken_target(p: Point) {{ missing[p.x] = 1; p.x = 2; }}\r\nfn readonly_target(p: Point) {{ p = Point {{ x: 2 }}; }}\r\nfn broken_index(items: [Point]) {{ items[true].x; items[true].x = 1; }}"
    );
    let file = engine.set_source(source_name, text.into(), SourceLayer::Overlay)?;
    // Declaration discovery does not resolve bodies or evaluate constants.
    let headers = engine.declarations(engine.source_snapshot(), &Default::default())?;
    let header = headers.file(file).expect("source declarations");
    assert!(header.diagnostics().is_empty());
    let variant = header
        .member_at(text.find("Ready").expect("variant declaration"))
        .expect("variant source identity");
    println!("variant {} -> {:?}", variant.name, variant.id);
    println!(
        "{} named declarations before body analysis",
        header.declarations().iter().count()
    );
    println!(
        "module name lookup: {:?}",
        header.names().items.lookup("Point")
    );
    let signatures = engine.signatures(engine.source_snapshot(), &Default::default())?;
    let signature = signatures.file(file).expect("source signatures");
    assert!(signature.diagnostics().is_empty());
    let inspect = signature
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "inspect")
        .expect("generic signature");
    println!(
        "checked generic bounds before body analysis: {:?}",
        inspect.bounds
    );
    assert_eq!(inspect.bounds[&inspect.generic_params[0]].len(), 1);
    println!(
        "enum payload type before body analysis: {:?}",
        signature.type_at(text.find("Running(Point").expect("payload") + "Running(".len())
    );
    println!(
        "{} function signatures before body analysis",
        signature.signatures().facts().functions().len()
    );
    let good = header
        .declarations()
        .iter()
        .find(|d| d.name == "good")
        .expect("good declaration");
    let kagari_hir::declarations::DeclarationId::Definition(good) = &good.id else {
        panic!("function has a definition identity");
    };
    let body = engine
        .body(engine.source_snapshot(), good, &Default::default())?
        .expect("function body");
    assert_eq!(body.checked_bodies(), 1);
    let selected = body
        .lowered()
        .module
        .functions
        .iter()
        .find(|function| function.id == body.function())
        .expect("selected function");
    assert_eq!(
        selected.body.owner(),
        kagari_hir::hir::HirOwner::Body(kagari_hir::hir::BodyOwner::Function(body.function()))
    );
    assert!(body.diagnostics().is_empty());
    println!(
        "queried one body: {:?}",
        body.type_at(text.rfind("answer }").expect("reference"))
    );
    let snapshot = engine.analyze(
        engine.source_snapshot(),
        Default::default(),
        &Default::default(),
    )?;
    let analysis = snapshot.file(file).expect("source belongs to snapshot");
    let write_start = text.find("p.x = 2").expect("field write");
    assert_eq!(
        analysis.definition_at(write_start).expect("receiver").name,
        "p"
    );
    assert!(analysis.definition_at(write_start + 1).is_none());
    assert_eq!(
        analysis
            .definition_at(write_start + 2)
            .expect("member")
            .name,
        "x"
    );
    let write_member = write_start + 2;
    assert!(
        analysis.member_receiver_type(write_member).is_some(),
        "field write receiver"
    );
    let broken_index = text.find("items[true].x").expect("invalid index") + "items[true].".len();
    assert_eq!(
        analysis
            .definition_at(broken_index)
            .expect("known array element field")
            .name,
        "x"
    );
    assert!(analysis.member_receiver_type(broken_index).is_some());
    let indexed_write =
        text.find("items[true].x =").expect("invalid indexed write") + "items[true].".len();
    assert_eq!(
        analysis
            .definition_at(indexed_write)
            .expect("indexed write field")
            .name,
        "x"
    );
    assert!(analysis.member_receiver_type(indexed_write).is_some());
    assert_eq!(
        analysis
            .result()
            .diagnostics()
            .iter()
            .filter(|diagnostic| matches!(
                diagnostic.kind,
                kagari_common::DiagnosticKind::InvalidIndexTarget { .. }
            ))
            .count(),
        2,
        "one diagnostic per invalid read/write index"
    );
    let readonly = text.find("p = Point").expect("readonly assignment");
    assert!(
        analysis.type_at(readonly).is_some(),
        "known type survives write rejection"
    );
    let initializer_field =
        text.find("Point { x: 2").expect("struct initializer") + "Point { ".len();
    assert_eq!(
        analysis
            .definition_at(initializer_field)
            .expect("checked initializer field")
            .name,
        "x"
    );
    let index_member = text.find("missing[p.x]").expect("broken assignment") + "missing[p.".len();
    assert_eq!(
        analysis
            .definition_at(index_member)
            .expect("independent index member")
            .name,
        "x"
    );
    assert!(analysis.type_at(index_member).is_some());
    let facts = analysis.result().facts();
    let helper = facts
        .lowered
        .module
        .body
        .expressions()
        .find_map(|(id, _)| match facts.names.expr_resolution(id) {
            Some(kagari_hir::resolver::ResolvedName::RuntimeHelper(helper)) => Some(helper),
            _ => None,
        })
        .expect("resolved prelude helper");
    println!("prelude helper target -> {helper:?}");
    let constructor_start = text.find("Mode::Running").expect("constructor reference");
    assert!(analysis.definition_at(constructor_start).is_none());
    assert!(
        analysis
            .definition_at(constructor_start + "Mode".len())
            .is_none()
    );
    let constructor = analysis
        .definition_at(constructor_start + "Mode::".len())
        .expect("resolved variant target");
    println!("constructor {} -> {:?}", constructor.name, constructor.id);
    assert!(std::sync::Arc::ptr_eq(
        signature.signatures(),
        analysis.signatures()
    ));
    assert!(std::sync::Arc::ptr_eq(
        header,
        snapshot
            .declaration_snapshot()
            .file(file)
            .expect("shared declarations")
    ));
    let offset = text.rfind("answer }").expect("reference offset");
    let target = analysis.definition_at(offset).expect("resolved local");
    let position = analysis
        .source()
        .position(target.location.range.start, PositionEncoding::Utf16)
        .expect("source position");
    println!(
        "{}:{}:{} -> {} ({:?})",
        source_name,
        position.line + 1,
        position.character + 1,
        target.name,
        target.id
    );
    for binding in analysis.visible_bindings(offset) {
        println!("{}: {:?}", binding.declaration.name, binding.ty);
    }
    let field = analysis
        .definition_at(text.rfind("p.x").expect("field access") + 2)
        .expect("resolved field");
    println!("field {} -> {:?}", field.name, field.id);
    let annotation = text.find("p: Point").expect("type annotation") + 3;
    let point_type = analysis
        .definition_at(annotation)
        .expect("resolved annotation");
    println!("type {} -> {:?}", point_type.name, point_type.id);
    let nominal = analysis.type_at(annotation).expect("nominal type fact");
    let kagari_hir::types::TypeId::Struct(definition) = &nominal else {
        panic!("Point has a struct identity");
    };
    assert_eq!(
        point_type.id,
        kagari_hir::declarations::DeclarationId::Definition(definition.declaration.clone())
    );
    assert!(definition.arguments.is_empty());
    println!("nominal type -> {nominal:?}");
    let constraint = analysis
        .definition_at(text.find("T: Show").expect("trait bound") + 3)
        .expect("resolved trait constraint");
    println!("constraint {} -> {:?}", constraint.name, constraint.id);
    let method = analysis
        .definition_at(text.find("value.show()").expect("generic method call") + "value.".len())
        .expect("nominal trait method target");
    println!("trait method {} -> {:?}", method.name, method.id);
    let kagari_hir::declarations::DeclarationId::Definition(method_id) = &method.id else {
        unreachable!("nominal method");
    };
    let contract = analysis
        .result()
        .facts()
        .aggregates
        .trait_method(method_id)
        .expect("shared checked method contract");
    assert_eq!(&contract.declaration, method);
    println!(
        "checked method parameters: {:?}; result: {:?}",
        contract.params, contract.return_type
    );
    let implementation = analysis
        .definition_at(text.find("impl Show").expect("impl header") + 5)
        .expect("checked impl trait target");
    assert_eq!(implementation.id, constraint.id);
    println!(
        "impl trait {} -> {:?}",
        implementation.name, implementation.id
    );

    engine.set_source(
        source_name,
        text.replace("value + 1", "value + 2"),
        SourceLayer::Overlay,
    )?;
    let edited = engine.analyze(
        engine.source_snapshot(),
        Default::default(),
        &Default::default(),
    )?;
    assert!(edited.declaration(&target.id).is_none());
    let old_facts = analysis.result().facts();
    let old_expr = old_facts
        .lowered
        .module
        .body
        .expressions()
        .find(|(id, _)| old_facts.typed.type_table.expr_type(*id).is_some())
        .expect("typed expression")
        .0;
    let new_facts = edited.file(file).expect("edited source").result().facts();
    assert_ne!(old_expr.arena(), new_facts.lowered.module.body.arena());
    assert!(new_facts.typed.type_table.expr_type(old_expr).is_none());
    assert!(
        edited
            .file(file)
            .expect("edited source")
            .signatures_reused()
    );
    println!("body edit reused checked signatures; local bindings belong to the new query");
    assert!(edited.declaration(&field.id).is_some());
    assert_eq!(snapshot.declaration(&target.id), Some(target));
    engine.set_source(
        source_name,
        format!("{text}\r\nenum Point {{ Origin }}"),
        SourceLayer::Overlay,
    )?;
    let ambiguous = engine.analyze(
        engine.source_snapshot(),
        Default::default(),
        &Default::default(),
    )?;
    let ambiguous_file = ambiguous.file(file).expect("edited source");
    assert!(
        ambiguous_file
            .result()
            .diagnostics()
            .iter()
            .any(|d| d.kind.code() == "KG_RESOLVE_DUPLICATE_DECLARATION")
    );
    assert!(ambiguous_file.definition_at(annotation).is_none());
    assert!(ambiguous_file.definition_at(offset).is_some());
    assert_eq!(analysis.definition_at(annotation), Some(point_type));
    println!(
        "type collision has no selected declaration; the good function and old snapshot remain queryable"
    );
    let applied_text = format!("{text}\r\nfn applied<T>(value: T<>) {{}}");
    engine.set_source(source_name, applied_text.clone(), SourceLayer::Overlay)?;
    let applied = engine.analyze(
        engine.source_snapshot(),
        Default::default(),
        &Default::default(),
    )?;
    let applied_file = applied.file(file).expect("edited source");
    let application = applied_text.find("T<>").expect("empty type application");
    assert_eq!(
        applied_file.type_at(application),
        Some(kagari_hir::types::TypeId::Error)
    );
    assert_eq!(
        applied_file
            .definition_at(application)
            .expect("known base binder")
            .name,
        "T"
    );
    println!("invalid type application retains its binder target for navigation");
    let bounded = format!(
        "{text}\r\nstruct Key<T: HashKey> {{ val value: T }}\r\nfn invalid_key(value: Key<f32>) {{}}"
    );
    engine.set_source(source_name, bounded, SourceLayer::Overlay)?;
    let bounded = engine.signatures(engine.source_snapshot(), &Default::default())?;
    let signature = bounded.file(file).expect("signature query");
    assert_eq!(signature.diagnostics().len(), 1);
    assert_eq!(
        signature.diagnostics()[0].kind.code(),
        "KG_TYPE_STANDARD_CONSTRAINT_NOT_SATISFIED"
    );
    let good_body = engine
        .body(engine.source_snapshot(), good, &Default::default())?
        .expect("good body");
    assert!(good_body.diagnostics().is_empty());
    println!(
        "applied generic bounds are checked by signatures; the neighboring good body stays queryable"
    );
    let clean = "fn good(value: i32) -> i32 { value + 1 }";
    engine.set_source(source_name, clean.into(), SourceLayer::Overlay)?;
    let original = engine
        .body(engine.source_snapshot(), good, &Default::default())?
        .expect("clean body");
    assert!(original.diagnostics().is_empty());
    engine.set_source(
        source_name,
        format!("// 文档 😀\r\n{clean}"),
        SourceLayer::Overlay,
    )?;
    let moved = engine
        .body(engine.source_snapshot(), good, &Default::default())?
        .expect("shifted body");
    assert_eq!(moved.reused_bodies(), 1);
    assert_eq!(moved.checked_bodies(), 0);
    println!("leading comment edits reuse body facts with updated source locations");
    Ok(())
}
