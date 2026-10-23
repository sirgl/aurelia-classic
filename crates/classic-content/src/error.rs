#[derive(Debug, Clone)]
pub enum ContentError {
    Io(String),
    Parse(String),
}

impl std::fmt::Display for ContentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for ContentError {}
