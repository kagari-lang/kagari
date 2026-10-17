//! Validate source implementation declarations before building aggregate records.
//! Dynamic value eligibility is checked against the completed applied catalog in
//! `typeck::interfaces`, independently of local or imported declaration origin.

use crate::{
    declarations::Declarations,
    lower::LoweredModule,
    resolver::resolved::ResolvedName,
    typeck::{
        check::possibly_overlapping_impls,
        constraints,
        table::TypeTable,
        ty::{display_type, display_type_id},
    },
    types::{NominalType, TypeId},
};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::language::Protocol;
use smallvec::SmallVec;

pub(super) fn validate_trait_surface(
    lowered: &LoweredModule,
    declarations: &Declarations,
    table: &mut TypeTable,
    diagnostics: &mut SmallVec<[Diagnostic; 4]>,
) {
    let mut seen_impls: Vec<(NominalType, TypeId)> = Vec::new();
    for impl_block in &lowered.module.impls {
        let Some(reference) = &impl_block.trait_ref else {
            continue;
        };
        let trait_name = display_type(&lowered.module, reference.ty);
        let Some(target) = table.constraint(reference.ty) else {
            continue;
        };
        let id = target;
        let trait_def = declarations
            .definition_target(&id.declaration)
            .and_then(|target| match target {
                ResolvedName::Trait(local_id) => lowered
                    .module
                    .traits
                    .iter()
                    .find(|item| item.id == local_id),
                _ => None,
            });
        let imported_trait = declarations
            .imported_types()
            .by_declaration(&id.declaration);

        // Applied bounds require the completed implementation catalog. The
        // application pass validates these headers together with imported traits.
        let Some(for_ty) = impl_block
            .for_type
            .and_then(|ty| table.type_ref(ty))
            .map(|resolved| resolved.ty.clone())
            .filter(|ty| !ty.is_unresolved())
        else {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                    trait_name: trait_name.to_string(),
                    type_name: "<missing>".to_string(),
                    reason: "impl target type is unknown".to_string(),
                })
                .with_span(lowered.source_map.impl_span(impl_block.id)),
            );
            continue;
        };
        let type_name = display_type_id(declarations.definitions(), &for_ty);
        // Installed implementations can define protocols on interface values,
        // just as Iterator's registered Iterable adapter does. This does not
        // extend source impl targets or allow bare generic receiver blankets.
        if matches!(for_ty, TypeId::Generic(_))
            || matches!(for_ty, TypeId::Trait(_)) && !lowered.registered_native_api
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                    trait_name: trait_name.to_string(),
                    type_name,
                    reason: "impl target must be a concrete type".to_string(),
                })
                .with_span(lowered.source_map.impl_span(impl_block.id)),
            );
            continue;
        }

        if seen_impls.iter().any(|(previous_trait, previous_type)| {
            previous_trait.declaration == id.declaration
                && (previous_trait.arguments == id.arguments
                    || !previous_trait.arguments.iter().all(TypeId::is_concrete)
                    || !id.arguments.iter().all(TypeId::is_concrete))
                && possibly_overlapping_impls(previous_type, &for_ty)
        }) {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidTraitImpl {
                    trait_name: trait_name.to_string(),
                    type_name: type_name.clone(),
                    reason: "overlapping impl".to_string(),
                })
                .with_span(lowered.source_map.impl_span(impl_block.id)),
            );
            continue;
        }
        seen_impls.push((id.clone(), for_ty.clone()));

        let standard = Protocol::from_definition(&id.declaration, declarations.definitions());
        if standard.is_some_and(|kind| {
            !kind.host_implementable() && matches!(for_ty, TypeId::Host(_))
                || !lowered.registered_native_api
                    && kind != Protocol::From
                    && !matches!(
                        for_ty,
                        TypeId::NativeObject(_)
                            | TypeId::Struct(_)
                            | TypeId::Enum(_)
                            | TypeId::Host(_)
                    )
        }) {
            diagnostics.push(Diagnostic::error(DiagnosticKind::InvalidTraitImpl { trait_name: trait_name.clone(), type_name: type_name.clone(), reason: "standard operator/equality impls require a script Struct or enum; formatting requires a nominal receiver".into() }).with_span(lowered.source_map.impl_span(impl_block.id)));
            continue;
        }
        let methods = if let Some(trait_def) = trait_def {
            trait_def
                .methods
                .iter()
                .filter_map(|method| {
                    impl_block
                        .methods
                        .iter()
                        .find(|implementation| implementation.name == method.name)
                        .map(|implementation| {
                            (
                                *declarations
                                    .definition(ResolvedName::Function(method.function))
                                    .expect("trait method declaration"),
                                implementation.function,
                            )
                        })
                })
                .collect()
        } else {
            imported_trait
                .into_iter()
                .flat_map(|trait_type| &trait_type.trait_methods)
                .filter_map(|method| {
                    impl_block
                        .methods
                        .iter()
                        .find(|implementation| implementation.name == method.name)
                        .map(|implementation| (method.declaration, implementation.function))
                })
                .collect()
        };
        let parameters = impl_block
            .generic_params
            .iter()
            .filter_map(|parameter| declarations.generic_type(parameter.id))
            .collect();
        let bounds = constraints::implementation_bounds(impl_block, declarations, table);
        table.insert_implementation(
            *declarations
                .impl_identity(impl_block.id)
                .expect("checked impl declaration identity"),
            id,
            for_ty,
            parameters,
            bounds,
            methods,
        );
    }
}
