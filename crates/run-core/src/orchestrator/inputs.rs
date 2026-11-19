//! Resolve a recipe's typed inputs into concrete values.
//!
//! An input's value comes from the env var `RUN_INPUT_<KEY>`, else the value
//! saved with the stack (under the same name in its `.env`), else an
//! interactive answer, else its declared default. A required input with none of
//! these is an error; an optional one resolves to the empty string. Resolved
//! values feed `${input.KEY}` expansion.

use std::collections::BTreeMap;

use crate::config::InputDef;

/// Prefix of the env var (and `.env` entry) carrying an input's value.
pub const ENV_PREFIX: &str = "RUN_INPUT_";

/// The env var (and `.env` key) carrying an input's value.
#[must_use]
pub fn env_key(key: &str) -> String {
    format!("{ENV_PREFIX}{}", key.to_ascii_uppercase())
}

/// The values chosen for inputs without prompting: an env override wins over
/// the value saved with the stack. Inputs with neither are absent, so the
/// caller can prompt for them or fall back to their defaults.
pub fn chosen_values(
    inputs: &[InputDef],
    env: impl Fn(&str) -> Option<String>,
    saved: &[(String, String)],
) -> BTreeMap<String, String> {
    let mut chosen = BTreeMap::new();

    for input in inputs {
        let var = env_key(&input.key);
        let saved_value = || saved.iter().find(|(k, _)| *k == var).map(|(_, v)| v.clone());

        if let Some(value) = env(&var).or_else(saved_value) {
            chosen.insert(input.key.clone(), value);
        }
    }

    chosen
}

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
    fn env_override_wins_over_the_saved_value() {
        let inputs = vec![input("TZ", Some("UTC"), None), input("REGION", None, None)];
        let saved = vec![
            ("RUN_INPUT_TZ".to_owned(), "Asia/Tokyo".to_owned()),
            ("RUN_INPUT_REGION".to_owned(), "eu".to_owned()),
        ];
        let env = |var: &str| (var == "RUN_INPUT_TZ").then(|| "Europe/Paris".to_owned());

        let chosen = chosen_values(&inputs, env, &saved);

        assert_eq!(chosen.get("TZ").map(String::as_str), Some("Europe/Paris"));
        assert_eq!(chosen.get("REGION").map(String::as_str), Some("eu"));
    }

    #[test]
    fn an_input_with_no_env_or_saved_value_is_left_unchosen() {
        let inputs = vec![input("TZ", Some("UTC"), None)];

        let chosen = chosen_values(&inputs, |_| None, &[]);

        assert!(chosen.is_empty(), "the default is applied later, not saved");
    }

    #[test]
    fn env_key_is_uppercased_and_prefixed() {
        assert_eq!(env_key("oidc_issuer"), "RUN_INPUT_OIDC_ISSUER");
    }

    #[test]
    fn optional_without_value_is_empty() {
        let inputs = vec![input("NOTE", None, None)];

        let resolved = resolve(&inputs, &BTreeMap::new()).expect("resolve");

        assert_eq!(resolved.get("NOTE").map(String::as_str), Some(""));
    }
}
