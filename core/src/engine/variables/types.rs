use indexmap::IndexMap;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ArgMap {
    pub named: IndexMap<String, String>,
    pub positional: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FinalExpansion {
    pub text: String,
    pub left_arrow_count: usize,
}
