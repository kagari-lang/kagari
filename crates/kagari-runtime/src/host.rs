#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostObjectId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPassingStyle {
    Owned,
    SharedBorrow,
    UniqueBorrow,
}

#[derive(Debug, Clone)]
pub struct HostParameter {
    pub name: &'static str,
    pub type_name: &'static str,
    pub passing: HostPassingStyle,
}

#[derive(Debug, Clone)]
pub struct HostFunction {
    pub symbol: &'static str,
    pub params: Vec<HostParameter>,
    pub return_type: &'static str,
}

#[derive(Debug, Default)]
pub struct HostRegistry {
    functions: Vec<HostFunction>,
}

impl HostRegistry {
    pub fn register(&mut self, function: HostFunction) {
        self.functions.push(function);
    }

    pub fn functions(&self) -> &[HostFunction] {
        &self.functions
    }
}

#[derive(Debug)]
pub struct SharedHostRef<'host, T: ?Sized> {
    value: &'host T,
}

impl<'host, T: ?Sized> SharedHostRef<'host, T> {
    pub fn new(value: &'host T) -> Self {
        Self { value }
    }

    pub fn get(&self) -> &'host T {
        self.value
    }
}

#[derive(Debug)]
pub struct MutHostRef<'host, T: ?Sized> {
    value: &'host mut T,
}

impl<'host, T: ?Sized> MutHostRef<'host, T> {
    pub fn new(value: &'host mut T) -> Self {
        Self { value }
    }

    pub fn get(&self) -> &T {
        self.value
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.value
    }
}
