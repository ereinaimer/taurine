use super::types::ArgMap;

pub fn interpolate(template: &str, args: &ArgMap) -> String {
    let _ = args; // placeholder
    template.to_string()
}
