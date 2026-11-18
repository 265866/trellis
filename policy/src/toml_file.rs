//! Reading TOML files and the values in them.

use std::fs;
use std::path::Path;

use toml::{Table, Value, de};

use crate::policy_error::PolicyError;

/// Reads and parses the TOML file at `path`.
pub(crate) fn read(path: &Path) -> Result<Table, PolicyError> {
    let text = fs::read_to_string(path).map_err(|source| PolicyError::Read {
        path: path.to_owned(),
        source,
    })?;
    parse(&text).map_err(|source| PolicyError::Parse {
        path: path.to_owned(),
        source,
    })
}

/// Parses `text` as a TOML document.
pub(crate) fn parse(text: &str) -> Result<Table, de::Error> {
    use std::str::FromStr as _;

    Table::from_str(text)
}

/// The value at `keys` in `table`, such as `["workspace", "exclude"]`, if every table on the way
/// is there.
pub(crate) fn lookup<'a>(table: &'a Table, keys: &[&str]) -> Option<&'a Value> {
    let (first, rest) = keys.split_first()?;
    let mut value = table.get(*first)?;
    for key in rest {
        value = value.as_table()?.get(*key)?;
    }
    Some(value)
}

/// The key `key` written as TOML: bare if it can be, quoted otherwise.
pub(crate) fn render_key(key: &str) -> String {
    let bare = !key.is_empty()
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if bare {
        key.to_owned()
    } else {
        format!("{key:?}")
    }
}

/// `value` written as TOML on one line, for error messages.
pub(crate) fn render(value: &Value) -> String {
    match value {
        Value::String(string) => format!("{string:?}"),
        Value::Integer(integer) => integer.to_string(),
        Value::Float(float) => float.to_string(),
        Value::Boolean(boolean) => boolean.to_string(),
        Value::Datetime(datetime) => datetime.to_string(),
        Value::Array(array) => {
            let items: Vec<String> = array.iter().map(render).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Table(table) => {
            let entries: Vec<String> = table
                .iter()
                .map(|(key, value)| format!("{key} = {}", render(value)))
                .collect();
            format!("{{ {} }}", entries.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_follows_nested_tables() {
        let table = parse("[workspace.metadata]\npolicy = { a = 1 }\n").unwrap();
        assert_eq!(
            lookup(&table, &["workspace", "metadata", "policy", "a"]),
            Some(&Value::Integer(1)),
            "the value under three tables should be found"
        );
        assert_eq!(
            lookup(&table, &["workspace", "missing"]),
            None,
            "a missing key should give None"
        );
        assert_eq!(
            lookup(&table, &["workspace", "metadata", "policy", "a", "b"]),
            None,
            "a key under a value that isn't a table should give None"
        );
    }

    #[test]
    fn render_key_quotes_only_keys_that_need_it() {
        assert_eq!(
            render_key("x86_64-pc-windows-msvc"),
            "x86_64-pc-windows-msvc",
            "a bare key stays bare"
        );
        assert_eq!(
            render_key("cfg(unix)"),
            "\"cfg(unix)\"",
            "a key with other characters is quoted"
        );
    }

    #[test]
    fn render_writes_toml_on_one_line() {
        let table = parse("value = [\"lints\", { path = \"lints\", deep = true }]\n").unwrap();
        assert_eq!(
            render(&table["value"]),
            r#"["lints", { deep = true, path = "lints" }]"#,
            "arrays and inline tables should render as TOML"
        );
    }
}
