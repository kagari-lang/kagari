#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Writeability {
    Val,
    Var,
}

impl Writeability {
    pub fn is_val(self) -> bool {
        matches!(self, Self::Val)
    }

    pub fn is_var(self) -> bool {
        matches!(self, Self::Var)
    }
}
