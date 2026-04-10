use indexmap::IndexMap;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ArgMap {
    pub named: IndexMap<String, String>,
    pub positional: Vec<String>,
}
