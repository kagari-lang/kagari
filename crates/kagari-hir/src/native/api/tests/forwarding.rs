//! A qualified forwarding call must keep its trait and enclosing binders on both input routes.
use crate::{
    analysis::{AnalysisDatabase, error::AnalysisError},
    typeck::table::CallTarget,
    types::{GenericParameterType, TypeId},
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        AssociatedTypeDef, FnDecl, Param, TraitDef,
        module::{ImplDecl, ModuleDecl},
        script::ScriptExpr,
    },
    ty::{GenericBound, GenericParam, NominalTy, Ty},
};
use std::{slice, sync::Arc};

fn registration() -> DefinitionMetadata<ModuleDecl> {
    let context = DefinitionContext::new().unwrap();
    let mut module = ModuleDecl::new(ModuleIdentity::single_file("forwarding.kgr"));
    let root = context.intern_root(&module.identity).unwrap();
    let child = |owner, kind, name| context.intern_child(owner, kind, name, 0).unwrap();
    let forward = child(root, DefinitionKind::Trait, "Forward");
    let reverse = child(root, DefinitionKind::Trait, "Reverse");
    let output = child(reverse, DefinitionKind::AssociatedType, "Output");
    let implementation = child(root, DefinitionKind::Impl, "");
    let parameter = |owner, position| GenericParam { owner, position };
    let applied = |declaration, argument| NominalTy {
        declaration,
        arguments: vec![argument],
        associated_types: Default::default(),
    };
    let method = |owner,
                  first_name: &str,
                  first_type,
                  result,
                  implementation,
                  mut inherited: Vec<GenericParam>| {
        let own = parameter(child(owner, DefinitionKind::Method, "route"), 0);
        inherited.push(own.clone());
        FnDecl {
            name: "route".into(),
            generic_params: inherited,
            bounds: vec![],
            implementation,
            method_policy: Default::default(),
            host_policy: None,
            params: vec![
                Param {
                    name: first_name.into(),
                    ty: first_type,
                    mutable: false,
                },
                Param {
                    // A local with the member's spelling is not the qualified callee.
                    name: "route".into(),
                    ty: own.as_type(),
                    mutable: false,
                },
            ],
            return_type: result,
        }
    };
    for (id, name, first, input, result) in [
        (
            forward,
            "Forward",
            "value",
            parameter(forward, 0).as_type(),
            Ty::SelfType(forward),
        ),
        (
            reverse,
            "Reverse",
            "self",
            Ty::SelfType(reverse),
            parameter(reverse, 0).as_type(),
        ),
    ] {
        module.traits.push(TraitDef {
            name: name.into(),
            generic_params: vec![parameter(id, 0)],
            methods: vec![method(
                id,
                first,
                input,
                result,
                CallableImplementation::Required,
                vec![],
            )],
            associated_types: if id == reverse {
                vec![AssociatedTypeDef {
                    declaration: output,
                    generic_params: vec![],
                    parameter_bounds: vec![],
                    bounds: vec![],
                }]
            } else {
                vec![]
            },
            associated_consts: vec![],
            bounds: vec![],
            supertraits: vec![],
            conversion_adapter: None,
        });
    }
    let binders = vec![parameter(implementation, 0), parameter(implementation, 1)];
    let source = binders[0].as_type();
    let destination = binders[1].as_type();
    let interface = applied(forward, source.clone());
    let mut header = applied(reverse, destination.clone());
    header.associated_types.insert(output, destination.clone());
    module.implementations.push(ImplDecl {
        generic_params: binders.clone(),
        for_type: source.clone(),
        trait_type: Some(header),
        bounds: vec![GenericBound {
            ty: destination.clone(),
            constraints: vec![interface.clone()],
        }],
        methods: vec![method(
            implementation,
            "self",
            source,
            destination.clone(),
            CallableImplementation::Script,
            binders,
        )],
    });
    let owner = child(implementation, DefinitionKind::Method, "route");
    let member = child(forward, DefinitionKind::Method, "route");
    module.script_bodies.insert(
        owner,
        ScriptExpr::Call {
            callee: Box::new(ScriptExpr::TraitMethod {
                receiver: destination,
                interface,
                member,
            }),
            type_arguments: vec![parameter(owner, 0).as_type()],
            arguments: vec![
                ScriptExpr::Name("self".into()),
                ScriptExpr::Name("route".into()),
            ],
        },
    );
    let definitions = module.collect_definitions(&context).unwrap();
    DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap()
}

#[test]
fn qualified_script_callee_matches_rendered_offline_semantics() {
    let registration = Arc::new(registration());
    let mut direct = AnalysisDatabase::new("/project").unwrap();
    direct.set_native_modules(vec![registration.clone()]);
    let cancel = Default::default();
    let published = direct.declarations(Default::default(), &cancel).unwrap();
    let rendered = published
        .render_native_document(&registration, &cancel)
        .unwrap();
    assert!(
        rendered
            .text
            .contains("<T2 as Forward<T1>>::route::<M>(self, route)"),
        "{}",
        rendered.text
    );
    let mut offline = AnalysisDatabase::new("/project").unwrap();
    let mut documents = SourceDatabase::default();
    documents
        .bind_module(&rendered.uri, registration.records().identity.clone())
        .unwrap();
    documents
        .set(&rendered.uri, rendered.text, SourceLayer::Base)
        .unwrap();
    offline.set_import_environment(published.import_environment().clone());
    for snapshot in [
        direct.snapshot(Default::default(), &cancel).unwrap(),
        offline.snapshot(documents.snapshot(), &cancel).unwrap(),
    ] {
        let file = snapshot
            .files()
            .find(|f| f.source().module_identity() == &registration.records().identity)
            .unwrap();
        assert!(
            file.result().diagnostics().is_empty(),
            "{:?}",
            file.result().diagnostics()
        );
        let definitions = file.definitions();
        let root = definitions
            .lookup_root(&registration.records().identity)
            .unwrap();
        let child = |id, kind, name| definitions.lookup_child(id, kind, name, 0).unwrap();
        let forward = child(root, DefinitionKind::Trait, "Forward");
        let reverse = child(root, DefinitionKind::Trait, "Reverse");
        let owner = child(root, DefinitionKind::Impl, "");
        let generic = |owner, position| {
            TypeId::Generic(GenericParameterType {
                owner,
                position,
                name: String::new(),
            })
        };
        let s = generic(owner, 0);
        let d = generic(owner, 1);
        let m = generic(child(owner, DefinitionKind::Method, "route"), 0);
        let facts = file.result().facts();
        let table = facts.typed.type_table();
        let (_, header, receiver, _, bounds, _) = table
            .implementation_entries()
            .find(|(id, ..)| **id == owner)
            .unwrap();
        assert_eq!((header.declaration, receiver), (reverse, &s));
        assert_eq!(header.arguments.as_slice(), slice::from_ref(&d));
        assert_eq!(
            header.associated_types[&child(reverse, DefinitionKind::AssociatedType, "Output")],
            d
        );
        let hir = &facts.lowered.module;
        let method = hir.impls[0].methods[0].function;
        let body = hir.functions[method.index()]
            .implementation
            .script_body()
            .unwrap();
        let tail = hir.block(body).tail_expr.unwrap();
        let call = table.call_resolution(tail).unwrap();
        assert_eq!(table.protocol_receiver(tail), Some(&d));
        let CallTarget::TraitMethod { method, interface } = &call.target else {
            panic!("not an ordinary trait call: {call:?}")
        };
        assert_eq!(*method, child(forward, DefinitionKind::Method, "route"));
        assert_eq!(interface.declaration, forward);
        assert_eq!(interface.arguments.as_slice(), slice::from_ref(&s));
        assert_eq!(bounds[&d].as_slice(), slice::from_ref(interface));
        assert!(call.receiver.is_none());
        let signature = call.signature.as_ref().unwrap();
        assert_eq!(signature.params, [s, m]);
        assert_eq!(signature.return_type, d);
    }
}

#[test]
fn script_registration_without_its_body_cannot_be_lowered() {
    let (definitions, mut module) = registration().into_parts();
    module.script_bodies.clear();
    let cancel = Default::default();
    let mut db = AnalysisDatabase::default();
    db.set_native_modules(vec![Arc::new(
        DefinitionMetadata::checked(definitions, module, &cancel).unwrap(),
    )]);
    assert!(
        matches!(db.declarations(Default::default(), &cancel), Err(AnalysisError::NativeApi(error)) if error.0 == "Script registration requires a complete body")
    );
}

#[test]
fn qualified_script_callee_rejects_same_named_member_from_another_owner() {
    let (definitions, mut module) = registration().into_parts();
    let (owner, body) = module.script_bodies.iter_mut().next().unwrap();
    let ScriptExpr::Call { callee, .. } = body else {
        unreachable!()
    };
    let ScriptExpr::TraitMethod { member, .. } = callee.as_mut() else {
        unreachable!()
    };
    *member = *owner;
    assert!(
        module
            .validate(&|_| None, &definitions)
            .unwrap_err()
            .0
            .contains("invalid structured Script body")
    );
}
