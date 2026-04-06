use std::collections::HashMap;

use crate::hir::{ConstId, EnumId, FunctionId, StaticId, StructId};

#[derive(Debug, Clone, Default)]
pub struct NameTable {
    pub(crate) functions: HashMap<String, FunctionId>,
    pub(crate) consts: HashMap<String, ConstId>,
    pub(crate) statics: HashMap<String, StaticId>,
    pub(crate) structs: HashMap<String, StructId>,
    pub(crate) enums: HashMap<String, EnumId>,
}

impl NameTable {
    pub(crate) fn insert_function(&mut self, name: String, id: FunctionId) -> Option<FunctionId> {
        self.functions.insert(name, id)
    }

    pub(crate) fn insert_const(&mut self, name: String, id: ConstId) -> Option<ConstId> {
        self.consts.insert(name, id)
    }

    pub(crate) fn insert_static(&mut self, name: String, id: StaticId) -> Option<StaticId> {
        self.statics.insert(name, id)
    }

    pub(crate) fn insert_struct(&mut self, name: String, id: StructId) -> Option<StructId> {
        self.structs.insert(name, id)
    }

    pub(crate) fn insert_enum(&mut self, name: String, id: EnumId) -> Option<EnumId> {
        self.enums.insert(name, id)
    }

    pub fn contains_function(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    pub fn contains_const(&self, name: &str) -> bool {
        self.consts.contains_key(name)
    }

    pub fn contains_static(&self, name: &str) -> bool {
        self.statics.contains_key(name)
    }

    pub fn contains_struct(&self, name: &str) -> bool {
        self.structs.contains_key(name)
    }

    pub fn contains_enum(&self, name: &str) -> bool {
        self.enums.contains_key(name)
    }
}
