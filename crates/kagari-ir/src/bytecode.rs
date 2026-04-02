#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    Return,
    LoadLocal(u16),
    CallHost { symbol: String, argc: u8 },
}
