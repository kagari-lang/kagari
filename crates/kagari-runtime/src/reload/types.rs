//! Normalize verified declaration leaves without reconstructing alias syntax.

use crate::reload::ReloadValidationError;
use kagari_bytecode::module::BytecodeModule;
use kagari_common::identity::{ModuleIdentity, table::DefinitionTable};
use kagari_contract::{exports::TypeExport, types::PublicItem};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeTypeConstructor},
    range::RangeKind,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::collections::{BTreeMap, HashMap};

pub(super) struct TypeDeclarations<'a> {
    items: HashMap<(&'a ModuleIdentity, &'a str), &'a PublicItem>,
}

impl<'a> TypeDeclarations<'a> {
    pub(super) fn new(modules: impl Iterator<Item = &'a BytecodeModule>) -> Self {
        Self {
            items: modules
                .flat_map(|module| {
                    module.public_items.iter().filter_map(|item| {
                        matches!(item, PublicItem::Type(_) | PublicItem::Trait(_))
                            .then_some(((&module.identity, item.name()), item))
                    })
                })
                .collect(),
        }
    }

    pub(super) fn normalize(
        &self,
        export: &TypeExport,
        definitions: &DefinitionTable,
    ) -> Result<TypeExport, ReloadValidationError> {
        let TypeExport::Declaration(id) = export else {
            return Ok(export.clone());
        };
        let invalid = ReloadValidationError::PublicExportMismatch;
        let path = definitions
            .resolve(*id)
            .map_err(|_| invalid.clone())?
            .to_path();
        let [segment] = path.path.as_slice() else {
            return Err(invalid);
        };
        let item = self
            .items
            .get(&(&path.module, segment.name.as_str()))
            .ok_or_else(|| invalid.clone())?;
        let nominal = NominalTy {
            declaration: *id,
            arguments: Vec::new(),
            associated_types: BTreeMap::new(),
        };
        // Public ABI validation separately preserves the owning declaration's
        // binders, bounds and members. An open declaration must not collapse into
        // any one closed application, even if that application has the same owner.
        let ty = match item {
            PublicItem::Type(record) if record.generic_params.is_empty() => match record.kind {
                TypeDefKind::Struct => Ty::Struct(nominal),
                TypeDefKind::Enum => Ty::Enum(nominal),
                TypeDefKind::NativeStorage(_) => Ty::NativeObject(nominal),
                TypeDefKind::Host(_) => Ty::Host(*id),
                TypeDefKind::Native(NativeTypeConstructor::String) => {
                    Ty::Builtin(BuiltinType::String)
                }
                TypeDefKind::Native(NativeTypeConstructor::Range(RangeKind::Full)) => {
                    Ty::Range(Box::new(Ty::Builtin(BuiltinType::Unit)), RangeKind::Full)
                }
                TypeDefKind::Native(_) => return Err(invalid),
            },
            PublicItem::Trait(record) if record.generic_params.is_empty() => Ty::Trait(nominal),
            PublicItem::Type(_) | PublicItem::Trait(_) => return Ok(export.clone()),
            _ => return Err(invalid),
        };
        Ok(TypeExport::Closed(ty))
    }
}
