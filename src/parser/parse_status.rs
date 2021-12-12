/// Public transport simple model
#[derive(Clone, Debug, PartialEq)]
pub struct ParseStatus {
    /// status code
    pub code: u64,
    /// details
    pub detail: String,
}

impl ParseStatus {
    pub fn new(code: u64, detail: &str) -> Self {
        ParseStatus {
            code,
            detail: detail.to_string(),
        }
    }
    pub fn ok() -> Self {
        ParseStatus {
            code: 0,
            detail: "".to_string(),
        }
    }
}
