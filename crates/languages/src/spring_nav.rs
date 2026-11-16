//! Navigation over the bean model (`SPEC-0115` v5 behaviours 12 to 15, `SPEC-0134` v5 behaviour
//! 7): from a configuration key in the code to the lines that define it and back, the value a key
//! resolves to (masked for keys that look like secrets), the gutter marks of beans and injection
//! points, and what a bean depends on and what depends on it. Everything is read from
//! [`SpringModel`]; nothing here opens a file.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::spring::{Bean, Property, SpringModel, ValueRef, canonical, defines, without_profile};

/// What is shown in place of the value of a key that looks like a secret.
pub const MASK: &str = "•••";

/// Parts of a key's name that mark its value as a secret (`SPEC-0115` v5 behaviour 13).
const SECRET_PARTS: &[&str] = &["password", "passwd", "secret", "token", "key", "credential"];

/// Whether the key's name says its value is a secret: it contains one of [`SECRET_PARTS`],
/// whatever the case and with or without `-` and `_`. Rather one key too many than a secret on
/// the screen.
pub fn is_secret_key(key: &str) -> bool {
    let name = canonical(key);
    SECRET_PARTS.iter().any(|part| name.contains(part))
}

/// The value of `key` as it may be shown: [`MASK`] for a key that looks like a secret.
pub fn shown_value(key: &str, value: &str) -> String {
    if is_secret_key(key) {
        MASK.to_owned()
    } else {
        value.to_owned()
    }
}

/// What stands at a place of a file: a key used in the code, or a key defined in a configuration
/// file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySite<'a> {
    /// A key in `@Value`, `@ConfigProperty`, `@Property`, or a prefix.
    Reference(&'a ValueRef),
    /// A key of a configuration file.
    Definition(&'a Property),
}

/// One line that defines a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyDefinition {
    /// The key as written there.
    pub key: String,
    /// Workspace-relative file.
    pub file: PathBuf,
    /// 0-based line.
    pub line: usize,
    /// 0-based column of the key.
    pub column: usize,
    /// Length of the key as written, in chars.
    pub length: usize,
    /// The profile of the definition; none for the default profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// The value as it may be shown (masked for a secret); none where the line has no scalar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// One place that uses a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyUsage {
    /// The key as written there.
    pub key: String,
    /// Workspace-relative file.
    pub file: PathBuf,
    /// 0-based line.
    pub line: usize,
    /// 0-based column of the key.
    pub column: usize,
    /// Length of the key as written, in chars.
    pub length: usize,
    /// The place names a prefix the key is below.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub prefix: bool,
}

/// A configuration key with its definitions and usages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyInfo {
    /// The key asked about.
    pub key: String,
    /// The lines that define it, in file order.
    pub definitions: Vec<KeyDefinition>,
    /// The places in the code that read it.
    pub usages: Vec<KeyUsage>,
    /// The value of the active profiles, else of the default profile, as it may be shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// The profile `value` comes from; none for the default profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_profile: Option<String>,
    /// The key looks like a secret: every value is masked.
    pub secret: bool,
}

/// A resolved value shown in place of a `${…}` placeholder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValueFold {
    /// 0-based line.
    pub line: usize,
    /// Byte range of the placeholder in the file, `${` and `}` included.
    pub start: usize,
    /// End of that range.
    pub end: usize,
    /// The key.
    pub key: String,
    /// What is shown: the value, or [`MASK`].
    pub text: String,
}

/// What a gutter mark stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkKind {
    /// A bean is declared on the line; the mark leads to where it is injected.
    Bean,
    /// A bean is injected on the line; the mark leads to the beans that fit.
    Injection,
}

/// Where a gutter mark leads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MarkTarget {
    /// The row of the chooser.
    pub name: String,
    /// What stands at the target, without its place: a bean's name and type, or the class an
    /// injection point is in.
    pub what: String,
    /// Workspace-relative file.
    pub file: PathBuf,
    /// 0-based line.
    pub line: usize,
}

/// A gutter mark of the bean model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FrameworkMark {
    /// 0-based line.
    pub line: usize,
    /// What it stands for.
    pub kind: MarkKind,
    /// The tooltip.
    pub tip: String,
    /// Where a click leads.
    pub targets: Vec<MarkTarget>,
}

/// A bean another bean needs or is needed by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BeanLink {
    /// The other bean's name; for a dependency without a bean, the injected type.
    pub name: String,
    /// The other bean's type, or the injected type.
    pub type_name: String,
    /// Workspace-relative file to open: the other bean's declaration, or the injection point of
    /// a dependency without a bean.
    pub file: PathBuf,
    /// 0-based line there.
    pub line: usize,
    /// No bean fits the injection point.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub missing: bool,
}

fn file_name(file: &Path) -> String {
    file.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl SpringModel {
    /// The key at byte `at` of `file`: in the code or in a configuration file.
    pub fn key_at(&self, file: &Path, at: usize) -> Option<KeySite<'_>> {
        if let Some(v) = self
            .values
            .iter()
            .find(|v| v.file == file && v.start <= at && at <= v.end)
        {
            return Some(KeySite::Reference(v));
        }
        self.properties
            .iter()
            .find(|p| p.file == file && p.start <= at && at <= p.end)
            .map(KeySite::Definition)
    }

    /// The properties that give `v` its value, or that lie below it when it is a prefix.
    pub fn definitions_of(&self, v: &ValueRef) -> Vec<&Property> {
        self.properties
            .iter()
            .filter(|p| defines(&p.key, v))
            .collect()
    }

    /// The places in the code that read the configuration key `key` (`SPEC-0115` v5 behaviour
    /// 12): the keys that it defines, and the prefixes it is below.
    pub fn usages_of(&self, key: &str) -> Vec<&ValueRef> {
        self.values.iter().filter(|v| defines(key, v)).collect()
    }

    /// The profiles the configuration itself switches on: `spring.profiles.active`,
    /// `quarkus.profile` and `micronaut.environments` of the default profile, split at commas.
    pub fn active_profiles(&self) -> Vec<String> {
        self.properties
            .iter()
            .filter(|p| {
                p.profile.is_none()
                    && matches!(
                        p.key.as_str(),
                        "spring.profiles.active" | "quarkus.profile" | "micronaut.environments"
                    )
            })
            .filter_map(|p| p.value.as_deref())
            .flat_map(|v| v.split(','))
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .collect()
    }

    /// The definition whose value the application gets among `defs`: the last one of an active
    /// profile, else of the default profile a `.properties` file before a YAML file, the last
    /// of them. A definition of a profile nothing switches on never counts.
    fn winning<'a>(&self, defs: &[&'a Property]) -> Option<&'a Property> {
        let active = self.active_profiles();
        let of_active = defs
            .iter()
            .rev()
            .find(|p| p.profile.as_ref().is_some_and(|x| active.contains(x)));
        let is_properties = |p: &&&Property| p.file.extension().is_some_and(|e| e == "properties");
        let default = || defs.iter().rev().filter(|p| p.profile.is_none());
        of_active
            .or_else(|| default().find(is_properties))
            .or_else(|| default().next())
            .copied()
    }

    /// The value `v` resolves to and the profile it comes from, unmasked. None for a prefix, for
    /// a key no default or active profile defines, and for a definition without a scalar.
    pub fn resolved(&self, v: &ValueRef) -> Option<(&str, Option<&str>)> {
        if v.prefix {
            return None;
        }
        let p = self.winning(&self.definitions_of(v))?;
        Some((p.value.as_deref()?, p.profile.as_deref()))
    }

    /// The resolved values to show in place of the `${…}` placeholders of `file` (`SPEC-0115` v5
    /// behaviour 13), masked for keys that look like secrets.
    pub fn value_folds(&self, file: &Path) -> Vec<ValueFold> {
        self.values
            .iter()
            .filter(|v| v.file == file)
            .filter_map(|v| {
                let (start, end) = v.span?;
                let (value, _) = self.resolved(v)?;
                Some(ValueFold {
                    line: v.line,
                    start,
                    end,
                    key: v.key.clone(),
                    text: shown_value(&v.key, value),
                })
            })
            .collect()
    }

    fn definition(p: &Property) -> KeyDefinition {
        KeyDefinition {
            key: p.key.clone(),
            file: p.file.clone(),
            line: p.line,
            column: p.column,
            length: p.end.saturating_sub(p.start),
            profile: p.profile.clone(),
            value: p.value.as_deref().map(|v| shown_value(&p.key, v)),
        }
    }

    fn usage(v: &ValueRef) -> KeyUsage {
        KeyUsage {
            key: v.key.clone(),
            file: v.file.clone(),
            line: v.line,
            column: v.column,
            length: v.key.chars().count(),
            prefix: v.prefix,
        }
    }

    /// Definitions, usages and value of the key a place of the code reads.
    pub fn key_info_of(&self, v: &ValueRef) -> KeyInfo {
        let defs = self.definitions_of(v);
        // The usages of every spelling the definitions have, and of the key as written here.
        let mut usages: Vec<&ValueRef> = Vec::new();
        let spellings = defs
            .iter()
            .map(|p| p.key.as_str())
            .chain((!v.prefix).then_some(v.key.as_str()));
        for key in spellings {
            for u in self.usages_of(key) {
                if !usages.iter().any(|x| std::ptr::eq(*x, u)) {
                    usages.push(u);
                }
            }
        }
        if v.prefix && !usages.iter().any(|x| std::ptr::eq(*x, v)) {
            usages.push(v);
        }
        usages.sort_by(|a, b| (&a.file, a.start).cmp(&(&b.file, b.start)));
        let resolved = self.resolved(v);
        KeyInfo {
            key: v.key.clone(),
            definitions: defs.iter().map(|p| Self::definition(p)).collect(),
            usages: usages.into_iter().map(Self::usage).collect(),
            value: resolved.map(|(value, _)| shown_value(&v.key, value)),
            value_profile: resolved.and_then(|(_, p)| p.map(str::to_owned)),
            secret: is_secret_key(&v.key),
        }
    }

    /// Definitions, usages and value of `key` as a `@Value` placeholder would read it (relaxed
    /// binding): the question the MCP tool `config_key` asks.
    pub fn key_info(&self, key: &str) -> KeyInfo {
        let key = without_profile(key);
        self.key_info_of(&ValueRef {
            key: key.to_owned(),
            has_default: false,
            file: PathBuf::new(),
            line: 0,
            column: 0,
            start: 0,
            end: 0,
            relaxed: true,
            prefix: false,
            span: None,
        })
    }

    /// Definitions, usages and value of the key at byte `at` of `file`, in the code or in a
    /// configuration file.
    pub fn key_info_at(&self, file: &Path, at: usize) -> Option<KeyInfo> {
        Some(match self.key_at(file, at)? {
            KeySite::Reference(v) => self.key_info_of(v),
            KeySite::Definition(p) => {
                let mut info = self.key_info(&p.key);
                // Seen from the file, the key is the one written there, and its usages are the
                // places that read exactly this line's key.
                info.key = without_profile(&p.key).to_owned();
                info.usages = self
                    .usages_of(&p.key)
                    .into_iter()
                    .map(Self::usage)
                    .collect();
                info
            }
        })
    }

    /// Quick documentation of a key as Markdown paragraphs: the value the application gets, the
    /// defining lines with their profiles and values, and the number of usages. Values of keys
    /// that look like secrets are masked.
    pub fn key_doc(info: &KeyInfo) -> String {
        let mut out = format!("# {}\n\nConfiguration key", info.key);
        match (&info.value, &info.value_profile) {
            (Some(v), Some(p)) => out.push_str(&format!("\n\nValue: `{v}` (profile {p})")),
            (Some(v), None) => out.push_str(&format!("\n\nValue: `{v}`")),
            (None, _) if info.definitions.is_empty() => {
                out.push_str("\n\nNot defined in the application configuration")
            }
            (None, _) => out.push_str("\n\nNo value in the default profile"),
        }
        if info.secret {
            out.push_str("\n\nThe name looks like a secret: its values are not shown");
        }
        for d in &info.definitions {
            let place = format!("{}:{}", d.file.display(), d.line + 1);
            let profile = d
                .profile
                .as_ref()
                .map_or(String::new(), |p| format!(" (profile {p})"));
            let value = d
                .value
                .as_ref()
                .map_or(String::new(), |v| format!(" = `{v}`"));
            out.push_str(&format!("\n\nDefined in `{place}`{profile}{value}"));
        }
        out.push_str(&match info.usages.len() {
            0 => "\n\nNot used in the project's Java sources".to_owned(),
            1 => "\n\nUsed in 1 place".to_owned(),
            n => format!("\n\nUsed in {n} places"),
        });
        out
    }

    /// The bean a class or producer line of `file` declares on `line`, by index.
    fn beans_on(&self, file: &Path) -> impl Iterator<Item = (usize, &Bean)> {
        self.beans
            .iter()
            .enumerate()
            .filter(move |(_, b)| b.file == file)
    }

    /// The gutter marks of `file` (`SPEC-0115` v5 behaviour 14): one on the line of every bean
    /// class and producer, leading to the places the bean is injected, and one on the line of
    /// every injection point, leading to the beans that fit. Sorted by line; a line with several
    /// injection points has one mark with all their targets.
    pub fn gutter_marks(&self, file: &Path) -> Vec<FrameworkMark> {
        let mut marks: Vec<FrameworkMark> = Vec::new();
        for (index, bean) in self.beans_on(file) {
            let targets: Vec<MarkTarget> = self
                .injections
                .iter()
                .filter(|i| i.candidates.contains(&index))
                .map(|i| MarkTarget {
                    what: i.owner.clone(),
                    name: format!("{} — {}:{}", i.owner, file_name(&i.file), i.line + 1),
                    file: i.file.clone(),
                    line: i.line,
                })
                .collect();
            let tip = match targets.len() {
                0 => format!("Bean {}: not injected in the project's sources", bean.name),
                1 => format!("Bean {}: injected in 1 place", bean.name),
                n => format!("Bean {}: injected in {n} places", bean.name),
            };
            marks.push(FrameworkMark {
                line: bean.line,
                kind: MarkKind::Bean,
                tip,
                targets,
            });
        }
        for inj in self.injections.iter().filter(|i| i.file == file) {
            let targets: Vec<MarkTarget> = inj
                .candidates
                .iter()
                .map(|&c| {
                    let b = &self.beans[c];
                    MarkTarget {
                        what: format!("{}: {}", b.name, b.type_name),
                        name: format!(
                            "{}: {} — {}:{}",
                            b.name,
                            b.type_name,
                            file_name(&b.file),
                            b.line + 1
                        ),
                        file: b.file.clone(),
                        line: b.line,
                    }
                })
                .collect();
            // A type of a library has its bean outside the project: nothing to lead to.
            if targets.is_empty() && !inj.own_type {
                continue;
            }
            let tip = match inj.candidates.as_slice() {
                [] => format!("Injects {}: no bean in the project", inj.type_name),
                [one] => format!("Injects {}: bean {}", inj.type_name, self.beans[*one].name),
                many => format!("Injects {}: {} beans fit", inj.type_name, many.len()),
            };
            match marks
                .iter_mut()
                .find(|m| m.line == inj.line && m.kind == MarkKind::Injection)
            {
                Some(m) => {
                    m.tip.push('\n');
                    m.tip.push_str(&tip);
                    for t in targets {
                        if !m.targets.contains(&t) {
                            m.targets.push(t);
                        }
                    }
                }
                None => marks.push(FrameworkMark {
                    line: inj.line,
                    kind: MarkKind::Injection,
                    tip,
                    targets,
                }),
            }
        }
        marks.sort_by_key(|m| (m.line, m.kind == MarkKind::Injection));
        marks
    }

    /// The injection points that belong to bean `index`: those of its class, or the parameters
    /// of the method that produces it.
    fn injections_of(&self, index: usize) -> impl Iterator<Item = &crate::spring::Injection> {
        let bean = &self.beans[index];
        let produced = bean.kind == "Bean";
        self.injections.iter().filter(move |i| {
            i.file == bean.file
                && match &i.producer {
                    Some(name) => produced && *name == bean.name,
                    None => !produced && i.owner == bean.type_name,
                }
        })
    }

    /// What bean `index` needs (`SPEC-0115` v5 behaviour 15): for each of its injection points
    /// the beans that fit, or the injected type marked missing when the type is the project's
    /// and none fits. A type of a library is left out.
    pub fn dependencies(&self, index: usize) -> Vec<BeanLink> {
        let mut out = Vec::new();
        for inj in self.injections_of(index) {
            if inj.candidates.is_empty() && inj.own_type {
                out.push(BeanLink {
                    name: inj.type_name.clone(),
                    type_name: inj.type_name.clone(),
                    file: inj.file.clone(),
                    line: inj.line,
                    missing: true,
                });
            }
            for &c in &inj.candidates {
                let b = &self.beans[c];
                let link = BeanLink {
                    name: b.name.clone(),
                    type_name: b.type_name.clone(),
                    file: b.file.clone(),
                    line: b.line,
                    missing: false,
                };
                if !out.contains(&link) {
                    out.push(link);
                }
            }
        }
        out
    }

    /// The beans that need bean `index`: those with an injection point it fits.
    pub fn dependents(&self, index: usize) -> Vec<BeanLink> {
        let mut out = Vec::new();
        for (other, b) in self.beans.iter().enumerate() {
            if other != index
                && self
                    .injections_of(other)
                    .any(|i| i.candidates.contains(&index))
            {
                out.push(BeanLink {
                    name: b.name.clone(),
                    type_name: b.type_name.clone(),
                    file: b.file.clone(),
                    line: b.line,
                    missing: false,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spring::model_of;

    fn files(list: &[(&str, &str)]) -> Vec<(PathBuf, String)> {
        list.iter()
            .map(|(p, t)| (PathBuf::from(p), (*t).to_owned()))
            .collect()
    }

    const SERVICE: &str = r#"import org.springframework.beans.factory.annotation.Value;
@Service
class Shop {
    @Value("${shop.name}") String name;
    @Value("${shop.currency}") String currency;
    @Value("${shop.max-items}") int max;
    @Value("${shop.debug-banner}") String banner;
    @Value("${shop.api-key}") String apiKey;
    @Value("${shop.db.password}") String password;
    @Value("${shop.timeout:30}") int timeout;
    @Value("${shop.missing}") String missing;
    @Value("${shop.name} in ${shop.currency}") String both;
}
"#;

    fn shop() -> SpringModel {
        model_of(&files(&[
            ("src/main/java/Shop.java", SERVICE),
            (
                "src/main/java/ShopProperties.java",
                "@ConfigurationProperties(prefix = \"shop\")\nclass ShopProperties {}\n",
            ),
            (
                "src/main/resources/application.properties",
                "shop.name = Corner Shop\nshop.api-key=abc123\nshop.db.password : hunter2\n",
            ),
            (
                "src/main/resources/application.yml",
                "shop:\n  currency: EUR\n  maxItems: 20\n---\nspring:\n  config:\n    activate:\n      on-profile: prod\nshop:\n  currency: USD\n",
            ),
            (
                "src/main/resources/application-dev.yml",
                "shop:\n  debug-banner: \"DEV MODE\"\n",
            ),
        ]))
    }

    fn at(m: &SpringModel, file: &str, needle: &str) -> usize {
        let v = m
            .values
            .iter()
            .find(|v| v.file.ends_with(file) && v.key == needle)
            .unwrap_or_else(|| panic!("no key {needle} in {file}"));
        v.start + 1
    }

    fn places(defs: &[KeyDefinition]) -> Vec<String> {
        defs.iter()
            .map(|d| {
                format!(
                    "{}:{}:{}{}",
                    file_name(&d.file),
                    d.line + 1,
                    d.column,
                    d.profile
                        .as_ref()
                        .map_or(String::new(), |p| format!(" [{p}]"))
                )
            })
            .collect()
    }

    /// SPEC-0115 AK-11: a key in `@Value` leads to its defining lines in `.properties` and YAML
    /// files, in a profile's file and in a profile's document; relaxed binding finds the
    /// camel-case spelling; a prefix leads to every key below it.
    #[test]
    fn keys_lead_to_their_defining_lines() {
        let m = shop();
        let java = Path::new("src/main/java/Shop.java");
        let info = |key: &str| m.key_info_at(java, at(&m, "Shop.java", key)).unwrap();
        assert_eq!(
            places(&info("shop.name").definitions),
            ["application.properties:1:0"]
        );
        // Two definitions: the chooser's rows.
        assert_eq!(
            places(&info("shop.currency").definitions),
            ["application.yml:2:2", "application.yml:10:2 [prod]"]
        );
        assert_eq!(
            places(&info("shop.max-items").definitions),
            ["application.yml:3:2"]
        );
        assert_eq!(
            places(&info("shop.debug-banner").definitions),
            ["application-dev.yml:2:2 [dev]"]
        );
        assert!(info("shop.missing").definitions.is_empty());
        let props = Path::new("src/main/java/ShopProperties.java");
        let prefix = m
            .key_info_at(props, at(&m, "ShopProperties.java", "shop"))
            .unwrap();
        assert_eq!(prefix.definitions.len(), 7);
        assert!(prefix.value.is_none());
    }

    /// SPEC-0115 AK-12: from a key of a configuration file to the places that read it, the
    /// prefix included; Find Usages from the code lists the same places.
    #[test]
    fn definitions_lead_back_to_their_usages() {
        let m = shop();
        let yml = Path::new("src/main/resources/application.yml");
        let p = m
            .properties
            .iter()
            .find(|p| p.key == "shop.maxItems")
            .unwrap();
        // The caret anywhere in `maxItems`.
        let info = m.key_info_at(yml, p.start + 3).unwrap();
        let rows: Vec<String> = info
            .usages
            .iter()
            .map(|u| {
                format!(
                    "{}:{} {}{}",
                    file_name(&u.file),
                    u.line + 1,
                    u.key,
                    if u.prefix { "*" } else { "" }
                )
            })
            .collect();
        assert_eq!(
            rows,
            ["Shop.java:6 shop.max-items", "ShopProperties.java:1 shop*"]
        );
        // The value's text is not the key.
        assert!(m.key_info_at(yml, p.end + 3).is_none());
        // `shop.name` is read twice in the code.
        let java = Path::new("src/main/java/Shop.java");
        let info = m
            .key_info_at(java, at(&m, "Shop.java", "shop.name"))
            .unwrap();
        let lines: Vec<usize> = info.usages.iter().map(|u| u.line + 1).collect();
        assert_eq!(lines, [4, 12, 1]);
        assert_eq!(m.key_info("shop.max-items").usages.len(), 2);
    }

    /// SPEC-0115 AK-13: the placeholder shows the value of the default profile, never that of a
    /// profile nothing switches on, and `•••` for a key that looks like a secret; an active
    /// profile's value wins.
    #[test]
    fn placeholders_show_resolved_values_and_mask_secrets() {
        let m = shop();
        let java = Path::new("src/main/java/Shop.java");
        let folds: Vec<(usize, String, String)> = m
            .value_folds(java)
            .into_iter()
            .map(|f| (f.line + 1, SERVICE[f.start..f.end].to_owned(), f.text))
            .collect();
        let row = |l: usize, k: &str, t: &str| (l, format!("${{{k}}}"), t.to_owned());
        assert_eq!(
            folds,
            [
                row(4, "shop.name", "Corner Shop"),
                row(5, "shop.currency", "EUR"),
                row(6, "shop.max-items", "20"),
                // `shop.debug-banner` has a value only in the profile `dev`: no fold.
                row(8, "shop.api-key", MASK),
                row(9, "shop.db.password", MASK),
                // `shop.timeout:30` and `shop.missing` are defined nowhere: no fold.
                row(12, "shop.name", "Corner Shop"),
                row(12, "shop.currency", "EUR"),
            ]
        );
        for secret in [
            "db.password",
            "API_TOKEN",
            "client-secret",
            "signingKey",
            "aws.credentials",
        ] {
            assert!(is_secret_key(secret), "{secret}");
        }
        assert!(!is_secret_key("shop.name"));
        // The MCP answer and the documentation never carry the secret.
        let info = m.key_info("shop.db.password");
        assert!(info.secret);
        assert_eq!(info.value.as_deref(), Some(MASK));
        assert_eq!(info.definitions[0].value.as_deref(), Some(MASK));
        let json = serde_json::to_string(&(&m, &info)).unwrap();
        assert!(
            !json.contains("hunter2") && !json.contains("abc123"),
            "{json}"
        );
        assert!(!SpringModel::key_doc(&info).contains("hunter2"));
        // With the profile switched on in the configuration, its value is the one shown.
        let mut sources = files(&[
            (
                "A.java",
                "@Service\nclass A { @Value(\"${a.b}\") String s; }\n",
            ),
            (
                "application.properties",
                "a.b=one\nspring.profiles.active=prod\n",
            ),
            ("application-prod.properties", "a.b=two\n"),
        ]);
        let active = model_of(&sources);
        assert_eq!(active.active_profiles(), ["prod"]);
        assert_eq!(active.value_folds(Path::new("A.java"))[0].text, "two");
        sources[1].1 = "a.b=one\n".into();
        assert_eq!(
            model_of(&sources).value_folds(Path::new("A.java"))[0].text,
            "one"
        );
    }

    /// SPEC-0115 AK-11: the quick documentation names value, defining lines and usages.
    #[test]
    fn key_documentation() {
        let m = shop();
        let doc = SpringModel::key_doc(&m.key_info("shop.currency"));
        assert_eq!(
            doc,
            "# shop.currency\n\nConfiguration key\n\nValue: `EUR`\n\n\
             Defined in `src/main/resources/application.yml:2` = `EUR`\n\n\
             Defined in `src/main/resources/application.yml:10` (profile prod) = `USD`\n\n\
             Used in 3 places"
        );
        let doc = SpringModel::key_doc(&m.key_info("shop.debug-banner"));
        assert!(doc.contains("No value in the default profile"), "{doc}");
        let doc = SpringModel::key_doc(&m.key_info("shop.nothing"));
        assert!(
            doc.contains("Not defined in the application configuration"),
            "{doc}"
        );
    }

    const WIRING: &[(&str, &str)] = &[
        (
            "src/main/java/Pay.java",
            "interface Pay {}\n@Component\nclass Card implements Pay {}\n@Component\nclass Cash implements Pay {}\n",
        ),
        (
            "src/main/java/Orders.java",
            "@Service\nclass Orders {\n    Orders(Pay pay, Clock clock,\n           Audit audit) {}\n}\ninterface Audit {}\n",
        ),
        (
            "src/main/java/Config.java",
            "@Configuration\nclass Config {\n    @Bean\n    Clock clock() { return null; }\n    @Bean\n    Report report(Orders orders) { return null; }\n}\nclass Clock {}\nclass Report {}\n",
        ),
        (
            "src/test/java/Fakes.java",
            "@TestConfiguration\nclass Fakes {\n    @Bean\n    Pay fake() { return null; }\n}\n",
        ),
    ];

    fn marks(m: &SpringModel, file: &str) -> Vec<String> {
        m.gutter_marks(Path::new(file))
            .into_iter()
            .map(|k| {
                format!(
                    "{} {:?} {} -> {}",
                    k.line + 1,
                    k.kind,
                    k.tip.replace('\n', " | "),
                    k.targets
                        .iter()
                        .map(|t| t.name.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            })
            .collect()
    }

    /// SPEC-0115 AK-14: marks on bean classes, producer methods and injection points, with the
    /// targets both ways; one target is a jump, several are the chooser's rows.
    #[test]
    fn gutter_marks_lead_both_ways() {
        let m = model_of(&files(WIRING));
        assert_eq!(
            marks(&m, "src/main/java/Pay.java"),
            [
                "3 Bean Bean card: injected in 1 place -> Orders — Orders.java:3",
                "5 Bean Bean cash: injected in 1 place -> Orders — Orders.java:3",
            ]
        );
        assert_eq!(
            marks(&m, "src/main/java/Orders.java"),
            [
                "2 Bean Bean orders: injected in 1 place -> Config — Config.java:6",
                // Two injection points on one line share a mark; the test bean is no candidate.
                "3 Injection Injects Pay: 2 beans fit | Injects Clock: bean clock -> \
                 card: Card — Pay.java:3; cash: Cash — Pay.java:5; clock: Clock — Config.java:4",
                "4 Injection Injects Audit: no bean in the project -> ",
            ]
        );
        assert_eq!(
            marks(&m, "src/main/java/Config.java"),
            [
                "2 Bean Bean config: not injected in the project's sources -> ",
                "4 Bean Bean clock: injected in 1 place -> Orders — Orders.java:3",
                "6 Bean Bean report: not injected in the project's sources -> ",
                "6 Injection Injects Orders: bean orders -> orders: Orders — Orders.java:2",
            ]
        );
    }

    /// SPEC-0134 AK-11: the same marks for CDI (Jakarta EE, Quarkus) and Micronaut beans.
    #[test]
    fn gutter_marks_of_cdi_and_micronaut() {
        let cdi = model_of(&files(&[
            (
                "Greeter.java",
                "import jakarta.enterprise.context.ApplicationScoped;\n@ApplicationScoped\nclass Greeter {}\n",
            ),
            (
                "Res.java",
                "import jakarta.inject.Inject;\nimport jakarta.ws.rs.Path;\n@Path(\"/a\")\nclass Res {\n    @Inject\n    Greeter greeter;\n}\n",
            ),
        ]));
        assert_eq!(
            marks(&cdi, "Greeter.java"),
            ["3 Bean Bean greeter: injected in 1 place -> Res — Res.java:6"]
        );
        assert_eq!(
            marks(&cdi, "Res.java"),
            [
                "4 Bean Bean res: not injected in the project's sources -> ",
                "6 Injection Injects Greeter: bean greeter -> greeter: Greeter — Greeter.java:3",
            ]
        );
        let micronaut = model_of(&files(&[
            (
                "F.java",
                "import io.micronaut.context.annotation.Factory;\n@Factory\nclass F {\n    @Singleton\n    Store store() { return null; }\n}\nclass Store {}\n",
            ),
            (
                "C.java",
                "import io.micronaut.http.annotation.Controller;\n@Controller(\"/c\")\nclass C {\n    C(Store store) {}\n}\n",
            ),
        ]));
        assert_eq!(
            marks(&micronaut, "F.java"),
            [
                "3 Bean Bean f: not injected in the project's sources -> ",
                "5 Bean Bean store: injected in 1 place -> C — C.java:4",
            ]
        );
        assert_eq!(
            marks(&micronaut, "C.java")[1],
            "4 Injection Injects Store: bean store -> store: Store — F.java:5"
        );
    }

    /// SPEC-0134 AK-12: `@ConfigProperty`, `@Property`, Micronaut's `@Value` and the prefixes of
    /// `@ConfigMapping` and `@ConfigurationProperties` lead to their defining lines, a `%dev.`
    /// key is a definition of the profile `dev`, and the definitions lead back.
    #[test]
    fn keys_of_quarkus_and_micronaut() {
        let quarkus = model_of(&files(&[
            (
                "R.java",
                "import org.eclipse.microprofile.config.inject.ConfigProperty;\n@ApplicationScoped\nclass R {\n    @ConfigProperty(name = \"greeting.message\") String m;\n    @ConfigProperty(name = \"greeting.debug\") String d;\n}\n",
            ),
            (
                "G.java",
                "@ConfigMapping(prefix = \"greeting\")\ninterface G {}\n",
            ),
            (
                "application.properties",
                "greeting.message=hello\n%dev.greeting.debug=true\n%dev.greeting.message=hi\n",
            ),
        ]));
        let info = quarkus.key_info("greeting.message");
        assert_eq!(
            places(&info.definitions),
            [
                "application.properties:1:0",
                "application.properties:3:0 [dev]"
            ]
        );
        assert_eq!(info.value.as_deref(), Some("hello"));
        assert_eq!(info.usages.len(), 2);
        // Only the profile `dev` defines it: no value for the default profile.
        let debug = quarkus.key_info("greeting.debug");
        assert_eq!(
            places(&debug.definitions),
            ["application.properties:2:0 [dev]"]
        );
        assert!(debug.value.is_none());
        // From the line `%dev.greeting.debug` back to the code.
        let p = &quarkus.properties[1];
        let back = quarkus
            .key_info_at(Path::new("application.properties"), p.start)
            .unwrap();
        assert_eq!(back.key, "greeting.debug");
        let lines: Vec<String> = back
            .usages
            .iter()
            .map(|u| format!("{}:{}", file_name(&u.file), u.line + 1))
            .collect();
        assert_eq!(lines, ["R.java:5", "G.java:1"]);
        // `@ConfigProperty` is no placeholder: nothing is folded.
        assert!(quarkus.value_folds(Path::new("R.java")).is_empty());

        let micronaut = model_of(&files(&[
            (
                "C.java",
                "import io.micronaut.context.annotation.Value;\n@Singleton\nclass C {\n    @Value(\"${notes.page-size}\") int size;\n    @Property(name = \"notes.owner\") String owner;\n}\n",
            ),
            (
                "N.java",
                "@ConfigurationProperties(\"notes\")\nclass N {}\n",
            ),
            ("application.yml", "notes:\n  pageSize: 25\n  owner: me\n"),
        ]));
        let c = Path::new("C.java");
        let size = micronaut
            .key_info_at(c, at(&micronaut, "C.java", "notes.page-size"))
            .unwrap();
        assert_eq!(places(&size.definitions), ["application.yml:2:2"]);
        assert_eq!(size.value.as_deref(), Some("25"));
        let owner = micronaut
            .key_info_at(c, at(&micronaut, "C.java", "notes.owner"))
            .unwrap();
        assert_eq!(places(&owner.definitions), ["application.yml:3:2"]);
        assert_eq!(owner.usages.len(), 2);
        assert_eq!(micronaut.value_folds(c)[0].text, "25");
        let prefix = micronaut
            .key_info_at(Path::new("N.java"), at(&micronaut, "N.java", "notes"))
            .unwrap();
        assert_eq!(prefix.definitions.len(), 2);
    }

    /// SPEC-0115 AK-15: what a bean needs and what needs it; test beans are marked.
    #[test]
    fn dependencies_and_dependents() {
        let m = model_of(&files(WIRING));
        let index = |name: &str| m.beans.iter().position(|b| b.name == name).unwrap();
        let names = |links: Vec<BeanLink>| -> Vec<String> {
            links
                .into_iter()
                .map(|l| format!("{}{}", l.name, if l.missing { "?" } else { "" }))
                .collect()
        };
        assert_eq!(
            names(m.dependencies(index("orders"))),
            ["card", "cash", "clock", "Audit?"]
        );
        assert_eq!(names(m.dependents(index("orders"))), ["report"]);
        assert_eq!(names(m.dependencies(index("report"))), ["orders"]);
        assert_eq!(names(m.dependents(index("clock"))), ["orders"]);
        // The configuration class itself needs nothing: the parameters belong to its methods.
        assert!(m.dependencies(index("config")).is_empty());
        let tests: Vec<&str> = m
            .beans
            .iter()
            .filter(|b| b.test)
            .map(|b| b.name.as_str())
            .collect();
        assert_eq!(tests, ["fakes", "fake"]);
    }
}
