//! Transitional legacy producer adaptation; shared paths own all validation.
//! Remove this adapter with the remaining HostInterface producers in HR03.
use crate::{
    declaration::{
        FieldDef, TypeDeclaration, TypeDef, TypeDefKind,
        host::{HostFieldPolicy, HostTypePolicy},
    },
    host_interface::{
        HostInterface, HostInterfaceError, adoption_error,
        path::{HostPathContract, HostPathDeclaration},
        type_declaration::Visibility as LegacyVisibility,
    },
    ty::Ty,
    visibility::Visibility,
};
use kagari_common::identity::table::DefinitionTable;

impl HostInterface {
    pub fn path_contract(
        &self,
        declaration: &HostPathDeclaration,
        definitions: &DefinitionTable,
    ) -> Result<HostPathContract, HostInterfaceError> {
        self.validate(definitions)?;
        self.resolve_path(declaration, definitions)
    }

    pub(super) fn resolve_path(
        &self,
        declaration: &HostPathDeclaration,
        definitions: &DefinitionTable,
    ) -> Result<HostPathContract, HostInterfaceError> {
        declaration
            .contract(&self.path_types(definitions)?, definitions)
            .map_err(adoption_error)
    }

    #[doc(hidden)]
    /// Convert the remaining legacy producer's data fields into shared schemas.
    /// This temporary HR03 bridge owns no validation or Rust storage. Identity
    /// errors reject conversion; path validation belongs to HostPathDeclaration.
    pub fn path_types(
        &self,
        definitions: &DefinitionTable,
    ) -> Result<Vec<TypeDeclaration>, HostInterfaceError> {
        self.types
            .iter()
            .map(|record| {
                let view = definitions
                    .resolve(record.id)
                    .map_err(|_| HostInterfaceError::InvalidDeclaration)?;
                let name = view
                    .last()
                    .ok_or(HostInterfaceError::InvalidDeclaration)?
                    .name
                    .to_owned();
                Ok(TypeDeclaration {
                    declaration: record.id,
                    definition: TypeDef {
                        name,
                        kind: TypeDefKind::Host(HostTypePolicy {
                            ownership: record.ownership,
                            reflection: record.reflection,
                            path_access: record.path_access,
                        }),
                        generic_params: vec![],
                        bounds: vec![],
                        variants: vec![],
                        fields: record
                            .fields
                            .iter()
                            .map(|field| FieldDef {
                                host_policy: Some(HostFieldPolicy {
                                    readable: field.readable,
                                    path_access: field.path_access,
                                }),
                                visibility: match field.visibility {
                                    LegacyVisibility::Public => Visibility::Public,
                                    LegacyVisibility::Private => Visibility::Private,
                                },
                                name: field.name.clone(),
                                ty: Ty::from_host_type(&field.ty),
                                mutable: field.writable,
                            })
                            .collect(),
                    },
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::host_interface::test_support::decode_fixture;
    use crate::{
        declaration::host::{HostTypeOwnership, PathAccess},
        host_interface::{
            path::{
                HostIndexSegmentDeclaration, HostPathSegmentDeclaration,
                HostVirtualSegmentDeclaration,
            },
            value_type::HostValueType,
        },
    };

    use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};

    #[test]
    fn index_contract_cannot_upgrade_readonly_collection_access() {
        let context = DefinitionContext::new().unwrap();
        let mut index = HostIndexSegmentDeclaration {
            slot: 0,
            collection: Ty::from_host_type(
                &(HostValueType::List(
                    context
                        .intern_child(
                            context
                                .intern_root(&ModuleIdentity::single_file("collections.kgr"))
                                .unwrap(),
                            DefinitionKind::Trait,
                            "List",
                            0,
                        )
                        .unwrap(),
                    Box::new(HostValueType::I32),
                )),
            ),
            index: Ty::from_host_type(&(HostValueType::I32)),
            result: Ty::from_host_type(&(HostValueType::I32)),
            access: PathAccess::ReadWrite,
        };
        assert!(index.validate(&context.snapshot()).is_err());
        index.access = PathAccess::ReadOnly;
        index.validate(&context.snapshot()).unwrap();
        index.collection =
            Ty::from_host_type(&(HostValueType::Array(Box::new(HostValueType::I32))));
        index.access = PathAccess::ReadWrite;
        index.validate(&context.snapshot()).unwrap();
    }

    use super::*;
    use crate::host_interface::type_declaration::{HostFieldDeclaration, HostTypeDeclaration};

    #[test]
    fn mixed_path_roundtrips_offline_and_rejects_broken_segment_contracts() {
        let context = DefinitionContext::new().unwrap();
        let mut root = HostTypeDeclaration::new(&context, "game.Inventory").unwrap();
        root.ownership = HostTypeOwnership::HostRoot;
        root.path_access = PathAccess::ReadWrite;
        let items_type = HostValueType::Array(Box::new(HostValueType::I32));
        let mut items =
            HostFieldDeclaration::new(&context, root.id, "items", items_type.clone()).unwrap();
        items.path_access = PathAccess::ReadWrite;
        items.writable = true;
        root.fields.push(items.clone());
        let declaration = HostPathDeclaration {
            root: root.id,
            segments: vec![
                HostPathSegmentDeclaration::Field(items.id),
                HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                    slot: 0,
                    collection: Ty::from_host_type(&(items_type)),
                    index: Ty::from_host_type(&(HostValueType::I32)),
                    result: Ty::from_host_type(&(HostValueType::I32)),
                    access: PathAccess::ReadOnly,
                }),
                HostPathSegmentDeclaration::Virtual(HostVirtualSegmentDeclaration {
                    name: "preview".into(),
                    result: Ty::from_host_type(&(HostValueType::I32)),
                    access: PathAccess::ReadOnly,
                }),
            ],
            access: PathAccess::ReadOnly,
            schema_epoch: 4,
        };
        let interface = HostInterface {
            types: vec![root],
            functions: vec![],
            paths: vec![declaration.clone()],
        };
        let bytes = interface.to_bytes(&context.snapshot()).unwrap();
        let decoded = decode_fixture(&bytes, &context).unwrap();
        assert_eq!(decoded, interface);
        assert_eq!(
            declaration
                .contract(
                    &interface.path_types(&context.snapshot()).unwrap(),
                    &context.snapshot()
                )
                .unwrap()
                .fingerprint(&context.snapshot())
                .unwrap(),
            decoded.paths[0]
                .contract(
                    &decoded.path_types(&context.snapshot()).unwrap(),
                    &context.snapshot()
                )
                .unwrap()
                .fingerprint(&context.snapshot())
                .unwrap()
        );
        let mut old = bytes.clone();
        old[4..6].copy_from_slice(&6u16.to_le_bytes());
        assert!(matches!(
            decode_fixture(&old, &context),
            Err(HostInterfaceError::Version)
        ));

        let mut broken = interface.clone();
        if let HostPathSegmentDeclaration::Index(index) = &mut broken.paths[0].segments[1] {
            index.collection = Ty::from_host_type(&(HostValueType::I32));
        } else {
            panic!("expected index segment");
        }
        assert_eq!(
            broken.validate(&context.snapshot()),
            Err(HostInterfaceError::InvalidDeclaration)
        );
        if let HostPathSegmentDeclaration::Index(index) = &mut broken.paths[0].segments[1] {
            index.collection =
                Ty::from_host_type(&(HostValueType::Array(Box::new(HostValueType::I32))));
            index.index = Ty::from_host_type(
                &(HostValueType::Opaque(
                    HostTypeDeclaration::new(&context, "game.Missing")
                        .unwrap()
                        .id,
                )),
            );
        }
        assert_eq!(
            broken.validate(&context.snapshot()),
            Err(HostInterfaceError::InvalidDeclaration)
        );
        if let HostPathSegmentDeclaration::Index(index) = &mut broken.paths[0].segments[1] {
            index.index = Ty::from_host_type(&(HostValueType::I32));
            index.slot = 1;
        }
        assert_eq!(
            broken.validate(&context.snapshot()),
            Err(HostInterfaceError::InvalidDeclaration)
        );
    }

    #[test]
    fn nested_field_contracts_resolve_nominal_owners_without_runtime_registration() {
        let context = DefinitionContext::new().unwrap();
        let mut child = HostTypeDeclaration::new(&context, "game.Child").unwrap();
        let mut count =
            HostFieldDeclaration::new(&context, child.id, "count", HostValueType::I32).unwrap();
        count.path_access = PathAccess::ReadOnly;
        child.fields.push(count.clone());
        let mut root = HostTypeDeclaration::new(&context, "game.Root").unwrap();
        root.ownership = HostTypeOwnership::HostRoot;
        root.path_access = PathAccess::ReadOnly;
        let mut nested =
            HostFieldDeclaration::new(&context, root.id, "child", HostValueType::Opaque(child.id))
                .unwrap();
        nested.path_access = PathAccess::ReadOnly;
        root.fields.push(nested.clone());
        let catalog = HostInterface {
            paths: vec![],
            types: vec![root.clone(), child],
            functions: vec![],
        };
        let fields = [nested.id, count.id];
        let declaration = HostPathDeclaration {
            root: root.id,
            segments: fields
                .iter()
                .copied()
                .map(HostPathSegmentDeclaration::Field)
                .collect(),
            access: PathAccess::ReadOnly,
            schema_epoch: 7,
        };
        let contract = declaration
            .contract(
                &catalog.path_types(&context.snapshot()).unwrap(),
                &context.snapshot(),
            )
            .unwrap();
        let mut published = catalog.clone();
        let mut next_schema = declaration.clone();
        next_schema.schema_epoch = 8;
        published.paths = vec![declaration.clone(), next_schema];
        let encoded = published.to_bytes(&context.snapshot()).unwrap();
        published.paths.reverse();
        assert_eq!(encoded, published.to_bytes(&context.snapshot()).unwrap());
        let decoded_paths = decode_fixture(&encoded, &context).unwrap();
        assert_eq!(decoded_paths.paths.len(), 2);
        assert!(decoded_paths.paths.iter().all(|path| {
            path.contract(
                &decoded_paths.path_types(&context.snapshot()).unwrap(),
                &context.snapshot(),
            )
            .is_ok()
        }));
        published.paths.push(declaration.clone());
        assert_eq!(
            published.validate(&context.snapshot()),
            Err(HostInterfaceError::DuplicateDeclaration)
        );
        published.paths = vec![declaration.clone()];
        published.paths[0].segments = vec![HostPathSegmentDeclaration::Field(count.id); 257];
        assert_eq!(
            published.validate(&context.snapshot()),
            Err(HostInterfaceError::TooLarge)
        );
        assert_eq!(contract.result, Ty::from_host_type(&HostValueType::I32));
        assert_eq!(contract.segments.len(), 2);
        let decoded =
            decode_fixture(&catalog.to_bytes(&context.snapshot()).unwrap(), &context).unwrap();
        assert_eq!(
            contract.fingerprint(&context.snapshot()).unwrap(),
            declaration
                .contract(
                    &decoded.path_types(&context.snapshot()).unwrap(),
                    &context.snapshot()
                )
                .unwrap()
                .fingerprint(&context.snapshot())
                .unwrap()
        );
        for invalid in [
            vec![],
            vec![count.id],
            vec![nested.id, nested.id],
            vec![fields[0], count.id, count.id],
        ] {
            assert!(
                catalog
                    .path_contract(
                        &HostPathDeclaration {
                            segments: invalid
                                .into_iter()
                                .map(HostPathSegmentDeclaration::Field)
                                .collect(),
                            ..declaration.clone()
                        },
                        &context.snapshot()
                    )
                    .is_err()
            );
        }
    }
}
