//! Nominal aggregate layouts used to verify field operands before bytecode emission.
use super::ValueType;
use kagari_common::identity::DefinitionId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructLayout {
    pub declaration: DefinitionId,
    pub fields: Vec<StructFieldLayout>,
}

impl StructLayout {
    pub fn name(&self) -> &str {
        self.declaration
            .path
            .last()
            .map(|part| part.name.as_str())
            .unwrap_or("")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructFieldLayout {
    pub declaration: DefinitionId,
    pub name: String,
    pub ty: ValueType,
    pub mutable: bool,
}
