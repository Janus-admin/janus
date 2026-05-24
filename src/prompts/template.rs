use std::collections::HashMap;

/// Interpolate `{{variable}}` placeholders in a template string.
/// Missing variables leave their placeholder intact; extra variables are ignored.
pub fn render(template: &str, variables: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in variables {
        result = result.replace(&format!("{{{{{key}}}}}"), value);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_variable_interpolated() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "Alice".to_string());
        assert_eq!(render("Hello, {{name}}!", &vars), "Hello, Alice!");
    }

    #[test]
    fn multiple_variables_interpolated() {
        let mut vars = HashMap::new();
        vars.insert("a".to_string(), "X".to_string());
        vars.insert("b".to_string(), "Y".to_string());
        assert_eq!(render("{{a}} and {{b}}", &vars), "X and Y");
    }

    #[test]
    fn missing_variable_leaves_placeholder() {
        let vars = HashMap::new();
        assert_eq!(render("Hello, {{name}}!", &vars), "Hello, {{name}}!");
    }

    #[test]
    fn extra_variables_ignored() {
        let mut vars = HashMap::new();
        vars.insert("used".to_string(), "yes".to_string());
        vars.insert("unused".to_string(), "no".to_string());
        assert_eq!(render("{{used}}", &vars), "yes");
    }
}
