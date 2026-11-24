//! Environment files of request files (`SPEC-0102`): `http-client.env.json` with named environments
//! of variables, `http-client.private.env.json` beside it for secrets, `$shared` for all.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

/// The shared environment file.
pub const ENV_FILE: &str = "http-client.env.json";
/// The private environment file, meant to stay out of version control.
pub const PRIVATE_ENV_FILE: &str = "http-client.private.env.json";
/// Variables every environment has (overridden by the environment's own).
pub const SHARED: &str = "$shared";

/// The environments found for a request file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Environments {
    /// Environment names, sorted (without `$shared`).
    pub names: Vec<String>,
    /// The environment files read, in reading order.
    pub files: Vec<PathBuf>,
    /// Files that could not be read, with the reason.
    pub problems: Vec<String>,
    #[serde(skip)]
    values: BTreeMap<String, BTreeMap<String, String>>,
    /// The values the private file gave, by environment.
    #[serde(skip)]
    private: BTreeMap<String, BTreeMap<String, String>>,
}

impl Environments {
    /// The variables of environment `name`: `$shared` first, then the environment's, the private
    /// file's values over the shared file's.
    pub fn get(&self, name: &str) -> Option<BTreeMap<String, String>> {
        let own = self.values.get(name)?;
        let mut out = self.values.get(SHARED).cloned().unwrap_or_default();
        out.extend(own.clone());
        Some(out)
    }

    /// The variables of environment `name` whose value comes from the private file (behaviour 5):
    /// they are written to no file.
    pub fn private(&self, name: &str) -> BTreeMap<String, String> {
        let Some(all) = self.get(name) else {
            return BTreeMap::new();
        };
        [SHARED, name]
            .iter()
            .filter_map(|env| self.private.get(*env))
            .flatten()
            .filter(|(k, v)| all.get(*k) == Some(*v))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// The names of [`Environments::get`]`(name)` whose value comes from the private file
    /// (`SPEC-0102` v3 behaviour 5): the keys of [`Environments::private`].
    pub fn private_names(&self, name: &str) -> BTreeSet<String> {
        self.private(name).into_keys().collect()
    }

    /// Whether an environment, `$shared` included, defines variable `name`.
    pub fn defines(&self, name: &str) -> bool {
        self.values.values().any(|vars| vars.contains_key(name))
    }

    /// The environment requests run in (`SPEC-0102` v5 behaviours 2 and 4): `chosen` while the
    /// files define it, else the only one, else none. With several and none chosen, requests whose
    /// names left `unresolved` without an environment include one an environment defines are not
    /// sent; the answer names the environments to choose from.
    pub fn choose(&self, chosen: Option<&str>, unresolved: &[String]) -> EnvironmentChoice {
        if let Some(c) = chosen.filter(|c| self.names.iter().any(|n| n == c)) {
            return EnvironmentChoice::Use(c.to_owned());
        }
        match self.names.as_slice() {
            [] => EnvironmentChoice::Without,
            [only] => EnvironmentChoice::Use(only.clone()),
            all if unresolved.iter().any(|n| self.defines(n)) => {
                EnvironmentChoice::Choose(all.to_vec())
            }
            _ => EnvironmentChoice::Without,
        }
    }
}

/// Which environment requests run in (`SPEC-0102` v5 behaviour 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentChoice {
    /// This one: the chosen one, or the only one.
    Use(String),
    /// None: the files define none, or the requests use none of their variables.
    Without,
    /// Several and none chosen, and the requests use a variable of one: nothing is sent; the
    /// environments to choose from.
    Choose(Vec<String>),
}

impl EnvironmentChoice {
    /// Why nothing was sent, naming the environments: "Not sent: choose an HTTP environment:
    /// local, staging"; `None` unless the choice is [`EnvironmentChoice::Choose`].
    pub fn refusal(&self) -> Option<String> {
        match self {
            Self::Choose(names) => Some(format!(
                "Not sent: choose an HTTP environment: {}",
                names.join(", ")
            )),
            _ => None,
        }
    }
}

/// `text` (a URL or an error naming one) as it may be written to a log in the project (`SPEC-0053`
/// behaviour 5): without the user information of URLs (`user:password@`) and with `{{name}}` in
/// place of every value of `private`.
pub fn mask_for_log(text: &str, private: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (before, after) = rest.split_at(at + 3);
        out.push_str(before);
        let authority_end = after
            .find(|c: char| matches!(c, '/' | '?' | '#') || c.is_whitespace())
            .unwrap_or(after.len());
        let skip = after[..authority_end].rfind('@').map_or(0, |i| i + 1);
        rest = &after[skip..];
    }
    out.push_str(rest);
    // Longer values first, so a value that contains another is replaced whole.
    let mut secrets: Vec<(&String, &String)> =
        private.iter().filter(|(_, v)| !v.is_empty()).collect();
    secrets.sort_by_key(|(_, v)| std::cmp::Reverse(v.len()));
    for (name, value) in secrets {
        out = out.replace(value.as_str(), &format!("{{{{{name}}}}}"));
    }
    out
}

fn value_text(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Reads the environment files for a request file in `dir`: the nearest folder from `dir` up to
/// `root` that has `http-client.env.json` or `http-client.private.env.json` (behaviour 1). Values
/// that are not strings, numbers or booleans (SSL settings, nested objects) are skipped.
pub fn load_environments(dir: &Path, root: &Path) -> Environments {
    let mut env = Environments::default();
    let mut folder = dir.to_path_buf();
    let found = loop {
        if folder.join(ENV_FILE).is_file() || folder.join(PRIVATE_ENV_FILE).is_file() {
            break Some(folder);
        }
        if folder == root || !folder.pop() {
            break None;
        }
    };
    let Some(folder) = found else {
        return env;
    };
    for name in [ENV_FILE, PRIVATE_ENV_FILE] {
        let file = folder.join(name);
        if !file.is_file() {
            continue;
        }
        let parsed = std::fs::read_to_string(&file)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string()));
        match parsed {
            Ok(serde_json::Value::Object(envs)) => {
                for (env_name, vars) in envs {
                    let slot = env.values.entry(env_name.clone()).or_default();
                    for (k, v) in vars.as_object().into_iter().flatten() {
                        if let Some(text) = value_text(v) {
                            if name == PRIVATE_ENV_FILE {
                                env.private
                                    .entry(env_name.clone())
                                    .or_default()
                                    .insert(k.clone(), text.clone());
                            }
                            slot.insert(k.clone(), text);
                        }
                    }
                }
                env.files.push(file);
            }
            Ok(_) => env
                .problems
                .push(format!("{}: not a JSON object", file.display())),
            Err(e) => env.problems.push(format!("{}: {e}", file.display())),
        }
    }
    env.names = env
        .values
        .keys()
        .filter(|k| *k != SHARED)
        .cloned()
        .collect();
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_private_and_nearest_folder() {
        let root = std::env::temp_dir().join(format!("ferris-idea-httpenv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("api/requests")).unwrap();
        std::fs::write(
            root.join("api").join(ENV_FILE),
            r#"{"$shared": {"version": "v1", "host": "example.invalid"},
                "dev": {"host": "localhost", "port": 8080, "tls": {"verify": false}},
                "prod": {"host": "api.example.invalid"}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("api").join(PRIVATE_ENV_FILE),
            r#"{"dev": {"token": "local-test-token", "port": 9090}}"#,
        )
        .unwrap();
        let env = load_environments(&root.join("api/requests"), &root);
        assert_eq!(env.names, ["dev", "prod"]);
        assert_eq!(env.files.len(), 2);
        let dev = env.get("dev").unwrap();
        assert_eq!(dev["host"], "localhost");
        assert_eq!(dev["port"], "9090", "the private file wins");
        assert_eq!(dev["version"], "v1", "$shared applies");
        assert!(!dev.contains_key("tls"));
        assert_eq!(env.get("prod").unwrap()["host"], "api.example.invalid");
        // SPEC-0102 v3 AK-6: which values are private.
        assert_eq!(
            env.private_names("dev").into_iter().collect::<Vec<_>>(),
            ["port", "token"]
        );
        assert!(env.private_names("prod").is_empty());
        assert!(env.get("$missing").is_none());
        assert!(
            load_environments(&root, &root).names.is_empty(),
            "none above"
        );
        std::fs::write(root.join(ENV_FILE), "{broken").unwrap();
        assert_eq!(load_environments(&root, &root).problems.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// SPEC-0102 v5 AK-8: the chosen environment while it exists, the only one, and with several
    /// none, asking only for requests that use a variable of one.
    #[test]
    fn the_environment_is_chosen_or_asked_for() {
        let root =
            std::env::temp_dir().join(format!("ferris-idea-httpchoose-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("one")).unwrap();
        std::fs::write(
            root.join(ENV_FILE),
            r#"{"$shared": {"version": "v1"}, "local": {"host": "127.0.0.1"}, "staging": {"host": "staging.invalid"}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join(PRIVATE_ENV_FILE),
            r#"{"staging": {"token": "test-token"}}"#,
        )
        .unwrap();
        let two = load_environments(&root, &root);
        let names = vec!["local".to_owned(), "staging".to_owned()];
        let host = ["host".to_owned()];
        use EnvironmentChoice::*;
        assert_eq!(two.choose(Some("staging"), &host), Use("staging".into()));
        assert_eq!(two.choose(Some("gone"), &host), Choose(names.clone()));
        assert_eq!(two.choose(None, &host), Choose(names.clone()));
        // A private variable of one environment, and one of `$shared`, need an environment too.
        assert_eq!(two.choose(None, &["token".into()]), Choose(names.clone()));
        assert_eq!(two.choose(None, &["version".into()]), Choose(names.clone()));
        // A request that uses none of their variables goes without one.
        assert_eq!(two.choose(None, &[]), Without);
        assert_eq!(two.choose(None, &["typo".into()]), Without);
        assert_eq!(
            two.choose(None, &host).refusal().as_deref(),
            Some("Not sent: choose an HTTP environment: local, staging")
        );
        assert_eq!(Use("local".into()).refusal(), None);
        assert!(two.defines("version") && two.defines("token") && !two.defines("typo"));

        std::fs::write(
            root.join("one").join(ENV_FILE),
            r#"{"dev": {"host": "localhost"}}"#,
        )
        .unwrap();
        let one = load_environments(&root.join("one"), &root);
        assert_eq!(one.choose(None, &host), Use("dev".into()));
        assert_eq!(one.choose(Some("gone"), &[]), Use("dev".into()));
        assert_eq!(Environments::default().choose(None, &host), Without);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn private_values_are_masked_for_the_log() {
        let root =
            std::env::temp_dir().join(format!("ferris-idea-httpmask-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(ENV_FILE),
            r#"{"$shared": {"host": "example.invalid", "region": "eu"},
                "dev": {"key": "public-key", "region": "local"},
                "prod": {"host": "api.example.invalid"}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join(PRIVATE_ENV_FILE),
            r#"{"$shared": {"tenant": "tenant-7", "region": "private-shared-region"},
                "dev": {"token": "local-test-token", "key": "private-key", "pin": 4711}}"#,
        )
        .unwrap();
        let env = load_environments(&root, &root);
        let dev = env.private("dev");
        assert_eq!(
            dev.keys().map(String::as_str).collect::<Vec<_>>(),
            ["key", "pin", "tenant", "token"],
            "`region` of dev comes from the public file: {dev:?}"
        );
        assert_eq!(dev["key"], "private-key");
        // `prod` only has the shared private values.
        let prod = env.private("prod");
        assert_eq!(
            prod.keys().map(String::as_str).collect::<Vec<_>>(),
            ["region", "tenant"]
        );
        assert!(env.private("missing").is_empty());

        assert_eq!(
            mask_for_log(
                "https://user:pass-word@example.invalid/v1/tenant-7?access_token=local-test-token&pin=4711&key=public-key",
                &dev
            ),
            "https://example.invalid/v1/{{tenant}}?access_token={{token}}&pin={{pin}}&key=public-key"
        );
        // Errors name URLs too; a mail address in a path is not user information.
        assert_eq!(
            mask_for_log(
                "invalid URL http://u:p@h:x/a then https://h/users/a@b.invalid?t=local-test-token",
                &dev
            ),
            "invalid URL http://h:x/a then https://h/users/a@b.invalid?t={{token}}"
        );
        assert_eq!(mask_for_log("no url", &BTreeMap::new()), "no url");
        // A value that contains another is replaced whole.
        let nested = BTreeMap::from([
            ("short".to_owned(), "abc".to_owned()),
            ("long".to_owned(), "abc-def".to_owned()),
        ]);
        assert_eq!(
            mask_for_log("x=abc-def&y=abc", &nested),
            "x={{long}}&y={{short}}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
