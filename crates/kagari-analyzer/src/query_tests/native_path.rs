use crate::query_tests::standard_modules;
use crate::{
    AnalyzerQueries,
    query_tests::{common::import_facts, test_analysis},
};
use std::sync::*;
// Manual external paths use common schemas and ordinary field identities.
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
    metadata::DefinitionMetadata, table::DefinitionId,
};
use kagari_common::span::Span;
use kagari_hir::{
    analysis::{AnalysisDatabase, FileAnalysis},
    types::{TypeId, semantic::raise_type},
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FieldDef, FnDecl, Param, TypeDef, TypeDefKind,
        host::{
            HostCallPolicy, HostFieldPolicy, HostPassingStyle, HostReflectionPolicy,
            HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::{ImplDecl, ModuleDecl},
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};

#[derive(Clone)]
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

    fn method(&mut self, name: &str, result: Ty, receiver: HostPassingStyle) -> DefinitionId {
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
                    passing: vec![receiver, HostPassingStyle::Owned],
                    effects: Default::default(),
                }),
            }],
        });
        method
    }

    fn providers(&self) -> [Arc<DefinitionMetadata<ModuleDecl>>; 2] {
        for module in &self.modules {
            module.collect_definitions(&self.context).unwrap();
        }
        let definitions = self.context.snapshot();
        self.modules.clone().map(|module| {
            Arc::new(
                DefinitionMetadata::checked(definitions.clone(), module, &Default::default())
                    .unwrap(),
            )
        })
    }

    fn install(&self, database: &mut AnalysisDatabase) {
        let mut providers = standard_modules();
        providers.extend(self.providers());
        database.set_native_modules(providers);
    }
}

fn imported_field_at(
    file: &FileAnalysis,
    offset: usize,
    declarations: &Hosts,
) -> Option<DefinitionId> {
    let field = file.queries().field_at(offset)?;
    let definitions = file.definitions();
    let owner = definitions.parent(field).unwrap().unwrap();
    let name = definitions.resolve(field).unwrap().last().unwrap().name;
    let actual = file.result().facts().aggregates.field(&field).unwrap();
    assert_eq!(actual.owner, owner);
    assert_eq!(actual.name, name);
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
    assert_eq!(actual.visibility, expected.visibility);
    assert_eq!(actual.writeability.is_var(), expected.mutable);
    assert_eq!(
        import_facts(&actual.ty, definitions, &declarations.context).into_records(),
        raise_type(&expected.ty)
    );
    Some(mapping.map(field).unwrap())
}

#[test]
fn field_reads_keep_offline_field_identity_after_neighbor_edits() {
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
    let changed = text.replace("{ 1 }", "{ 10 + 20 }");
    sources
        .set("mem://field", changed.clone(), SourceLayer::Base)
        .unwrap();
    let new = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let new_file = new.file(root).unwrap();
    for (file, source) in [(old_file, text), (new_file, changed.as_str())] {
        let start = source.find("score").unwrap();
        assert_eq!(imported_field_at(file, start, &declarations), Some(field));
        assert_eq!(file.queries().field_at(start - 1), None);
        assert_eq!(file.queries().field_at(start + "score".len()), None);
    }
    new.check_program(root, &Default::default()).unwrap();
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
        file.queries()
            .type_at(incomplete.find("left::make()").unwrap()),
        Some(TypeId::Host(
            file.definitions()
                .lookup(
                    &context
                        .snapshot()
                        .resolve(declarations.owners[0])
                        .unwrap()
                        .to_path()
                )
                .unwrap()
        ))
    );
    assert_eq!(
        file.queries().type_at(incomplete.rfind('7').unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

mod explicit_method;
mod queries;

#[test]
fn field_write_navigation_survives_neighbor_edits_and_immutable_field_diagnostics() {
    let mut declarations = Hosts::new();
    declarations.root(PathAccess::ReadWrite);
    let field = declarations.field(
        0,
        "score",
        Ty::Builtin(BuiltinType::I32),
        PathAccess::ReadOnly,
    );
    declarations.modules[0].types[0].fields[0].mutable = true;
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
    let changed = text.replace("{ 1 }", "{ 10 + 20 }");
    sources
        .set("mem://write", changed.clone(), SourceLayer::Base)
        .unwrap();
    let new = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = new.file(root).unwrap();

    assert_eq!(
        imported_field_at(file, changed.find("score").unwrap(), &declarations),
        Some(field)
    );
    assert_eq!(
        imported_field_at(file, changed.find("neighbor").unwrap(), &declarations),
        None
    );
    new.check_program(root, &Default::default()).unwrap();
    let mut immutable = declarations.clone();
    immutable.modules[0].types[0].fields[0].mutable = false;
    immutable.install(&mut db);
    let readonly = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let diagnostics = readonly.file(root).unwrap().result().diagnostics();
    let [diagnostic] = diagnostics else {
        panic!("expected one immutable field diagnostic, got {diagnostics:?}");
    };
    assert_eq!(
        diagnostic.kind,
        DiagnosticKind::InvalidAssignmentTarget {
            reason: "`val` field `score` cannot be assigned".into(),
        }
    );
    let span = diagnostic.span.unwrap();
    assert_eq!(&changed[span.start..span.end], "target.score");
    assert!(readonly.check_program(root, &Default::default()).is_err());
    assert_eq!(
        imported_field_at(
            readonly.file(root).unwrap(),
            changed.find("score").unwrap(),
            &immutable
        ),
        Some(field)
    );
    assert_eq!(
        imported_field_at(old_file, text.find("score").unwrap(), &declarations),
        Some(field)
    );
}

#[test]
fn source_and_host_field_chains_navigate_each_declared_member() {
    let mut declarations = Hosts::new();
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
    let chain = "value.inner.host.related.count";
    for start in [text.find(chain).unwrap(), text.rfind(chain).unwrap()] {
        let queries = file.queries();
        assert_eq!(queries.field_at(start), None);
        for name in ["inner", "host"] {
            let offset = start + chain.find(name).unwrap();
            let target = queries.definition_at(offset).unwrap();
            let location = target.source_location().unwrap();
            let declaration = text.find(&format!("{name}: ")).unwrap();
            assert_eq!(
                location,
                file.source()
                    .span(Span::new(declaration, declaration + name.len()))
                    .unwrap()
            );
            assert_eq!(queries.field_at(offset - 1), None);
        }
        for (name, field) in [("related", related), ("count", count)] {
            let offset = start + chain.find(name).unwrap();
            assert_eq!(imported_field_at(file, offset, &declarations), Some(field));
            assert_eq!(queries.field_at(offset - 1), None);
            assert_eq!(queries.field_at(offset + name.len()), None);
        }
    }
}
