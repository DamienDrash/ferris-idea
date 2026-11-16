//! Layered settings (`SPEC-0002`).
//!
//! Settings are TOML tables stacked in a fixed order of precedence:
//! built-in defaults, then user settings, then workspace settings. A later
//! layer overrides an earlier one key by key; nested tables are merged, all
//! other values are replaced as a whole.

use serde::de::DeserializeOwned;
use toml::{Table, Value};

/// Origin of a settings layer, in ascending precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    /// Values shipped with the application.
    Defaults,
    /// Values from the user's settings file.
    User,
    /// Values from the workspace's settings file.
    Workspace,
}

/// Errors raised while reading settings.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The layer source is not valid TOML.
    #[error("invalid settings in {layer:?} layer: {source}")]
    Parse {
        /// Layer that failed to parse.
        layer: Layer,
        /// Underlying parser error.
        source: toml::de::Error,
    },
    /// A value exists but has the wrong shape for the requested type.
    #[error("setting `{key}` has an unexpected type: {source}")]
    Type {
        /// Dotted key that was requested.
        key: String,
        /// Underlying conversion error.
        source: toml::de::Error,
    },
}

/// A stack of settings layers with key-wise override.
#[derive(Debug, Default, Clone)]
pub struct LayeredConfig {
    layers: Vec<(Layer, Table)>,
}

impl LayeredConfig {
    /// Creates an empty configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses `source` as TOML and installs it as `layer`, replacing any
    /// earlier content of that layer.
    pub fn set_layer_toml(&mut self, layer: Layer, source: &str) -> Result<(), ConfigError> {
        let table = source
            .parse::<Table>()
            .map_err(|source| ConfigError::Parse { layer, source })?;
        self.set_layer(layer, table);
        Ok(())
    }

    /// Installs `table` as `layer`, replacing any earlier content of that layer.
    pub fn set_layer(&mut self, layer: Layer, table: Table) {
        self.layers.retain(|(l, _)| *l != layer);
        self.layers.push((layer, table));
        self.layers.sort_by_key(|(l, _)| *l);
    }

    /// Returns the effective settings with all layers applied.
    pub fn effective(&self) -> Table {
        let mut out = Table::new();
        for (_, table) in &self.layers {
            merge(&mut out, table);
        }
        out
    }

    /// Looks up a dotted key such as `editor.tab_size` in the effective
    /// settings. Returns `Ok(None)` when the key is not set in any layer.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, ConfigError> {
        let effective = self.effective();
        let Some(value) = lookup(&effective, key) else {
            return Ok(None);
        };
        value
            .clone()
            .try_into()
            .map(Some)
            .map_err(|source| ConfigError::Type {
                key: key.to_owned(),
                source,
            })
    }

    /// Returns the highest-precedence layer that sets `key`, if any.
    pub fn origin(&self, key: &str) -> Option<Layer> {
        self.layers
            .iter()
            .rev()
            .find(|(_, table)| lookup(table, key).is_some())
            .map(|(layer, _)| *layer)
    }
}

fn lookup<'a>(table: &'a Table, key: &str) -> Option<&'a Value> {
    let (head, rest) = match key.split_once('.') {
        Some((head, rest)) => (head, Some(rest)),
        None => (key, None),
    };
    let value = table.get(head)?;
    match (rest, value) {
        (None, v) => Some(v),
        (Some(rest), Value::Table(t)) => lookup(t, rest),
        (Some(_), _) => None,
    }
}

fn merge(into: &mut Table, from: &Table) {
    for (key, value) in from {
        match (into.get_mut(key), value) {
            (Some(Value::Table(existing)), Value::Table(incoming)) => merge(existing, incoming),
            _ => {
                into.insert(key.clone(), value.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> LayeredConfig {
        let mut c = LayeredConfig::new();
        c.set_layer_toml(
            Layer::Defaults,
            "[editor]\ntab_size = 4\ninsert_spaces = true\nfont = \"mono\"",
        )
        .unwrap();
        c.set_layer_toml(Layer::User, "[editor]\ntab_size = 2")
            .unwrap();
        c
    }

    #[test]
    fn later_layer_overrides_single_key_and_keeps_siblings() {
        let c = config();
        assert_eq!(c.get::<i64>("editor.tab_size").unwrap(), Some(2));
        assert_eq!(c.get::<bool>("editor.insert_spaces").unwrap(), Some(true));
    }

    #[test]
    fn workspace_beats_user_regardless_of_insertion_order() {
        let mut c = LayeredConfig::new();
        c.set_layer_toml(Layer::Workspace, "[editor]\ntab_size = 8")
            .unwrap();
        c.set_layer_toml(Layer::User, "[editor]\ntab_size = 2")
            .unwrap();
        assert_eq!(c.get::<i64>("editor.tab_size").unwrap(), Some(8));
        assert_eq!(c.origin("editor.tab_size"), Some(Layer::Workspace));
    }

    #[test]
    fn replacing_a_layer_drops_its_old_values() {
        let mut c = config();
        c.set_layer_toml(Layer::User, "").unwrap();
        assert_eq!(c.get::<i64>("editor.tab_size").unwrap(), Some(4));
        assert_eq!(c.origin("editor.tab_size"), Some(Layer::Defaults));
    }

    #[test]
    fn missing_key_is_none_and_wrong_type_is_error() {
        let c = config();
        assert_eq!(c.get::<i64>("editor.missing").unwrap(), None);
        assert_eq!(c.get::<i64>("editor.tab_size.deeper").unwrap(), None);
        assert!(matches!(
            c.get::<i64>("editor.font"),
            Err(ConfigError::Type { .. })
        ));
    }

    #[test]
    fn invalid_toml_reports_layer() {
        let mut c = LayeredConfig::new();
        let err = c.set_layer_toml(Layer::User, "[editor").unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Parse {
                layer: Layer::User,
                ..
            }
        ));
    }
}
