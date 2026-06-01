use std::collections::HashMap;

use crate::builtin::surface;
use crate::hir::{ConstId, EnumId, FunctionId, ImplId, ModuleId, StructId, TraitId};

#[derive(Debug, Clone, Default)]
pub struct NameTable {
    pub(crate) functions: HashMap<String, FunctionId>,
    pub(crate) consts: HashMap<String, ConstId>,
    pub(crate) modules: HashMap<String, ModuleId>,
    pub(crate) standard_modules: HashMap<String, surface::StandardModule>,
    pub(crate) standard_functions: HashMap<String, surface::StandardIntrinsic>,
    pub(crate) structs: HashMap<String, StructId>,
    pub(crate) enums: HashMap<String, EnumId>,
    pub(crate) traits: HashMap<String, TraitId>,
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

    pub(crate) fn insert_struct(&mut self, name: String, id: StructId) -> Option<StructId> {
        self.structs.insert(name, id)
    }

    pub(crate) fn insert_enum(&mut self, name: String, id: EnumId) -> Option<EnumId> {
        self.enums.insert(name, id)
    }

    pub(crate) fn insert_trait(&mut self, name: String, id: TraitId) -> Option<TraitId> {
        self.traits.insert(name, id)
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
        self.structs.contains_key(name)
    }

    pub fn contains_enum(&self, name: &str) -> bool {
        self.enums.contains_key(name)
    }

    pub fn contains_trait(&self, name: &str) -> bool {
        self.traits.contains_key(name)
    }

    pub fn impl_count(&self) -> usize {
        self.impls.len()
    }
}
