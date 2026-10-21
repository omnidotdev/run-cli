//! Resolve a recipe's typed inputs into concrete values.
//!
//! An input's value comes from an override (the env var `RUN_INPUT_<KEY>`), else
//! its declared default. A required input with neither is an error; an optional
//! one resolves to the empty string. Resolved values feed `${input.KEY}`
//! expansion.

use std::collections::BTreeMap;

use crate::config::InputDef;

/// Resolve input definitions into a `key -> value` map using the given overrides.
///
/// # Errors
///
/// Returns `Error::MissingInput` when a required input has neither an override
/// nor a default.
pub fn resolve(inputs: &[InputDef], overrides: &BTreeMap<String, String>) -> crate::Result<BTreeMap<String, String>> {
    let mut resolved = BTreeMap::new();

    for input in inputs {
        let value = overrides.get(&input.key).cloned().or_else(|| input.default.clone());

        if let Some(value) = value {
            resolved.insert(input.key.clone(), value);
        } else if input.required.unwrap_or(false) {
            return Err(crate::Error::MissingInput { key: input.key.clone() });
        } else {
            resolved.insert(input.key.clone(), String::new());
        }
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(key: &str, default: Option<&str>, required: Option<bool>) -> InputDef {
        InputDef {
            key: key.to_owned(),
            label: None,
            input_type: None,
            default: default.map(ToOwned::to_owned),
            required,
        }
    }

    #[test]
    fn override_wins_over_default() {
        let inputs = vec![input("TZ", Some("UTC"), None)];
        let mut overrides = BTreeMap::new();
        overrides.insert("TZ".to_owned(), "Europe/Paris".to_owned());

        let resolved = resolve(&inputs, &overrides).expect("resolve");

        assert_eq!(resolved.get("TZ").map(String::as_str), Some("Europe/Paris"));
    }

    #[test]
    fn default_is_used_without_an_override() {
        let inputs = vec![input("REGION", Some("us-east"), None)];

        let resolved = resolve(&inputs, &BTreeMap::new()).expect("resolve");

        assert_eq!(resolved.get("REGION").map(String::as_str), Some("us-east"));
    }

    #[test]
    fn required_without_value_errors() {
        let inputs = vec![input("API_KEY", None, Some(true))];

        assert!(matches!(
            resolve(&inputs, &BTreeMap::new()),
            Err(crate::Error::MissingInput { .. })
        ));
    }

    #[test]
    fn optional_without_value_is_empty() {
        let inputs = vec![input("NOTE", None, None)];

        let resolved = resolve(&inputs, &BTreeMap::new()).expect("resolve");

        assert_eq!(resolved.get("NOTE").map(String::as_str), Some(""));
    }
}
