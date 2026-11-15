//! Explicit traversal of the owning HIR records.

use crate::declarations::{BindingId, Declaration, DeclarationId, DeclarationOrigin, Declarations};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for DeclarationId {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(match self {
            Self::Definition(field0) => DeclarationId::Definition(mapper.reference(field0)?),
            Self::GenericParameter { owner, position } => DeclarationId::GenericParameter {
                owner: mapper.reference(owner)?,
                position: *(position),
            },
            Self::Binding(field0) => DeclarationId::Binding(BindingId {
                body: mapper.reference(&field0.body)?,
                ..field0.clone()
            }),
        })
    }
}

impl DefinitionReferences for DeclarationId {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        match self {
            Self::Definition(field0) => {
                visit(field0)?;
            }
            Self::GenericParameter { owner, position: _ } => {
                visit(owner)?;
            }
            Self::Binding(binding) => visit(&binding.body)?,
        }
        Ok(())
    }
}

impl RemapDefinitions for Declaration {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Declaration {
            id: self.id.remap_definitions(mapper)?,
            name: self.name.clone(),
            origin: match self.origin {
                DeclarationOrigin::Source(location) => DeclarationOrigin::Source(location),
                DeclarationOrigin::Structured { declaration } => DeclarationOrigin::Structured {
                    declaration: mapper.reference(&declaration)?,
                },
            },
        })
    }
}

impl DefinitionReferences for Declaration {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.id.visit_definition_ids(visit, cancel)?;
        if let DeclarationOrigin::Structured { declaration } = self.origin {
            check_cancel(cancel)?;
            visit(&declaration)?;
        }
        Ok(())
    }
}

impl DefinitionReferences for Declarations {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for id in self.language_items.values() {
            check_cancel(cancel)?;
            visit(id)?;
        }
        self.imported_types.visit_definition_ids(visit, cancel)?;
        for value0 in self.targets.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for key0 in self.identities.keys() {
            (key0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.impl_identities.values() {
            check_cancel(cancel)?;
            visit(value0)?;
        }
        for value0 in self.native_types.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::declarations::{Declaration, DeclarationId, DeclarationOrigin};
    use kagari_common::{
        identity::{
            DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
            metadata::DefinitionMetadata,
        },
        span::Span,
    };
    use kagari_source::source::SourceFile;

    #[test]
    fn declaration_origins_remap_registration_ids_without_fabricating_source() {
        let context = DefinitionContext::new().unwrap();
        let root = context
            .intern_root(&ModuleIdentity {
                package: PackageId("origins".into()),
                path: vec![],
            })
            .unwrap();
        let owner = context
            .intern_child(root, DefinitionKind::Trait, "Owner", 0)
            .unwrap();
        let member = context
            .intern_child(owner, DefinitionKind::AssociatedType, "Item", 0)
            .unwrap();
        let declaration = Declaration {
            id: DeclarationId::Definition(member),
            name: "Item".into(),
            origin: DeclarationOrigin::Structured { declaration: owner },
        };
        assert!(declaration.source_location().is_none());
        let definitions = context.snapshot();
        let owner_path = definitions.resolve(owner).unwrap().to_path();
        let member_path = definitions.resolve(member).unwrap().to_path();
        let target = DefinitionContext::new().unwrap();
        let metadata = DefinitionMetadata::checked(
            definitions.clone(),
            declaration.clone(),
            &Default::default(),
        )
        .unwrap();
        let remapped = metadata.import_into(&target, &Default::default()).unwrap();
        assert_eq!(
            remapped.records().id,
            DeclarationId::Definition(target.snapshot().lookup(&member_path).unwrap())
        );
        assert_eq!(
            remapped.records().origin,
            DeclarationOrigin::Structured {
                declaration: target.snapshot().lookup(&owner_path).unwrap()
            }
        );
        assert_ne!(remapped.records().origin, declaration.origin);
        assert!(remapped.records().source_location().is_none());

        let source = SourceFile::new("origin.kgr", "Item");
        let location = source.span(Span::new(0, 4)).unwrap();
        let declaration = Declaration {
            origin: DeclarationOrigin::Source(location),
            ..declaration
        };
        let metadata =
            DefinitionMetadata::checked(definitions, declaration, &Default::default()).unwrap();
        let remapped = metadata.import_into(&target, &Default::default()).unwrap();
        assert_eq!(remapped.records().source_location(), Some(location));
    }
}
