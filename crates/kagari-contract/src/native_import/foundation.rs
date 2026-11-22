//! Binding identities and contracts reserved by the core Runtime, not by a product.
use crate::native_import::NativeImport;
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::NativeDeclaration,
    language::binding,
    range::RangeKind,
    ty::{GenericParam, NominalTy, Ty},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoundationNative {
    RangeStart,
}

impl FoundationNative {
    /// Match a scoped free-function identity, never a source expression spelling.
    pub fn for_id(id: DefinitionId, definitions: &DefinitionTable) -> Option<Self> {
        let definition = definitions.resolve(id).ok()?;
        let mut segments = definition.segments();
        let segment = segments.next()?;
        if segments.next().is_none()
            && definition.module().package.0 == "kagari-core"
            && definition.module().path == ["ops"]
            && segment.kind == DefinitionKind::Function
            && segment.occurrence == 0
            && segment.name == "range_start"
        {
            Some(Self::RangeStart)
        } else {
            None
        }
    }

    pub fn import_allowed(
        import: &NativeImport,
        caller: &ModuleIdentity,
        definitions: &DefinitionTable,
    ) -> bool {
        let declaration = import.instance.declaration;
        if Self::for_id(declaration, definitions).is_none()
            && Self::for_id(import.binding, definitions).is_none()
        {
            return true;
        }
        declaration == import.binding
            && definitions
                .resolve(declaration)
                .is_ok_and(|view| view.module() == caller)
    }

    /// Independently specify the checked ABI of the implementation. The carried
    /// declaration is evidence to compare, not a way to choose what Rust accepts.
    pub fn declaration_matches(
        self,
        declaration: &NativeDeclaration,
        definitions: &DefinitionTable,
    ) -> bool {
        let id = declaration.declaration;
        if Self::for_id(id, definitions) != Some(self) {
            return false;
        }
        let function = &declaration.function;
        let parameter = GenericParam {
            owner: id,
            position: 0,
        };
        let item = parameter.as_type();
        let Some(bound) = definitions.lookup(&binding::bound_declaration()) else {
            return false;
        };
        let [range] = function.params.as_slice() else {
            return false;
        };
        function.implementation == CallableImplementation::Native(id)
            && function.name == "range_start"
            && function.generic_params == [parameter]
            && function.bounds.is_empty()
            && function.host_policy.is_none()
            && function.method_policy == Default::default()
            && !range.mutable
            && range.ty == Ty::Range(Box::new(item.clone()), RangeKind::Exclusive)
            && function.return_type
                == Ty::Enum(NominalTy {
                    declaration: bound,
                    arguments: vec![item],
                    associated_types: Default::default(),
                })
            && declaration.concrete_result.is_none()
            && declaration.callable_requirements.is_empty()
    }
}
