//! Imported raw inputs retain scope, owner and proof checks before preparation.
use crate::native::{
    assembly::{ModuleDefinition, ModuleSetBuilder, authoring::PendingBinding},
    binding::{Codec, NativeBinding, NativeResult},
    catalog::DeclarationCatalog,
    declarations::{FunctionDecl, MethodDecl},
    types::Type,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
    metadata::DefinitionMetadata, table::DefinitionId,
};
use kagari_types::{callable::CallableImplementation, declaration::module::ModuleDecl, ty::Ty};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn owner() -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("kagari-core".into()),
        path: vec!["array".into()],
    }
}

fn seed() -> (DefinitionMetadata<ModuleDecl>, DefinitionId) {
    let context = DefinitionContext::new().unwrap();
    let mut module = ModuleDecl::new(owner());
    let root = context.intern_root(&module.identity).unwrap();
    let id = context
        .intern_child(root, DefinitionKind::Function, "seed", 0)
        .unwrap();
    module.functions.push(
        FunctionDecl::new("seed")
            .lower(CallableImplementation::Native(id), &context)
            .unwrap(),
    );
    (
        DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap(),
        id,
    )
}

#[test]
fn imported_seed_and_structural_receiver_share_one_checked_batch() {
    let (declaration, id) = seed();
    let source_scope = declaration.definitions().id();
    let preparations = Arc::new(AtomicUsize::new(0));
    let prepared = preparations.clone();
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(owner(), move |module| {
        let pending = PendingBinding::implementation_with_send(move |_| {
            prepared.fetch_add(1, Ordering::SeqCst);
            Ok(NativeBinding::new(
                vec![],
                Codec::Exact(Type::unit()),
                |_| panic!("registration must not invoke callbacks"),
            ))
        });
        module.import_declaration(&declaration, BTreeMap::from([(id, pending)]))?;
        assert_ne!(module.definitions().id(), source_scope);
        assert!(
            module
                .import_declaration(&declaration, BTreeMap::new())
                .is_err()
        );
        module.inherent_with(
            1,
            |arguments| {
                Type::from_semantic(
                    Ty::Array(Box::new(arguments[0].abi().clone())),
                    arguments[0].definitions().unwrap(),
                )
            },
            |arguments, methods| {
                methods.define(
                    MethodDecl::instance("first")
                        .returns(arguments[0].clone())
                        .implementation_send(NativeBinding::new(
                            vec![Codec::Value],
                            Codec::Value,
                            |_| panic!("registration must not invoke callbacks"),
                        )),
                )?;
                Ok(())
            },
        )?;
        let local = module.definition(DefinitionKind::Function, "seed")?;
        module.document(local, "Imported source documentation.")?;
        Ok(())
    }))
    .unwrap();
    let assembly = set.finish().unwrap().try_into_send().unwrap();
    assert_eq!(preparations.load(Ordering::SeqCst), 0);
    let snapshot = assembly.declarations();
    let record = snapshot.modules()[0].records();
    let implementation = &record.implementations[0];
    let parameter = implementation.generic_params[0].as_type();
    assert_eq!(
        implementation.for_type,
        Ty::Array(Box::new(parameter.clone()))
    );
    assert_eq!(implementation.methods[0].return_type, parameter);
    let bytes = record
        .compiler_input_bytes(snapshot.modules()[0].definitions(), &Default::default())
        .unwrap();
    let modules = assembly.into_modules().unwrap();
    assert_eq!(preparations.load(Ordering::SeqCst), 1);
    assert_eq!(
        modules[0]
            .declaration()
            .compiler_input_bytes(modules[0].definitions(), &Default::default())
            .unwrap(),
        bytes
    );
}

#[test]
fn imported_seed_rejects_foreign_owner_and_missing_or_unknown_binding() {
    for foreign in [false, true] {
        let (declaration, _) = seed();
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        let identity = if foreign {
            ModuleIdentity {
                package: PackageId("other".into()),
                path: vec![],
            }
        } else {
            owner()
        };
        set.add(ModuleDefinition::new(identity, move |module| {
            module.import_declaration(&declaration, BTreeMap::new())
        }))
        .unwrap();
        assert!(set.finish().is_err());
    }
    let (declaration, id) = seed();
    let root = declaration.definitions().lookup_root(&owner()).unwrap();
    let mut invalid = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    invalid
        .add(ModuleDefinition::new(owner(), move |module| {
            let pending = || {
                PendingBinding::implementation_with_send(|_| -> NativeResult<NativeBinding> {
                    panic!("invalid coverage must reject before factories")
                })
            };
            module.import_declaration(
                &declaration,
                BTreeMap::from([(id, pending()), (root, pending())]),
            )
        }))
        .unwrap();
    assert!(invalid.finish().is_err());
    let (declaration, id) = seed();
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(owner(), move |module| {
        module.documentation("Do not overwrite authored presentation.");
        assert!(
            module
                .import_declaration(&declaration, BTreeMap::new())
                .is_err()
        );
        assert!(module.document(id, "Foreign scope").is_err());
        Ok(())
    }))
    .unwrap();
    set.finish().unwrap();
}
