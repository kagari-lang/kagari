use std::collections::HashMap;

use crate::hir::{EnumId, FunctionId, StructId};

#[derive(Debug, Clone, Default)]
pub struct NameTable {
    pub(crate) functions: HashMap<String, FunctionId>,
    pub(crate) structs: HashMap<String, StructId>,
    pub(crate) enums: HashMap<String, EnumId>,
}

impl NameTable {
    pub(crate) fn insert_function(&mut self, name: String, id: FunctionId) -> Option<FunctionId> {
        self.functions.insert(name, id)
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

    pub fn contains_struct(&self, name: &str) -> bool {
        self.structs.contains_key(name)
    }

    pub fn contains_enum(&self, name: &str) -> bool {
        self.enums.contains_key(name)
    }
}
