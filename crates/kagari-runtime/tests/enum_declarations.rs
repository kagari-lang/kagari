use kagari_hir::{
    analysis::AnalysisDatabase, declarations::DeclarationId, native::render::declaration_source,
};
use kagari_runtime::{
    Runtime,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        declarations::FunctionDecl,
        types::{Type, TypeRef},
    },
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::catalog as standard_catalog;
use kagari_types::{
    declaration::TypeDefKind,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::{collections::BTreeMap, sync::Arc};

fn event(module: &mut ModuleBuilder) -> TypeRef {
    module.documentation("# Events\n\nApplication events with typed payloads.");
    let mut event = module.define_enum("Event");
    event.documentation("# Event\n\nCarries application data.\n\n```kgr\nEvent::Closed\n```");
    let item = event.type_parameter("T").unwrap();
    let data = event.variant("Data", [item.ty(), Type::i32()]).unwrap();
    event
        .variant_documentation(&data, "Contains data and an integer sequence number.")
        .unwrap();
    event.variant("Closed", []).unwrap();
    event.variant("Nested", [event.self_type()]).unwrap();
    event.finish().unwrap()
}

#[test]
fn native_enum_authoring_uses_nominal_types_without_storage_or_frontend() {
    let mut module = ModuleBuilder::new("application::events", &DeclarationCatalog::default());
    let event = event(&mut module);
    assert!(matches!(
        event.apply([Type::i32()]).unwrap().abi(),
        Ty::Enum(_)
    ));
    assert!(event.apply([]).is_err());
    assert!(event.apply([Type::i32(), Type::i32()]).is_err());
    assert!(event.variant("Absent").is_err());
    let mut empty = module.define_enum("Empty");
    empty.documentation("An uninhabited application type.");
    let empty = empty.finish().unwrap();
    assert!(empty.variant("Anything").is_err());
    let module = module.finish().unwrap();
    let declaration = module.to_declaration().unwrap();
    assert!(
        declaration
            .types
            .iter()
            .all(|ty| ty.kind == TypeDefKind::Enum)
    );
    assert_eq!(declaration.types[0].variants[0].payload.len(), 2);
    let text = declaration_source(&declaration, &[]).unwrap().text;
    assert!(text.contains("pub enum Event<T0>"), "{text}");
    assert!(text.contains("Data(T0, i32)"), "{text}");
    assert!(text.contains("Nested(Event<T0>)"), "{text}");
    assert!(text.contains("pub enum Empty {"), "{text}");
    module.install(&mut Runtime::default()).unwrap();
}

#[test]
fn registered_enum_views_are_parsed_with_full_documentation_and_owning_sites() {
    let mut module = ModuleBuilder::new("application::events", &DeclarationCatalog::default());
    let event = event(&mut module);
    let data = event.variant("Data").unwrap();
    let module = Arc::new(module.finish().unwrap().to_declaration().unwrap());
    let mut providers = standard_catalog::shared();
    providers.push(module.clone());
    let generated = declaration_source(&module, &providers).unwrap();
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(providers);
    let sources = SourceDatabase::default();
    let snapshot = database
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let file = snapshot
        .files()
        .find(|file| file.source().name() == generated.uri)
        .unwrap();
    assert!(file.diagnostics().is_empty(), "{:?}", file.diagnostics());
    for id in [event.id(), data.id()] {
        let declaration = snapshot
            .declaration(&DeclarationId::Definition(id.clone()))
            .unwrap();
        let site = &generated.sites[id];
        assert_eq!(
            file.source().local_range(declaration.location),
            Some(site.name_span)
        );
        let doc = snapshot.documentation(&declaration.id).unwrap();
        assert_eq!(doc.documentation, module.documentation[id]);
    }
    assert_eq!(
        snapshot
            .module_documentation(&module.identity)
            .unwrap()
            .documentation,
        module.module_documentation
    );
}

#[test]
fn source_analysis_uses_registered_enum_constructors_and_patterns() {
    let mut module = ModuleBuilder::new("application::events", &DeclarationCatalog::default());
    event(&mut module);
    let module = Arc::new(module.finish().unwrap().to_declaration().unwrap());
    let mut providers = standard_catalog::shared();
    providers.push(module);
    let mut sources = SourceDatabase::default();
    let file = sources.set("main.kgr", "use application::events::Event; fn main() -> i32 { val event = Event::Data(40, 2); match event { Event::Data(value, sequence) => value + sequence, Event::Closed => 0, Event::Nested(_) => 1 } }".into(), SourceLayer::Base).unwrap();
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(providers);
    let snapshot = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let analysis = snapshot.file(file).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
}

#[test]
fn changed_enum_payload_view_cannot_attach_to_the_registered_declaration() {
    let mut module = ModuleBuilder::new("application::events", &DeclarationCatalog::default());
    event(&mut module);
    let module = Arc::new(module.finish().unwrap().to_declaration().unwrap());
    let mut providers = standard_catalog::shared();
    providers.push(module);
    let mut views = providers
        .iter()
        .map(|module| declaration_source(module, &providers).unwrap())
        .collect::<Vec<_>>();
    let view = views.last_mut().unwrap();
    assert!(view.text.contains("Data(T0, i32)"));
    view.text = view.text.replace("Data(T0, i32)", "Data(T0, i64)");
    let mut database = AnalysisDatabase::default();
    database.set_native_sources(providers, views).unwrap();
    let sources = SourceDatabase::default();
    let error = database
        .declarations(sources.snapshot(), &Default::default())
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("differs from authoritative registration"),
        "{error}"
    );
}

#[test]
fn failed_enum_completion_does_not_publish_types_or_docs() {
    let mut module = ModuleBuilder::new("application::events", &DeclarationCatalog::default());
    let mut invalid = module.define_enum("Event");
    let parameter = invalid.type_parameter("T").unwrap();
    invalid.variant("bad::name", [parameter.ty()]).unwrap();
    invalid.documentation("Must never be published.");
    assert!(invalid.finish().is_err());
    let event = event(&mut module);
    let mut duplicate = module.define_enum("Event");
    duplicate.documentation("Must not overwrite the first enum.");
    assert!(duplicate.finish().is_err());
    let mut other = module.define_enum("Other");
    assert!(
        other
            .variant_documentation(&event.variant("Data").unwrap(), "Foreign")
            .is_err()
    );
    other.variant("Unit", []).unwrap();
    assert!(other.variant("Unit", []).is_err());
    other.finish().unwrap();
    let declaration = module.finish().unwrap().to_declaration().unwrap();
    assert_eq!(declaration.types.len(), 2);
    assert!(declaration.documentation[event.id()].contains("Carries application data"));
}

#[test]
fn enum_payloads_reject_foreign_binders_and_undeclared_or_malformed_applications() {
    let mut module = ModuleBuilder::new("application::events", &DeclarationCatalog::default());
    let event = event(&mut module);
    let foreign_parameter = Type::from_semantic(Ty::Parameter {
        owner: event.id().clone(),
        position: 0,
    });
    let mut foreign = module.define_enum("Foreign");
    foreign.variant("Data", [foreign_parameter]).unwrap();
    assert!(foreign.finish().is_err());

    for arguments in [vec![], vec![Ty::Builtin(BuiltinType::I32); 2]] {
        let mut invalid =
            ModuleBuilder::new("application::invalid", &DeclarationCatalog::default());
        let mut holder = invalid.define_enum("Holder");
        holder
            .variant(
                "Data",
                [Type::from_semantic(Ty::Enum(NominalTy {
                    declaration: event.id().clone(),
                    arguments,
                    associated_types: BTreeMap::new(),
                }))],
            )
            .unwrap();
        holder.finish().unwrap();
        assert!(invalid.finish().is_err());
    }
    let declaration = module.finish().unwrap().to_declaration().unwrap();
    let providers = DeclarationCatalog::from_declarations([&declaration]).unwrap();
    let mut invalid = ModuleBuilder::new("application::arity", &providers);
    let mut holder = invalid.define_enum("Holder");
    holder
        .variant(
            "Data",
            [Type::from_semantic(Ty::Enum(NominalTy {
                declaration: event.id().clone(),
                arguments: vec![],
                associated_types: BTreeMap::new(),
            }))],
        )
        .unwrap();
    holder.finish().unwrap();
    assert!(
        invalid
            .finish()
            .unwrap_err()
            .message()
            .contains("enum application")
    );
}

#[test]
fn enum_payload_dependencies_are_checked_at_atomic_installation() {
    let mut provider = ModuleBuilder::new("application::events", &DeclarationCatalog::default());
    let event = event(&mut provider);
    let provider = provider.finish().unwrap();
    let providers = DeclarationCatalog::from_modules(&[&provider]).unwrap();
    let mut consumer = ModuleBuilder::new("application::consumer", &providers);
    let mut holder = consumer.define_enum("Holder");
    holder
        .variant("Data", [event.apply([Type::i32()]).unwrap()])
        .unwrap();
    holder.finish().unwrap();
    let consumer = consumer.finish().unwrap();
    let mut runtime = Runtime::default();
    assert!(consumer.install(&mut runtime).is_err());
    provider.install(&mut runtime).unwrap();
    consumer.install(&mut runtime).unwrap();
}

#[test]
fn declared_native_enum_signatures_reject_missing_provider_contracts() {
    let mut module = ModuleBuilder::new("application::events", &DeclarationCatalog::default());
    let event = event(&mut module);
    let mut ty = event.apply([Type::i32()]).unwrap().abi().clone();
    let Ty::Enum(nominal) = &mut ty else {
        panic!("nominal enum");
    };
    nominal.declaration.path[0].name = "Missing".into();
    let function = module
        .define_function(FunctionDecl::new("missing").parameter("value", Type::from_semantic(ty)))
        .unwrap();
    module
        .bind_with(
            function,
            NativeBinding::new(vec![Codec::Value], Codec::Value, |_| Ok(Value::Unit)),
        )
        .unwrap();
    let error = module.finish().unwrap_err();
    assert!(error.message().contains("enum is absent"), "{error}");
}
