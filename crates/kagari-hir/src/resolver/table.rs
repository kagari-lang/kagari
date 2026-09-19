use std::collections::HashMap;

use super::ResolvedName;
use crate::builtin::surface;
use crate::hir::{ConstId, FunctionId, ImplId, ModuleId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeNameResolution {
    Unique(ResolvedName),
    Ambiguous,
}

impl TypeNameResolution {
    pub fn target(self) -> Option<ResolvedName> {
        match self {
            Self::Unique(target) => Some(target),
            Self::Ambiguous => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NameTable {
    pub(crate) source_imports: HashMap<String, usize>,
    pub(crate) host_modules: HashMap<String, crate::host::HostModuleId>,
    pub(crate) host_functions: HashMap<String, crate::host::HostFunctionId>,
    pub(crate) functions: HashMap<String, FunctionId>,
    pub(crate) consts: HashMap<String, ConstId>,
    pub(crate) modules: HashMap<String, ModuleId>,
    pub(crate) standard_modules: HashMap<String, surface::StandardModule>,
    pub(crate) standard_functions: HashMap<String, surface::StandardIntrinsic>,
    types: HashMap<String, TypeNameResolution>,
    pub(crate) impls: Vec<ImplId>,
}

impl NameTable {
    pub(crate) fn insert_function(&mut self, name: String, id: FunctionId) -> Option<FunctionId> {
        self.functions.insert(name, id)
    }

    pub(crate) fn insert_const(&mut self, name: String, id: ConstId) -> Option<ConstId> {
        self.consts.insert(name, id)
    }

    pub(crate) fn insert_module(&mut self, name: String, id: ModuleId) -> Option<ModuleId> {
        self.modules.insert(name, id)
    }

    pub(crate) fn insert_standard_module(
        &mut self,
        name: String,
        module: surface::StandardModule,
    ) -> Option<surface::StandardModule> {
        self.standard_modules.insert(name, module)
    }

    pub(crate) fn insert_standard_function(
        &mut self,
        name: String,
        intrinsic: surface::StandardIntrinsic,
    ) -> Option<surface::StandardIntrinsic> {
        self.standard_functions.insert(name, intrinsic)
    }

    /// A collision never chooses a declaration by kind or insertion order.
    pub(crate) fn insert_type(&mut self, name: String, target: ResolvedName) -> bool {
        use std::collections::hash_map::Entry;
        assert!(matches!(
            target,
            ResolvedName::Struct(_) | ResolvedName::Enum(_) | ResolvedName::Trait(_)
        ));
        match self.types.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(TypeNameResolution::Unique(target));
                true
            }
            Entry::Occupied(mut entry) => {
                entry.insert(TypeNameResolution::Ambiguous);
                false
            }
        }
    }

    pub fn local_type(&self, name: &str) -> Option<TypeNameResolution> {
        self.types.get(name).copied()
    }

    pub(crate) fn insert_impl(&mut self, id: ImplId) {
        self.impls.push(id);
    }

    pub fn contains_function(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    pub fn contains_const(&self, name: &str) -> bool {
        self.consts.contains_key(name)
    }

    pub fn contains_module(&self, name: &str) -> bool {
        self.modules.contains_key(name)
    }

    pub fn contains_standard_module(&self, name: &str) -> bool {
        self.standard_modules.contains_key(name)
    }

    pub fn contains_standard_function(&self, name: &str) -> bool {
        self.standard_functions.contains_key(name)
    }

    pub fn contains_struct(&self, name: &str) -> bool {
        matches!(
            self.local_type(name),
            Some(TypeNameResolution::Unique(ResolvedName::Struct(_)))
        )
    }

    pub fn contains_enum(&self, name: &str) -> bool {
        matches!(
            self.local_type(name),
            Some(TypeNameResolution::Unique(ResolvedName::Enum(_)))
        )
    }

    pub fn contains_trait(&self, name: &str) -> bool {
        matches!(
            self.local_type(name),
            Some(TypeNameResolution::Unique(ResolvedName::Trait(_)))
        )
    }

    pub fn impl_count(&self) -> usize {
        self.impls.len()
    }
}
