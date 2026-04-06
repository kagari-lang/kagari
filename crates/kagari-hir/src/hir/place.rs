#[derive(Debug, Clone)]
pub struct PlaceData {
    pub kind: PlaceKind,
}

#[derive(Debug, Clone)]
pub enum PlaceKind {
    Name(String),
}
