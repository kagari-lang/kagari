//! Manual external paths use common schemas and ordinary field identities.
use crate::{
    analysis::{AnalysisDatabase, FileAnalysis},
    tests::{common::import_facts, test_analysis},
    types::TypeId,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
    metadata::DefinitionMetadata, table::DefinitionId,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog as foundation_catalog;
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FieldDef, FnDecl, Param, TypeDef, TypeDefKind,
        host::{
            HostCallPolicy, HostFieldPolicy, HostPassingStyle, HostReflectionPolicy,
            HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::{ImplDecl, ModuleDecl},
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::sync::Arc;

struct Hosts {
    context: DefinitionContext,
    modules: [ModuleDecl; 2],
    owners: [DefinitionId; 2],
}

impl Hosts {
    fn new() -> Self {
        let context = DefinitionContext::new().unwrap();
        let modules = ["left", "right"].map(|package| {
            let mut module = ModuleDecl::new(ModuleIdentity {
                package: PackageId(package.into()),
                path: vec![],
            });
            module.types.push(TypeDef {
                name: "Item".into(),
                kind: TypeDefKind::Host(HostTypePolicy {
                    ownership: HostTypeOwnership::Opaque,
                    reflection: HostReflectionPolicy::Hidden,
                    path_access: PathAccess::None,
                }),
                generic_params: vec![],
                bounds: vec![],
                fields: vec![],
                variants: vec![],
            });
            module.collect_definitions(&context).unwrap();
            module
        });
        let owners = modules.each_ref().map(|module| {
            module
                .definition(DefinitionKind::Struct, "Item", &context.snapshot())
                .unwrap()
        });
        let mut fixture = Self {
            context,
            modules,
            owners,
        };
        fixture.modules[0]
            .dependencies
            .insert(fixture.modules[1].identity.clone());
        let root = fixture
            .context
            .snapshot()
            .lookup_root(&fixture.modules[0].identity)
            .unwrap();
        for (name, params, result, passing) in [
            ("make", vec![], Ty::Host(owners[0]), vec![]),
            (
                "take",
                vec![Param {
                    name: "value".into(),
                    ty: Ty::Host(owners[0]),
                    mutable: false,
                }],
                Ty::Builtin(BuiltinType::I32),
                vec![HostPassingStyle::SharedBorrow],
            ),
        ] {
            let id = fixture
                .context
                .intern_child(root, DefinitionKind::Function, name, 0)
                .unwrap();
            fixture.modules[0].functions.push(FnDecl {
                name: name.into(),
                implementation: CallableImplementation::Native(id),
                method_policy: Default::default(),
                generic_params: vec![],
                bounds: vec![],
                params,
                return_type: result,
                host_policy: Some(HostCallPolicy {
                    passing,
                    effects: Default::default(),
                }),
            });
        }
        fixture
    }

    fn root(&mut self, access: PathAccess) {
        self.modules[0].types[0].kind = TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: access,
        });
    }

    fn field(&mut self, owner: usize, name: &str, ty: Ty, access: PathAccess) -> DefinitionId {
        let id = self
            .context
            .intern_child(self.owners[owner], DefinitionKind::Field, name, 0)
            .unwrap();
        self.modules[owner].types[0].fields.push(FieldDef {
            name: name.into(),
            ty,
            visibility: Visibility::Public,
            mutable: access == PathAccess::ReadWrite,
            host_policy: Some(HostFieldPolicy {
                readable: true,
                path_access: access,
            }),
        });
        id
    }

    fn method(&mut self, name: &str, result: Ty) -> DefinitionId {
        let root = self
            .context
            .snapshot()
            .lookup_root(&self.modules[0].identity)
            .unwrap();
        let owner = self
            .context
            .intern_child(
                root,
                DefinitionKind::Impl,
                "",
                self.modules[0].implementations.len().try_into().unwrap(),
            )
            .unwrap();
        let method = self
            .context
            .intern_child(owner, DefinitionKind::Method, name, 0)
            .unwrap();
        self.modules[0].implementations.push(ImplDecl {
            generic_params: vec![],
            bounds: vec![],
            trait_type: None,
            for_type: Ty::Host(self.owners[0]),
            methods: vec![FnDecl {
                name: name.into(),
                implementation: CallableImplementation::Native(method),
                method_policy: Default::default(),
                generic_params: vec![],
                bounds: vec![],
                params: vec![
                    Param {
                        name: "self".into(),
                        ty: Ty::Host(self.owners[0]),
                        mutable: false,
                    },
                    Param {
                        name: "amount".into(),
                        ty: Ty::Builtin(BuiltinType::I32),
                        mutable: false,
                    },
                ],
                return_type: result,
                host_policy: Some(HostCallPolicy {
                    passing: vec![HostPassingStyle::SharedBorrow, HostPassingStyle::Owned],
                    effects: Default::default(),
                }),
            }],
        });
        method
    }

    fn install(&self, database: &mut AnalysisDatabase) {
        for module in &self.modules {
            module.collect_definitions(&self.context).unwrap();
        }
        let definitions = self.context.snapshot();
        let mut providers = foundation_catalog::shared().unwrap();
        providers.extend(self.modules.iter().cloned().map(|module| {
            Arc::new(
                DefinitionMetadata::checked(definitions.clone(), module, &Default::default())
                    .unwrap(),
            )
        }));
        database.set_native_modules(providers);
    }
}

fn imported_field_at(
    file: &FileAnalysis,
    offset: usize,
    declarations: &Hosts,
) -> Option<DefinitionId> {
    let field = file.field_at(offset)?;
    let definitions = file.definitions();
    let owner = definitions.parent(field).unwrap().unwrap();
    let name = definitions.resolve(field).unwrap().last().unwrap().name;
    let schema = file
        .result()
        .facts()
        .declarations
        .external_type(owner)
        .unwrap();
    let actual = schema
        .fields
        .iter()
        .find(|candidate| candidate.name == name)
        .unwrap();
    let actual = import_facts(actual, definitions, &declarations.context).into_records();
    let mapping = declarations
        .context
        .import(definitions, [field, owner])
        .unwrap();
    let expected_owner = declarations
        .owners
        .iter()
        .position(|id| *id == mapping.map(owner).unwrap())
        .unwrap();
    let expected = declarations.modules[expected_owner].types[0]
        .fields
        .iter()
        .find(|candidate| candidate.name == name)
        .unwrap();
    assert_eq!(&actual, expected);
    Some(mapping.map(field).unwrap())
}

#[test]
fn field_reads_keep_offline_facts_and_remap_root_ids_after_neighbor_edits() {
    let mut declarations = Hosts::new();
    let context = declarations.context.clone();
    declarations.root(PathAccess::ReadOnly);
    let field = declarations.field(
        0,
        "score",
        Ty::Builtin(BuiltinType::I32),
        PathAccess::ReadOnly,
    );
    declarations.modules[0]
        .documentation
        .insert(field, "Offline score documentation".into());
    let path = HostPathDeclaration {
        root: declarations.owners[0],
        segments: vec![HostPathSegmentDeclaration::Field(field)],
        access: PathAccess::ReadOnly,
        schema_epoch: 2,
    };
    declarations.modules[0].paths.push(path.clone());
    let mut sources = SourceDatabase::default();
    let text = "fn neighbor() -> i32 { 1 } fn read() -> i32 { left::make().score }";
    let root = sources
        .set("mem://field", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    declarations.install(&mut db);

    let old = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let old_file = old.file(root).unwrap();
    assert!(old_file.result().diagnostics().is_empty());
    assert_eq!(
        imported_field_at(old_file, text.find("score").unwrap(), &declarations).unwrap(),
        field
    );
    let changed = text.replace("{ 1 }", "{ 10 + 20 }");
    sources
        .set("mem://field", changed.clone(), SourceLayer::Base)
        .unwrap();
    let new = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let new_file = new.file(root).unwrap();
    assert_eq!(new_file.result().facts().typed.reused_bodies, 1);
    let path_fact = |file: &FileAnalysis| {
        let authoring_facts = file.to_unverified(&Default::default()).unwrap();
        let facts = authoring_facts.records().facts();
        facts
            .lowered
            .module
            .body
            .expressions()
            .find_map(|(id, _)| facts.typed.type_table.host_path(id).cloned())
            .unwrap()
    };
    let old_path = path_fact(old_file);
    let new_path = path_fact(new_file);
    assert_ne!(old_path.root, new_path.root);
    assert_eq!(old_path.declaration, new_path.declaration);
    new.check_program(root, &Default::default()).unwrap();
    for ambiguous in [false, true] {
        let mut invalid = Hosts {
            context: context.clone(),
            modules: declarations.modules.clone(),
            owners: declarations.owners,
        };
        if ambiguous {
            let mut other = path.clone();
            other.schema_epoch = 3;
            invalid.modules[0].paths.push(other);
        } else {
            invalid.modules[0].paths.clear();
        }
        invalid.install(&mut db);
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let file = snapshot.file(root).unwrap();
        assert!(
            file.result()
                .diagnostics()
                .iter()
                .any(|d| matches!(d.kind, DiagnosticKind::InvalidHostPath { .. }))
        );
        assert_eq!(
            imported_field_at(file, changed.find("score").unwrap(), &declarations).unwrap(),
            field
        );
        assert!(snapshot.check_program(root, &Default::default()).is_err());
    }
    declarations.install(&mut db);
    let incomplete = "fn bad() { left::make(). } fn good() -> i32 { 7 }";
    sources
        .set("mem://field", incomplete.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = snapshot.file(root).unwrap();
    assert!(!file.result().diagnostics().is_empty());
    assert_eq!(
        file.type_at(incomplete.find("left::make()").unwrap()),
        Some(TypeId::Host(
            file.definitions()
                .lookup(&context.snapshot().resolve(path.root).unwrap().to_path())
                .unwrap()
        ))
    );
    assert_eq!(
        file.type_at(incomplete.rfind('7').unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn field_write_facts_survive_body_reuse_and_readonly_paths_are_diagnostics() {
    let mut declarations = Hosts::new();
    declarations.root(PathAccess::ReadWrite);
    let field = declarations.field(
        0,
        "score",
        Ty::Builtin(BuiltinType::I32),
        PathAccess::ReadWrite,
    );
    declarations.modules[0].paths.push(HostPathDeclaration {
        root: declarations.owners[0],
        segments: vec![HostPathSegmentDeclaration::Field(field)],
        access: PathAccess::ReadWrite,
        schema_epoch: 0,
    });
    let mut sources = SourceDatabase::default();
    let text = "fn neighbor() -> i32 { 1 } fn update(target: left::Item) { target.score += 2; }";
    let root = sources
        .set("mem://write", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    declarations.install(&mut db);

    let old = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    old.check_program(root, &Default::default()).unwrap();
    assert_eq!(
        imported_field_at(
            old.file(root).unwrap(),
            text.find("score").unwrap(),
            &declarations
        ),
        Some(field)
    );
    let write_start = text.find("target.score").unwrap();
    let old_file = old.file(root).unwrap();
    assert_eq!(
        imported_field_at(old_file, write_start, &declarations),
        None
    );
    assert_eq!(
        imported_field_at(old_file, write_start + "target".len(), &declarations),
        None
    );
    assert_eq!(
        imported_field_at(old_file, write_start + "target.".len(), &declarations),
        Some(field)
    );
    let authoring_old_table = old
        .file(root)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let old_table = &authoring_old_table.records().facts().typed.type_table;
    let old_place = old_table.host_write_places().next().unwrap();
    let old_path = old_table.host_place_path(old_place).unwrap().clone();
    sources
        .set(
            "mem://write",
            text.replace("{ 1 }", "{ 10 + 20 }"),
            SourceLayer::Base,
        )
        .unwrap();
    let new = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = new.file(root).unwrap();
    assert_eq!(file.result().facts().typed.reused_bodies, 1);
    let authoring_table = file.to_unverified(&Default::default()).unwrap();
    let table = &authoring_table.records().facts().typed.type_table;
    let place = table.host_write_places().next().unwrap();
    let path = table.host_place_path(place).unwrap();
    assert_ne!(old_path.root, path.root);
    assert_eq!(old_path.declaration, path.declaration);
    let changed = text.replace("{ 1 }", "{ 10 + 20 }");
    assert_eq!(
        imported_field_at(file, changed.find("score").unwrap(), &declarations),
        Some(field)
    );
    assert_eq!(
        imported_field_at(file, changed.find("neighbor").unwrap(), &declarations),
        None
    );
    new.check_program(root, &Default::default()).unwrap();
    declarations.modules[0].paths[0].access = PathAccess::ReadOnly;
    declarations.install(&mut db);
    let readonly = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(
        readonly
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::InvalidHostPath { .. }))
    );
    assert!(readonly.check_program(root, &Default::default()).is_err());
    assert_eq!(
        imported_field_at(
            readonly.file(root).unwrap(),
            changed.find("score").unwrap(),
            &declarations
        ),
        Some(field)
    );
    assert_eq!(
        imported_field_at(
            old.file(root).unwrap(),
            text.find("score").unwrap(),
            &declarations
        ),
        Some(field)
    );
}

#[test]
fn mixed_field_chains_resolve_the_complete_host_suffix() {
    let mut declarations = Hosts::new();
    let context = declarations.context.clone();
    declarations.root(PathAccess::ReadWrite);
    let related = declarations.field(
        0,
        "related",
        Ty::Host(declarations.owners[1]),
        PathAccess::ReadWrite,
    );
    let count = declarations.field(
        1,
        "count",
        Ty::Builtin(BuiltinType::I32),
        PathAccess::ReadWrite,
    );
    let path = HostPathDeclaration {
        root: declarations.owners[0],
        segments: vec![
            HostPathSegmentDeclaration::Field(related),
            HostPathSegmentDeclaration::Field(count),
        ],
        access: PathAccess::ReadWrite,
        schema_epoch: 0,
    };
    declarations.modules[0].paths.push(path.clone());
    // Query abstract signatures without constructing a script heap object that
    // contains a host handle (which execution deliberately rejects).
    let text = "struct Box { val host: left::Item } struct Outer { val inner: Box } fn read(value: Outer) -> i32 { value.inner.host.related.count } fn write(value: Outer) { value.inner.host.related.count += 1; }";
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("mem://mixed", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    declarations.install(&mut db);
    let snapshot = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = snapshot.file(root).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let authoring_facts = file.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.records().facts();
    let reads: Vec<_> = facts
        .lowered
        .module
        .body
        .expressions()
        .filter_map(|(id, _)| facts.typed.type_table.host_path(id))
        .collect();
    let writes: Vec<_> = facts
        .lowered
        .module
        .body
        .places()
        .filter_map(|(id, _)| facts.typed.type_table.host_place_path(id))
        .collect();
    assert_eq!(reads.len(), 1);
    assert_eq!(writes.len(), 1);
    assert_eq!(
        import_facts(&reads[0].declaration, file.definitions(), &context).into_records(),
        path
    );
    assert_eq!(
        import_facts(&writes[0].declaration, file.definitions(), &context).into_records(),
        path
    );
    assert_eq!(
        imported_field_at(file, text.find("value.inner.host").unwrap(), &declarations),
        None
    );
    assert_eq!(
        imported_field_at(file, text.find(".related").unwrap(), &declarations),
        None
    );
    assert_eq!(
        imported_field_at(file, text.find("related").unwrap(), &declarations),
        Some(related)
    );
    assert_eq!(
        imported_field_at(file, text.find("count").unwrap(), &declarations),
        Some(count)
    );

    let read_span = facts.lowered.source_map.expr_span(reads[0].root);
    let write_span = facts.lowered.source_map.place_span(writes[0].root);
    assert_eq!(&text[read_span.start..read_span.end], "value.inner.host");
    assert_eq!(&text[write_span.start..write_span.end], "value.inner.host");
}

mod queries;
