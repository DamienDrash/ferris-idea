//! Bundled New UI themes and editor colour schemes (`SPEC-0060`, ADR-0009, ADR-0018).
//!
//! The JSON and XML files are unchanged upstream assets (`themes/README.md`), except the IDECraft
//! brand themes, which are own files over the Islands themes; this module is own code that reads
//! them. Key fallbacks follow the open `DefaultLanguageHighlighterColors`, the lookup order
//! (scheme chain first, then fallback key) `AbstractColorsScheme`/`EditorColorsSchemeImpl` (logic
//! templates).

use std::collections::BTreeMap;

use quick_xml::Reader;
use quick_xml::events::Event;
use serde::Serialize;

/// Bundled dark New UI theme.
pub const DARK_THEME_JSON: &str = include_str!("../themes/expUI_dark.theme.json");
/// Bundled light New UI theme.
pub const LIGHT_THEME_JSON: &str = include_str!("../themes/expUI_light.theme.json");
/// Editor scheme `Dark` (parent `Darcula`).
pub const DARK_SCHEME_XML: &str = include_str!("../themes/expUI_darkScheme.xml");
/// Editor scheme `Light` (parent `Default`).
pub const LIGHT_SCHEME_XML: &str = include_str!("../themes/expUI_lightScheme.xml");
/// Base schemes `Default` and `Darcula`.
pub const BASE_SCHEMES_XML: &str = include_str!("../themes/DefaultColorSchemesManager.xml");
/// Python colours added to the base scheme `Darcula` (the Python plugin's additional text attributes,
/// `SPEC-0027` v6).
pub const PYTHON_DARCULA_XML: &str = include_str!("../themes/PythonDarcula.xml");
/// Python colours added to the base scheme `Default`.
pub const PYTHON_DEFAULT_XML: &str = include_str!("../themes/PythonDefault.xml");
/// Islands Dark theme (parent `ExperimentalDark`, the New UI dark theme), the reference's default.
pub const ISLANDS_DARK_THEME_JSON: &str = include_str!("../themes/ManyIslandsDark.theme.json");
/// Editor scheme `Islands Dark` (parent `Darcula`).
pub const ISLANDS_DARK_SCHEME_XML: &str = include_str!("../themes/IslandSchemeDark.xml");
/// Islands Light theme (parent `ExperimentalLightWithLightHeader`, editor scheme `Light`).
pub const ISLANDS_LIGHT_THEME_JSON: &str = include_str!("../themes/ManyIslandsLight.theme.json");
/// New UI `Light with Light Header` (parent `ExperimentalLight`), the parent of Islands Light.
pub const LIGHT_HEADER_THEME_JSON: &str =
    include_str!("../themes/expUI_light_with_light_header.theme.json");

/// IDECraft Dark (own file, `SPEC-0060` v6): brand colours over `Islands Dark`.
pub const IDECRAFT_DARK_THEME_JSON: &str = include_str!("../themes/IDECraftDark.theme.json");
/// Editor scheme `IDECraft Dark` (parent `Islands Dark`).
pub const IDECRAFT_DARK_SCHEME_XML: &str = include_str!("../themes/IDECraftDarkScheme.xml");
/// IDECraft Light (own file, `SPEC-0060` v6): brand colours over `Islands Light`.
pub const IDECRAFT_LIGHT_THEME_JSON: &str = include_str!("../themes/IDECraftLight.theme.json");
/// Editor scheme `IDECraft Light` (parent `Light`).
pub const IDECRAFT_LIGHT_SCHEME_XML: &str = include_str!("../themes/IDECraftLightScheme.xml");

/// The bundled look-and-feel variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ThemeId {
    /// `IDECraft Dark` with scheme `IDECraft Dark`: the brand palette on the Islands layout, the
    /// default for new installs (`SPEC-0060` v6, ADR-0018).
    IdecraftDark,
    /// `IDECraft Light` with scheme `IDECraft Light`: the brand's light counterpart.
    IdecraftLight,
    /// New UI `Dark` with scheme `Dark`.
    Dark,
    /// New UI `Light` with scheme `Light`.
    Light,
    /// `Islands Dark` with scheme `Islands Dark`, the reference's default.
    IslandsDark,
    /// `Islands Light` with scheme `Light`, the reference's default light theme.
    IslandsLight,
}

impl ThemeId {
    /// Every bundled theme, in the order the theme popup lists them; the default comes first.
    pub const ALL: [ThemeId; 6] = [
        ThemeId::IdecraftDark,
        ThemeId::IdecraftLight,
        ThemeId::IslandsDark,
        ThemeId::IslandsLight,
        ThemeId::Dark,
        ThemeId::Light,
    ];

    /// The theme of a new install, and of an install whose saved choice is missing or unknown
    /// (`SPEC-0060` v6).
    pub const DEFAULT: ThemeId = ThemeId::IdecraftDark;

    /// The theme to draw with at start-up: the saved choice when it names a bundled theme, else
    /// [`ThemeId::DEFAULT`]. A choice the user made is never replaced (`SPEC-0060` v6).
    pub fn from_saved(saved: Option<&str>) -> ThemeId {
        saved.and_then(Self::from_name).unwrap_or(Self::DEFAULT)
    }

    /// Whether the theme carries the IDECraft brand palette (ADR-0018).
    pub fn is_brand(self) -> bool {
        matches!(self, ThemeId::IdecraftDark | ThemeId::IdecraftLight)
    }

    /// The name shown for the theme (the reference's name for the themes taken from it).
    pub fn name(self) -> &'static str {
        match self {
            ThemeId::IdecraftDark => "IDECraft Dark",
            ThemeId::IdecraftLight => "IDECraft Light",
            ThemeId::Dark => "Dark",
            ThemeId::Light => "Light",
            ThemeId::IslandsDark => "Islands Dark",
            ThemeId::IslandsLight => "Islands Light",
        }
    }

    /// The theme called `name` (as [`ThemeId::name`] gives it, ignoring case).
    pub fn from_name(name: &str) -> Option<ThemeId> {
        Self::ALL
            .into_iter()
            .find(|t| t.name().eq_ignore_ascii_case(name.trim()))
    }

    /// Whether the theme is dark (icons use their dark variant).
    pub fn is_dark(self) -> bool {
        !matches!(
            self,
            ThemeId::Light | ThemeId::IslandsLight | ThemeId::IdecraftLight
        )
    }
}

/// Why a theme or scheme could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ThemeError {
    /// Malformed XML.
    #[error("invalid colour scheme XML: {0}")]
    Xml(String),
    /// Malformed JSON.
    #[error("invalid theme JSON: {0}")]
    Json(String),
    /// A scheme names a parent that is not bundled.
    #[error("unknown parent scheme `{0}`")]
    Parent(String),
}

/// Text attributes as a scheme defines them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TextStyle {
    /// Text colour (0xRRGGBB).
    pub foreground: Option<u32>,
    /// Background colour.
    pub background: Option<u32>,
    /// Bold (font type bit 1).
    pub bold: bool,
    /// Italic (font type bit 2).
    pub italic: bool,
    /// Colour of the effect (underline, wave, box), e.g. for errors and warnings.
    pub effect: Option<u32>,
    /// Colour of the mark in the editor's error stripe (`ERROR_STRIPE_COLOR`, SPEC-0070 v5).
    pub stripe: Option<u32>,
    /// The effect is a line under the text (`EFFECT_TYPE` 1), drawn in the effect colour, e.g.
    /// for reassigned variables (`SPEC-0027` v11).
    pub underline: bool,
}

#[derive(Debug, Clone)]
enum Attr {
    Value(TextStyle),
    Base(String),
}

/// One scheme as written in a file.
#[derive(Debug, Clone)]
pub struct SchemeFile {
    /// Scheme name.
    pub name: String,
    /// Parent scheme name.
    pub parent: Option<String>,
    colors: BTreeMap<String, Option<u32>>,
    attributes: BTreeMap<String, Attr>,
    /// Colours that could not be read and were left out (`SPEC-0060` v10 behaviour 15).
    notes: Vec<String>,
}

/// A colour value of a scheme: `None` for an empty value (the scheme sets no colour there),
/// `Some(Err)` for one that is no hexadecimal colour.
fn scheme_colour(value: Option<&str>) -> Option<Result<u32, String>> {
    let v = value?.trim();
    if v.trim_start_matches('#').is_empty() {
        return None;
    }
    Some(hex(v).ok_or_else(|| v.to_owned()))
}

fn hex(value: &str) -> Option<u32> {
    let v = value.trim().trim_start_matches('#');
    if v.is_empty() {
        return None;
    }
    // Upstream also writes short values such as `0` (black).
    u32::from_str_radix(v, 16).ok().map(|c| c & 0xff_ffff)
}

/// Reads every `<scheme>` in `xml` (a scheme file or `DefaultColorSchemesManager.xml`).
pub fn parse_schemes(xml: &str) -> Result<Vec<SchemeFile>, ThemeError> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut current: Option<SchemeFile> = None;
    let (mut in_colors, mut in_attributes) = (false, false);
    let mut attr_name: Option<String> = None;
    let mut style = TextStyle::default();
    loop {
        let event = reader
            .read_event()
            .map_err(|e| ThemeError::Xml(e.to_string()))?;
        let (start, empty) = match &event {
            Event::Start(e) => (Some(e.clone()), false),
            Event::Empty(e) => (Some(e.clone()), true),
            _ => (None, false),
        };
        if let Some(e) = start {
            let mut attrs = BTreeMap::new();
            for a in e.attributes().flatten() {
                attrs.insert(
                    String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                    a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|e| ThemeError::Xml(e.to_string()))?
                        .into_owned(),
                );
            }
            match e.name().as_ref() {
                b"scheme" => {
                    current = Some(SchemeFile {
                        name: attrs.get("name").cloned().unwrap_or_default(),
                        parent: attrs.get("parent_scheme").cloned(),
                        colors: BTreeMap::new(),
                        attributes: BTreeMap::new(),
                        notes: Vec::new(),
                    });
                }
                // An additional attributes file (`<list>` of attribute options, no scheme name).
                b"list" if !empty => {
                    current = Some(SchemeFile {
                        name: String::new(),
                        parent: None,
                        colors: BTreeMap::new(),
                        attributes: BTreeMap::new(),
                        notes: Vec::new(),
                    });
                    in_attributes = true;
                }
                b"colors" => in_colors = !empty,
                b"attributes" => in_attributes = !empty,
                b"option" => {
                    let name = attrs.get("name").cloned().unwrap_or_default();
                    let value = attrs.get("value").map(String::as_str);
                    if let Some(scheme) = current.as_mut() {
                        if in_colors {
                            // A value that is no colour is left out, so the parent scheme's
                            // colour counts (behaviour 15); an empty value sets "no colour".
                            match scheme_colour(value) {
                                Some(Err(bad)) => scheme.notes.push(format!(
                                    "scheme `{}`: `{name}` has the malformed colour `{bad}`; \
                                     the inherited colour is used",
                                    scheme.name
                                )),
                                colour => {
                                    scheme.colors.insert(name, colour.and_then(Result::ok));
                                }
                            }
                        } else if in_attributes && attr_name.is_some() {
                            let mut colour = |slot: &mut Option<u32>| match scheme_colour(value) {
                                Some(Err(bad)) => scheme.notes.push(format!(
                                    "scheme `{}`: `{}` has the malformed {name} `{bad}`; \
                                     it is left out",
                                    scheme.name,
                                    attr_name.as_deref().unwrap_or_default()
                                )),
                                c => *slot = c.and_then(Result::ok),
                            };
                            match name.as_str() {
                                "FOREGROUND" => colour(&mut style.foreground),
                                "BACKGROUND" => colour(&mut style.background),
                                "EFFECT_COLOR" => colour(&mut style.effect),
                                "ERROR_STRIPE_COLOR" => colour(&mut style.stripe),
                                "EFFECT_TYPE" => style.underline = value == Some("1"),
                                "FONT_TYPE" => {
                                    let t: u8 = value.and_then(|v| v.parse().ok()).unwrap_or(0);
                                    style.bold = t & 1 != 0;
                                    style.italic = t & 2 != 0;
                                }
                                _ => {}
                            }
                        } else if in_attributes {
                            if let Some(base) = attrs.get("baseAttributes") {
                                scheme.attributes.insert(name, Attr::Base(base.clone()));
                            } else if empty {
                                scheme
                                    .attributes
                                    .insert(name, Attr::Value(TextStyle::default()));
                            } else {
                                attr_name = Some(name);
                                style = TextStyle::default();
                            }
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        match event {
            Event::End(e) => match e.name().as_ref() {
                b"colors" => in_colors = false,
                b"attributes" => in_attributes = false,
                b"option" if in_attributes => {
                    // Only the closing tag of an attribute (its inner options are empty elements).
                    if let (Some(name), Some(scheme)) = (attr_name.take(), current.as_mut()) {
                        scheme
                            .attributes
                            .insert(name, Attr::Value(std::mem::take(&mut style)));
                    }
                }
                b"scheme" => {
                    if let Some(s) = current.take() {
                        out.push(s);
                    }
                }
                b"list" => {
                    in_attributes = false;
                    if let Some(s) = current.take() {
                        out.push(s);
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// Fallback keys of the default highlighter colours (behaviour 3; logic template
/// `DefaultLanguageHighlighterColors`). `TEXT` is the plain text key.
fn fallback(key: &str) -> Option<&'static str> {
    Some(match key {
        "DEFAULT_IDENTIFIER"
        | "DEFAULT_METADATA"
        | "DEFAULT_TAG"
        | "DEFAULT_TEMPLATE_LANGUAGE_COLOR" => "TEXT",
        "DEFAULT_LABEL"
        | "DEFAULT_CONSTANT"
        | "DEFAULT_LOCAL_VARIABLE"
        | "DEFAULT_GLOBAL_VARIABLE"
        | "DEFAULT_FUNCTION_DECLARATION"
        | "DEFAULT_FUNCTION_CALL"
        | "DEFAULT_PARAMETER"
        | "DEFAULT_CLASS_NAME"
        | "DEFAULT_INTERFACE_NAME"
        | "DEFAULT_CLASS_REFERENCE"
        | "DEFAULT_INSTANCE_FIELD"
        | "DEFAULT_STATIC_FIELD"
        | "DEFAULT_PREDEFINED_SYMBOL"
        | "DEFAULT_ATTRIBUTE"
        | "DEFAULT_ENTITY" => "DEFAULT_IDENTIFIER",
        "DEFAULT_REASSIGNED_LOCAL_VARIABLE" => "DEFAULT_LOCAL_VARIABLE",
        "DEFAULT_REASSIGNED_PARAMETER" => "DEFAULT_PARAMETER",
        "DEFAULT_INSTANCE_METHOD" | "DEFAULT_STATIC_METHOD" => "DEFAULT_FUNCTION_DECLARATION",
        "DEFAULT_HIGHLIGHTED_REFERENCE" => "DEFAULT_STRING",
        // Java keys of semantic highlighting (SPEC-0290 behaviour 3; logic template
        // `JavaHighlightingColors`).
        "METHOD_DECLARATION_ATTRIBUTES" => "DEFAULT_FUNCTION_DECLARATION",
        "METHOD_CALL_ATTRIBUTES" => "DEFAULT_FUNCTION_CALL",
        "STATIC_METHOD_ATTRIBUTES" => "DEFAULT_STATIC_METHOD",
        "STATIC_FIELD_ATTRIBUTES" | "STATIC_FINAL_FIELD_ATTRIBUTES" => "DEFAULT_STATIC_FIELD",
        "INSTANCE_FIELD_ATTRIBUTES" => "DEFAULT_INSTANCE_FIELD",
        "PARAMETER_ATTRIBUTES" | "TYPE_PARAMETER_NAME_ATTRIBUTES" => "DEFAULT_PARAMETER",
        "LOCAL_VARIABLE_ATTRIBUTES" => "DEFAULT_LOCAL_VARIABLE",
        "INTERFACE_NAME_ATTRIBUTES" => "DEFAULT_INTERFACE_NAME",
        "CLASS_NAME_ATTRIBUTES" => "DEFAULT_CLASS_NAME",
        "ANNOTATION_NAME_ATTRIBUTES" => "DEFAULT_METADATA",
        // Python (SPEC-0027 v6; logic template: the Python plugin's `PyHighlighter`).
        "PY.DECORATOR" => "DEFAULT_METADATA",
        "PY.SELF_PARAMETER" | "PY.PARAMETER" | "PY.KEYWORD_ARGUMENT" => "DEFAULT_PARAMETER",
        "PY.FUNC_DEFINITION" => "DEFAULT_FUNCTION_DECLARATION",
        "PY.NESTED_FUNC_DEFINITION" => "PY.FUNC_DEFINITION",
        "PY.CLASS_DEFINITION" => "DEFAULT_CLASS_NAME",
        "PY.DOC_COMMENT" => "DEFAULT_DOC_COMMENT",
        "PY.BUILTIN_NAME" | "PY.PREDEFINED_DEFINITION" | "PY.PREDEFINED_USAGE" => {
            "DEFAULT_PREDEFINED_SYMBOL"
        }
        // JavaScript/TypeScript and SQL: neutral fallbacks of IDECraft's own (SPEC-0027 v6).
        "JS.GLOBAL_VARIABLE" => "DEFAULT_GLOBAL_VARIABLE",
        "JS.REGEXP" => "DEFAULT_STRING",
        "SQL_COLUMN" => "DEFAULT_INSTANCE_FIELD",
        // Always italic, see `ALWAYS_ITALIC` (SPEC-0027 v7).
        "SQL_BUILTIN_FUNCTION" => "DEFAULT_FUNCTION_DECLARATION",
        // Markdown (logic template `MarkdownHighlighterColors`).
        "MARKDOWN_HEADER_LEVEL_1" | "MARKDOWN_CODE_FENCE_LANGUAGE" => "DEFAULT_CONSTANT",
        "MARKDOWN_LINK_LABEL" => "DEFAULT_KEYWORD",
        "MARKDOWN_CODE_FENCE" | "MARKDOWN_CODE_SPAN" | "MARKDOWN_BLOCK_QUOTE" => "DEFAULT_STRING",
        "MARKDOWN_LINK_DESTINATION" => "DEFAULT_STATIC_METHOD",
        _ => return None,
    })
}

/// The base schemes `Default` and `Darcula` with the additional attributes of language plugins:
/// an additional file adds the keys its base scheme does not define yet (`SPEC-0027` v6; logic
/// template: the reference's `additionalTextAttributes` extension).
fn base_schemes() -> Vec<SchemeFile> {
    let mut bases = parse_schemes(BASE_SCHEMES_XML).expect("bundled schemes parse");
    for (base, xml) in [
        ("Default", PYTHON_DEFAULT_XML),
        ("Darcula", PYTHON_DARCULA_XML),
    ] {
        let extra = parse_schemes(xml).expect("bundled attributes parse");
        if let Some(scheme) = bases.iter_mut().find(|s| s.name == base) {
            for (key, attr) in extra.into_iter().flat_map(|e| e.attributes) {
                scheme.attributes.entry(key).or_insert(attr);
            }
        }
    }
    bases
}

/// IDECraft's own keys that are italic whatever their colour comes from: SQL built-in functions take
/// the function-declaration colour when a scheme does not define them, but stay italic (`SPEC-0027`
/// v7).
const ALWAYS_ITALIC: [&str; 1] = ["SQL_BUILTIN_FUNCTION"];

/// An editor colour scheme with its parents resolved.
#[derive(Debug, Clone)]
pub struct ColorScheme {
    /// Scheme name, e.g. `Dark`.
    pub name: String,
    /// The scheme first, then its parents.
    chain: Vec<SchemeFile>,
}

impl ColorScheme {
    /// Resolves `scheme` against the schemes in `bases` (behaviour 2).
    pub fn resolve(scheme: SchemeFile, bases: &[SchemeFile]) -> Result<Self, ThemeError> {
        let mut chain = vec![scheme];
        while let Some(parent) = chain.last().and_then(|s| s.parent.clone()) {
            let base = bases
                .iter()
                .find(|b| b.name == parent)
                .ok_or_else(|| ThemeError::Parent(parent.clone()))?;
            if chain.iter().any(|s| s.name == base.name) {
                break;
            }
            chain.push(base.clone());
        }
        Ok(Self {
            name: chain[0].name.clone(),
            chain,
        })
    }

    /// The bundled `Dark` or `Light` scheme.
    pub fn bundled(dark: bool) -> Self {
        let bases = base_schemes();
        let xml = if dark {
            DARK_SCHEME_XML
        } else {
            LIGHT_SCHEME_XML
        };
        let scheme = parse_schemes(xml).expect("bundled scheme parses").remove(0);
        Self::resolve(scheme, &bases).expect("bundled parents exist")
    }

    /// The editor scheme of a bundled theme.
    pub fn for_theme(id: ThemeId) -> Self {
        match id {
            ThemeId::Dark => Self::bundled(true),
            ThemeId::Light | ThemeId::IslandsLight => Self::bundled(false),
            ThemeId::IslandsDark => Self::bundled_over(ISLANDS_DARK_SCHEME_XML, &[]),
            ThemeId::IdecraftDark => {
                Self::bundled_over(IDECRAFT_DARK_SCHEME_XML, &[ISLANDS_DARK_SCHEME_XML])
            }
            ThemeId::IdecraftLight => {
                Self::bundled_over(IDECRAFT_LIGHT_SCHEME_XML, &[LIGHT_SCHEME_XML])
            }
        }
    }

    /// The bundled scheme in `xml`, resolved against the base schemes and the schemes in `more`.
    fn bundled_over(xml: &str, more: &[&str]) -> Self {
        let mut bases = base_schemes();
        for m in more {
            bases.extend(parse_schemes(m).expect("bundled scheme parses"));
        }
        let scheme = parse_schemes(xml).expect("bundled scheme parses").remove(0);
        Self::resolve(scheme, &bases).expect("bundled parents exist")
    }

    /// Names in the chain, scheme first.
    pub fn chain(&self) -> Vec<&str> {
        self.chain.iter().map(|s| s.name.as_str()).collect()
    }

    /// The colours of the chain's files that could not be read (behaviour 15), scheme first.
    pub fn notes(&self) -> Vec<String> {
        self.chain.iter().flat_map(|s| s.notes.clone()).collect()
    }

    /// An editor colour such as `CARET_ROW_COLOR` (behaviour 2).
    pub fn color(&self, key: &str) -> Option<u32> {
        self.chain
            .iter()
            .find_map(|s| s.colors.get(key))
            .copied()
            .flatten()
    }

    /// Text attributes of `key`: defined in the scheme or a parent, else through `baseAttributes`,
    /// else through the key's fallback (behaviour 3). The root scheme of the chain (`Default`, the
    /// light base) counts last: a fallback that a more specific scheme defines beats a key only the
    /// root defines, so Java fields in a dark scheme take its field colour, not the light one (v9,
    /// OBS-0023 L24).
    pub fn attributes(&self, key: &str) -> TextStyle {
        let mut style = self.resolve_key(key, 0).map(|(s, _)| s).unwrap_or_default();
        if ALWAYS_ITALIC.contains(&key) {
            style.italic = true;
        }
        style
    }

    /// The style of `key` and whether it came from the root scheme only.
    fn resolve_key(&self, key: &str, depth: usize) -> Option<(TextStyle, bool)> {
        if depth > 16 {
            return None;
        }
        let (root, specific) = self.chain.split_last()?;
        if let Some(a) = specific.iter().find_map(|s| s.attributes.get(key)) {
            return match a {
                Attr::Value(v) => Some((v.clone(), false)),
                Attr::Base(b) => self.resolve_key(b, depth + 1),
            };
        }
        let fallen = fallback(key).and_then(|f| self.resolve_key(f, depth + 1));
        let own = root.attributes.get(key);
        // A root value without colours (only a font style or an effect, e.g. the italic of
        // predefined symbols) cannot clash with a dark background, so it counts as in the
        // reference (SPEC-0027 v6).
        let colourless =
            matches!(own, Some(Attr::Value(v)) if v.foreground.is_none() && v.background.is_none());
        match (fallen, own) {
            (Some((v, false)), _) if !colourless => Some((v, false)),
            (_, Some(Attr::Value(v))) => Some((v.clone(), true)),
            (_, Some(Attr::Base(b))) => self.resolve_key(b, depth + 1).map(|(v, _)| (v, true)),
            (fallen, None) => fallen,
        }
    }

    /// Style of a syntax capture: its attribute key's style over the plain text style.
    pub fn capture_style(&self, kind: &str) -> TextStyle {
        let text = self.attributes("TEXT");
        let s = self.attributes(capture_key(kind));
        TextStyle {
            foreground: s.foreground.or(text.foreground),
            background: s.background,
            bold: s.bold,
            italic: s.italic,
            effect: s.effect,
            stripe: s.stripe,
            underline: s.underline,
        }
    }
}

/// The suffixes of language-specific capture names (`SPEC-0027` v6): Python, JavaScript and
/// TypeScript (`js`), SQL.
const ROLE_LANGUAGES: [&str; 3] = ["python", "js", "sql"];

/// The key of a language's own colour role (`SPEC-0027` v6). Python keys are the Python plugin's;
/// the JavaScript keys are those the bundled schemes define; SQL uses a neutral column key.
fn language_key(lang: &str, role: &str) -> Option<&'static str> {
    Some(match (lang, role) {
        ("python", "attribute") => "PY.DECORATOR",
        ("python", "variable.builtin.self") => "PY.SELF_PARAMETER",
        ("python", "function") => "PY.FUNC_DEFINITION",
        ("python", "comment.doc") => "PY.DOC_COMMENT",
        ("js", "variable.global") => "JS.GLOBAL_VARIABLE",
        ("js", "string.regex") => "JS.REGEXP",
        ("sql", "property.column") => "SQL_COLUMN",
        ("sql", "function.builtin") => "SQL_BUILTIN_FUNCTION",
        // An alias where it is used is plain text.
        ("sql", "alias") => "TEXT",
        _ => return None,
    })
}

/// Attribute key of a tree-sitter capture name (behaviour 4).
///
/// A language-specific capture `<role>.<lang>` (`SPEC-0027` v6, e.g. `attribute.python`) takes the
/// language's own key; the key falls back to the neutral one, so a scheme without it still shows
/// the neutral colour. A role the language has no own key for maps like the plain role.
pub fn capture_key(kind: &str) -> &'static str {
    if let Some((role, lang)) = kind.rsplit_once('.')
        && ROLE_LANGUAGES.contains(&lang)
    {
        return language_key(lang, role).unwrap_or_else(|| capture_key(role));
    }
    // Reassigned Java locals and parameters (`SPEC-0027` v11).
    match kind {
        "variable.reassigned" => return "DEFAULT_REASSIGNED_LOCAL_VARIABLE",
        "variable.parameter.reassigned" => return "DEFAULT_REASSIGNED_PARAMETER",
        _ => {}
    }
    let mut parts = kind.split('.');
    let head = parts.next().unwrap_or_default();
    let sub = parts.next().unwrap_or_default();
    match (head, sub) {
        ("keyword" | "boolean" | "conditional" | "repeat" | "include" | "storage", _) => {
            "DEFAULT_KEYWORD"
        }
        ("constant", "builtin") | ("variable", "builtin") => "DEFAULT_KEYWORD",
        ("string", "escape") | ("escape", _) => "DEFAULT_VALID_STRING_ESCAPE",
        ("string" | "character", _) => "DEFAULT_STRING",
        ("comment", "documentation" | "doc") => "DEFAULT_DOC_COMMENT",
        ("comment", "block") => "DEFAULT_BLOCK_COMMENT",
        ("comment", _) => "DEFAULT_LINE_COMMENT",
        ("number" | "float", _) => "DEFAULT_NUMBER",
        ("function", "call") => "DEFAULT_FUNCTION_CALL",
        ("function", "method") | ("method", _) => "DEFAULT_INSTANCE_METHOD",
        ("function", "macro") => "DEFAULT_PREDEFINED_SYMBOL",
        ("function" | "constructor", _) => "DEFAULT_FUNCTION_DECLARATION",
        ("type", "parameter") => "TYPE_PARAMETER_NAME_ATTRIBUTES",
        ("type", _) => "DEFAULT_CLASS_NAME",
        ("variable", "parameter") | ("parameter", _) => "DEFAULT_PARAMETER",
        ("variable", _) => "DEFAULT_LOCAL_VARIABLE",
        // A static field (Java fields declared `static`, interface fields, enum constants;
        // `SPEC-0027` v10).
        ("property" | "field", "static") => "DEFAULT_STATIC_FIELD",
        ("property" | "field", _) => "DEFAULT_INSTANCE_FIELD",
        ("constant", _) => "DEFAULT_CONSTANT",
        ("attribute" | "annotation", _) => "DEFAULT_METADATA",
        ("label", _) => "DEFAULT_LABEL",
        ("operator", _) => "DEFAULT_OPERATION_SIGN",
        ("punctuation", "bracket") => "DEFAULT_BRACES",
        ("punctuation", "delimiter") => "DEFAULT_COMMA",
        ("tag", _) => "DEFAULT_TAG",
        ("embedded", _) => "DEFAULT_TEMPLATE_LANGUAGE_COLOR",
        // Markdown block grammar.
        ("text", "title") => "MARKDOWN_HEADER_LEVEL_1",
        ("text", "literal") => "MARKDOWN_CODE_FENCE",
        ("text", "uri") => "MARKDOWN_LINK_DESTINATION",
        ("text", "reference") => "MARKDOWN_LINK_LABEL",
        _ => "TEXT",
    }
}

/// Which keys a file's semantic tokens take (`SPEC-0290` behaviour 3, v2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyFamily {
    /// Java files: the Java keys.
    Java,
    /// Kotlin files: the Java keys too, so a Kotlin function, property or parameter has the role
    /// of its Java counterpart (`SPEC-0290` v3).
    Kotlin,
    /// JavaScript and TypeScript files: the neutral keys, except the roles of `SPEC-0027` v6 the
    /// server must not downgrade (global objects, type parameters).
    Script,
    /// Every other file: the neutral keys.
    Neutral,
}

impl KeyFamily {
    /// The family of a file by its name or path.
    pub fn of_path(path: &str) -> Self {
        let ext = path.rsplit_once('.').map_or("", |(_, e)| e);
        match ext.to_ascii_lowercase().as_str() {
            "java" => Self::Java,
            "kt" | "kts" => Self::Kotlin,
            "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" => Self::Script,
            _ => Self::Neutral,
        }
    }
}

/// Colour-scheme key of a language server's semantic token (`SPEC-0290` behaviour 3): from its type
/// and modifiers, the Java key in Java files, else the language-neutral key; `None` keeps the syntax
/// colour.
pub fn semantic_key(token_type: &str, modifiers: &[&str], java: bool) -> Option<&'static str> {
    let family = if java {
        KeyFamily::Java
    } else {
        KeyFamily::Neutral
    };
    semantic_key_in(token_type, modifiers, family)
}

/// [`semantic_key`] for a file of `family`: in JavaScript and TypeScript a `variable`/`property`
/// with `defaultLibrary` is a global object and a `typeParameter` keeps the type-parameter key, as
/// their syntax roles do (`SPEC-0290` v2).
pub fn semantic_key_in(
    token_type: &str,
    modifiers: &[&str],
    family: KeyFamily,
) -> Option<&'static str> {
    let has = |m: &str| modifiers.contains(&m);
    if family == KeyFamily::Script {
        match token_type {
            "variable" | "property" if has("defaultLibrary") => return Some("JS.GLOBAL_VARIABLE"),
            "typeParameter" => return Some("TYPE_PARAMETER_NAME_ATTRIBUTES"),
            _ => {}
        }
    }
    let java = matches!(family, KeyFamily::Java | KeyFamily::Kotlin);
    let (java_key, neutral) = match token_type {
        "method" | "function" if has("declaration") => (
            "METHOD_DECLARATION_ATTRIBUTES",
            "DEFAULT_FUNCTION_DECLARATION",
        ),
        "method" | "function" if has("static") => {
            ("STATIC_METHOD_ATTRIBUTES", "DEFAULT_STATIC_METHOD")
        }
        "method" | "function" => ("METHOD_CALL_ATTRIBUTES", "DEFAULT_FUNCTION_CALL"),
        "property" | "recordComponent" if has("static") && has("readonly") => {
            ("STATIC_FINAL_FIELD_ATTRIBUTES", "DEFAULT_STATIC_FIELD")
        }
        "enumMember" => ("STATIC_FINAL_FIELD_ATTRIBUTES", "DEFAULT_STATIC_FIELD"),
        "property" if has("static") => ("STATIC_FIELD_ATTRIBUTES", "DEFAULT_STATIC_FIELD"),
        "property" | "recordComponent" => ("INSTANCE_FIELD_ATTRIBUTES", "DEFAULT_INSTANCE_FIELD"),
        "parameter" => ("PARAMETER_ATTRIBUTES", "DEFAULT_PARAMETER"),
        "variable" if has("static") => ("STATIC_FIELD_ATTRIBUTES", "DEFAULT_GLOBAL_VARIABLE"),
        "variable" => ("LOCAL_VARIABLE_ATTRIBUTES", "DEFAULT_LOCAL_VARIABLE"),
        "interface" => ("INTERFACE_NAME_ATTRIBUTES", "DEFAULT_INTERFACE_NAME"),
        "class" | "struct" | "record" | "enum" | "type" => {
            ("CLASS_NAME_ATTRIBUTES", "DEFAULT_CLASS_NAME")
        }
        "typeParameter" => ("TYPE_PARAMETER_NAME_ATTRIBUTES", "DEFAULT_PARAMETER"),
        "decorator" | "annotation" => ("ANNOTATION_NAME_ATTRIBUTES", "DEFAULT_METADATA"),
        _ => return None,
    };
    Some(if java { java_key } else { neutral })
}

/// A colour with alpha from a theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Rgba {
    /// 0xRRGGBB.
    pub rgb: u32,
    /// 0–255.
    pub alpha: u8,
}

/// A New UI theme with palette names resolved.
#[derive(Debug, Clone)]
pub struct UiTheme {
    /// Theme name.
    pub name: String,
    /// Dark theme.
    pub dark: bool,
    values: BTreeMap<String, String>,
    palette: BTreeMap<String, String>,
    /// Colours that could not be read and were left out (`SPEC-0060` v10 behaviour 15).
    notes: Vec<String>,
}

/// Whether `value` is written as a colour (`#…`) without being one.
fn malformed_colour(value: &str) -> bool {
    value
        .strip_prefix('#')
        .is_some_and(|h| !matches!(h.len(), 6 | 8) || !h.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn flatten(prefix: &str, v: &serde_json::Value, out: &mut BTreeMap<String, String>) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&key, v, out);
            }
        }
        serde_json::Value::String(s) => {
            out.insert(prefix.to_owned(), s.clone());
        }
        other => {
            out.insert(prefix.to_owned(), other.to_string());
        }
    }
}

impl UiTheme {
    /// Reads a `.theme.json` file (behaviour 5).
    pub fn parse(json: &str) -> Result<Self, ThemeError> {
        let v: serde_json::Value =
            serde_json::from_str(json).map_err(|e| ThemeError::Json(e.to_string()))?;
        let mut values = BTreeMap::new();
        flatten("", &v["ui"], &mut values);
        let mut palette = BTreeMap::new();
        flatten("", &v["colors"], &mut palette);
        let name = v["name"].as_str().unwrap_or_default().to_owned();
        // A value written as a colour that is none is left out, so the parent theme's value or
        // the `*` default counts for that key alone (behaviour 15).
        let mut notes = Vec::new();
        for (what, map) in [("", &mut values), ("palette colour ", &mut palette)] {
            map.retain(|key, value| {
                let bad = malformed_colour(value);
                if bad {
                    notes.push(format!(
                        "theme `{name}`: {what}`{key}` has the malformed colour `{value}`; \
                         the inherited colour is used"
                    ));
                }
                !bad
            });
        }
        Ok(Self {
            name,
            dark: v["dark"].as_bool().unwrap_or(false),
            values,
            palette,
            notes,
        })
    }

    /// `child` over `parent`: the child's values and palette win, everything else is inherited
    /// (behaviour 5, `parentTheme`).
    pub fn with_parent(child: &str, parent: &UiTheme) -> Result<Self, ThemeError> {
        let child = Self::parse(child)?;
        let mut values = parent.values.clone();
        values.extend(child.values);
        let mut palette = parent.palette.clone();
        palette.extend(child.palette);
        let mut notes = child.notes;
        notes.extend(parent.notes.iter().cloned());
        Ok(Self {
            name: child.name,
            dark: child.dark,
            values,
            palette,
            notes,
        })
    }

    /// The colours of the theme's files that could not be read (behaviour 15), the theme's own
    /// first.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Whether the theme sets `key` (or its `*` default) to something that gives no colour, such
    /// as a palette name nobody defines.
    pub fn unresolved(&self, key: &str) -> Option<&str> {
        let value = self.values.get(key).or_else(|| {
            let last = key.rsplit('.').next()?;
            self.values.get(&format!("*.{last}"))
        })?;
        self.rgba(value, 0).is_none().then_some(value.as_str())
    }

    /// A bundled theme with its parent resolved.
    pub fn for_theme(id: ThemeId) -> Self {
        match id {
            ThemeId::Dark => Self::bundled(true),
            ThemeId::Light => Self::bundled(false),
            ThemeId::IslandsDark => {
                Self::with_parent(ISLANDS_DARK_THEME_JSON, &Self::bundled(true))
                    .expect("bundled theme parses")
            }
            ThemeId::IslandsLight => {
                let header = Self::with_parent(LIGHT_HEADER_THEME_JSON, &Self::bundled(false))
                    .expect("bundled theme parses");
                Self::with_parent(ISLANDS_LIGHT_THEME_JSON, &header).expect("bundled theme parses")
            }
            ThemeId::IdecraftDark => Self::with_parent(
                IDECRAFT_DARK_THEME_JSON,
                &Self::for_theme(ThemeId::IslandsDark),
            )
            .expect("bundled theme parses"),
            ThemeId::IdecraftLight => Self::with_parent(
                IDECRAFT_LIGHT_THEME_JSON,
                &Self::for_theme(ThemeId::IslandsLight),
            )
            .expect("bundled theme parses"),
        }
    }

    /// The bundled dark or light theme.
    pub fn bundled(dark: bool) -> Self {
        Self::parse(if dark {
            DARK_THEME_JSON
        } else {
            LIGHT_THEME_JSON
        })
        .expect("bundled theme parses")
    }

    fn rgba(&self, value: &str, depth: usize) -> Option<Rgba> {
        if let Some(hexs) = value.strip_prefix('#') {
            let n = u32::from_str_radix(hexs, 16).ok()?;
            return match hexs.len() {
                6 => Some(Rgba { rgb: n, alpha: 255 }),
                8 => Some(Rgba {
                    rgb: n >> 8,
                    alpha: (n & 0xff) as u8,
                }),
                _ => None,
            };
        }
        if depth > 8 {
            return None;
        }
        self.rgba(self.palette.get(value)?, depth + 1)
    }

    /// A numeric UI value such as `EditorTabs.unselectedAlpha`, with the same `*` default as
    /// [`UiTheme::color`].
    pub fn number(&self, key: &str) -> Option<f32> {
        let value = self.values.get(key).or_else(|| {
            let last = key.rsplit('.').next()?;
            self.values.get(&format!("*.{last}"))
        })?;
        value.parse().ok()
    }

    /// `key.compact` in compact mode when the theme sets it, else `key` (`SPEC-0165`).
    pub fn number_in(&self, key: &str, compact: bool) -> Option<f32> {
        compact
            .then(|| self.values.get(&format!("{key}.compact"))?.parse().ok())
            .flatten()
            .or_else(|| self.number(key))
    }

    /// [`UiTheme::insets`] with the compact value in compact mode (`SPEC-0165`).
    pub fn insets_in(&self, key: &str, compact: bool) -> Option<[f32; 4]> {
        compact
            .then(|| self.insets(&format!("{key}.compact")))
            .flatten()
            .or_else(|| self.insets(key))
    }

    /// [`UiTheme::number_in`] without the `*` default: only the key itself (or its compact value)
    /// counts (`SPEC-0061` v4, keys such as `Button.ToolWindow.arc` whose last segment is common).
    pub fn own_number_in(&self, key: &str, compact: bool) -> Option<f32> {
        let get = |k: &str| self.values.get(k)?.parse().ok();
        compact
            .then(|| get(&format!("{key}.compact")))
            .flatten()
            .or_else(|| get(key))
    }

    /// A size such as `StripeToolbar.Button.size` (`"width,height"`), the compact value in compact
    /// mode when the theme sets one (`SPEC-0061` v4).
    pub fn size_in(&self, key: &str, compact: bool) -> Option<[f32; 2]> {
        let parse = |k: &str| -> Option<[f32; 2]> {
            let parts: Vec<f32> = self
                .values
                .get(k)?
                .split(',')
                .map(|p| p.trim().parse().ok())
                .collect::<Option<_>>()?;
            <[f32; 2]>::try_from(parts).ok()
        };
        compact
            .then(|| parse(&format!("{key}.compact")))
            .flatten()
            .or_else(|| parse(key))
    }

    /// Insets such as `EditorTabs.tabInsets` (`"top,left,bottom,right"`), with the same `*`
    /// default as [`UiTheme::color`] (`SPEC-0154`).
    pub fn insets(&self, key: &str) -> Option<[f32; 4]> {
        let value = self.values.get(key).or_else(|| {
            let last = key.rsplit('.').next()?;
            self.values.get(&format!("*.{last}"))
        })?;
        let parts: Vec<f32> = value
            .split(',')
            .map(|p| p.trim().parse().ok())
            .collect::<Option<_>>()?;
        <[f32; 4]>::try_from(parts).ok()
    }

    /// A UI colour such as `EditorTabs.underlineColor`: the key itself, else the `*` default for its
    /// last segment (behaviour 5).
    pub fn color(&self, key: &str) -> Option<Rgba> {
        let value = self.values.get(key).or_else(|| {
            let last = key.rsplit('.').next()?;
            self.values.get(&format!("*.{last}"))
        })?;
        self.rgba(value, 0)
    }
}

/// WCAG 2.2 contrast ratio of two opaque `0xRRGGBB` colours (1.0 to 21.0), used to check the brand
/// themes' text colours (`SPEC-0060` v6).
pub fn contrast(a: u32, b: u32) -> f32 {
    fn luminance(rgb: u32) -> f32 {
        let channel = |shift: u32| {
            let c = ((rgb >> shift) & 0xff) as f32 / 255.0;
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `a` mixed with `b`: `share` 0.0 is `a`, 1.0 is `b`.
pub fn mix(a: u32, b: u32, share: f32) -> u32 {
    let channel = |shift: u32| {
        let (x, y) = (((a >> shift) & 0xff) as f32, ((b >> shift) & 0xff) as f32);
        ((x + (y - x) * share.clamp(0.0, 1.0)).round() as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// `colour` as text on `background`: itself when it reaches the contrast `min`, else mixed towards
/// `text` (the theme's plain text on that background) in steps of 1/16 until it does; `text` when
/// nothing less does (`SPEC-0060` v10 behaviour 16).
pub fn readable(colour: u32, background: u32, text: u32, min: f32) -> u32 {
    (0..=16)
        .map(|step| mix(colour, text, step as f32 / 16.0))
        .find(|c| contrast(*c, background) >= min)
        .unwrap_or(text)
}

/// `colour` with alpha `alpha` (0–255) drawn over the opaque `background`.
pub fn over(colour: u32, alpha: u8, background: u32) -> u32 {
    mix(background, colour, f32::from(alpha) / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0295/AK-5: every bundled scheme has a background and a stripe colour for the usages
    /// of the symbol at the caret, and write accesses differ from read accesses.
    #[test]
    fn bundled_schemes_colour_read_and_write_occurrences() {
        for id in ThemeId::ALL {
            let s = ColorScheme::for_theme(id);
            let read = s.attributes("IDENTIFIER_UNDER_CARET_ATTRIBUTES");
            let write = s.attributes("WRITE_IDENTIFIER_UNDER_CARET_ATTRIBUTES");
            assert!(read.background.is_some(), "{}", id.name());
            assert!(write.background.is_some(), "{}", id.name());
            assert_ne!(read.background, write.background, "{}", id.name());
            assert!(
                read.stripe.is_some() && write.stripe.is_some(),
                "{}",
                id.name()
            );
            // The marks are backgrounds: the text keeps its own colour.
            assert_ne!(
                read.background,
                s.attributes("TEXT").background,
                "{}",
                id.name()
            );
        }
        let dark = ColorScheme::for_theme(ThemeId::IdecraftDark);
        assert_eq!(
            dark.attributes("IDENTIFIER_UNDER_CARET_ATTRIBUTES")
                .background,
            Some(0x373b39)
        );
        assert_eq!(
            dark.attributes("WRITE_IDENTIFIER_UNDER_CARET_ATTRIBUTES")
                .background,
            Some(0x402f33)
        );
        let light = ColorScheme::bundled(false);
        assert_eq!(
            light
                .attributes("IDENTIFIER_UNDER_CARET_ATTRIBUTES")
                .background,
            Some(0xedebfc)
        );
        assert_eq!(
            light
                .attributes("WRITE_IDENTIFIER_UNDER_CARET_ATTRIBUTES")
                .background,
            Some(0xfce8f4)
        );
    }

    #[test]
    fn dark_scheme_resolves_through_darcula_and_default() {
        let s = ColorScheme::bundled(true);
        assert_eq!(s.chain(), ["Dark", "Darcula", "Default"]);
        assert_eq!(s.color("CARET_ROW_COLOR"), Some(0x26282e));
        assert_eq!(s.color("LINE_NUMBERS_COLOR"), Some(0x4b5059));
        assert_eq!(s.attributes("TEXT").background, Some(0x1e1f22));
        assert_eq!(s.attributes("DEFAULT_KEYWORD").foreground, Some(0xcf8e6d));
        // `baseAttributes` and code fallbacks.
        assert_eq!(
            s.attributes("ANNOTATION_NAME_ATTRIBUTES"),
            s.attributes("DEFAULT_METADATA")
        );
        // A key defined anywhere in the chain wins over its fallback, as in the reference
        // (`AbstractColorsScheme.getDirectlyDefinedAttributes` walks the parents first): `Darcula`
        // defines instance methods, so `Dark` does not fall back to its function colour.
        assert_eq!(
            s.attributes("DEFAULT_INSTANCE_METHOD").foreground,
            Some(0x57aaf7)
        );
        assert_eq!(
            s.attributes("DEFAULT_FUNCTION_DECLARATION").foreground,
            Some(0x56a8f5)
        );
        // A key only the light root `Default` defines (`660e7a`) gives way to the fallback the dark
        // schemes define: fields stay readable on dark backgrounds (v9, OBS-0023 L24).
        assert_eq!(
            s.attributes("INSTANCE_FIELD_ATTRIBUTES"),
            s.attributes("DEFAULT_INSTANCE_FIELD")
        );
        assert_ne!(
            s.attributes("INSTANCE_FIELD_ATTRIBUTES").foreground,
            Some(0x660e7a)
        );
        // A key nobody defines takes its fallback: class names look like identifiers.
        assert_eq!(
            s.attributes("DEFAULT_CLASS_NAME"),
            s.attributes("DEFAULT_IDENTIFIER")
        );
        assert_eq!(
            s.capture_style("keyword.control").foreground,
            Some(0xcf8e6d)
        );
        assert_eq!(s.capture_style("string").foreground, Some(0x6aab73));
        assert_eq!(
            s.capture_style("property").foreground,
            s.attributes("DEFAULT_INSTANCE_FIELD").foreground
        );
        assert_eq!(
            s.capture_style("unknown").foreground,
            s.attributes("TEXT").foreground
        );
        // Markdown headings and code fences fall back like `MarkdownHighlighterColors`.
        assert_eq!(
            s.capture_style("text.title").foreground,
            s.attributes("DEFAULT_CONSTANT").foreground
        );
        assert_eq!(
            s.capture_style("text.literal").foreground,
            s.attributes("MARKDOWN_CODE_FENCE").foreground
        );
        assert!(s.capture_style("text.literal").foreground.is_some());
    }

    #[test]
    fn light_scheme_and_selection() {
        let s = ColorScheme::bundled(false);
        assert_eq!(s.chain(), ["Light", "Default"]);
        assert_eq!(s.color("SELECTION_BACKGROUND"), Some(0xa6d2ff));
        assert!(s.attributes("TEXT").foreground.is_some());
        let dark = ColorScheme::bundled(true);
        assert!(dark.color("SELECTION_BACKGROUND").is_some(), "inherited");
    }

    #[test]
    fn theme_palette_and_star_defaults() {
        let t = UiTheme::bundled(true);
        assert!(t.dark);
        assert_eq!(t.name, "Dark");
        assert_eq!(
            t.color("EditorTabs.background"),
            Some(Rgba {
                rgb: 0x1e1f22,
                alpha: 255
            })
        );
        // Not set for the tree: the `*` default applies.
        assert_eq!(
            t.color("Tree.selectionBackground"),
            t.color("*.selectionBackground")
        );
        assert_eq!(
            t.color("EditorTabs.hoverBackground"),
            Some(Rgba {
                rgb: 0x1e1f22,
                alpha: 0
            })
        );
        assert_eq!(t.number("EditorTabs.unselectedAlpha"), Some(0.75));
        assert_eq!(t.number("Tree.rowHeight"), Some(24.0));
        assert!(!UiTheme::bundled(false).dark);
        assert_eq!(t.color("No.suchKeyAnywhere"), None);
    }

    #[test]
    fn islands_dark_inherits_from_the_new_ui_dark_theme() {
        let t = UiTheme::for_theme(ThemeId::IslandsDark);
        assert_eq!(t.name, "Islands Dark");
        // Own values through several palette names.
        assert!(t.color("MainWindow.background").is_some());
        assert_ne!(
            t.color("MainWindow.background"),
            t.color("ToolWindow.background")
        );
        assert_eq!(
            t.color("MainWindow.background").map(|c| c.rgb),
            Some(0x26282c)
        );
        assert_eq!(
            t.color("ToolWindow.background").map(|c| c.rgb),
            Some(0x191a1c)
        );
        assert_eq!(t.color("Tree.hoverBackground").map(|c| c.alpha), Some(16));
        assert_eq!(t.number("Island.arc"), Some(20.0));
        // Insets of tabs and trees (SPEC-0154).
        assert_eq!(
            t.insets("EditorTabs.tabInsets"),
            Some([-6.0, 8.0, -6.0, 8.0])
        );
        assert_eq!(t.insets("Tree.border"), Some([4.0, 12.0, 4.0, 12.0]));
        assert_eq!(t.insets("Tree.rowHeight"), None);
        assert_eq!(t.number("Island.borderWidth"), Some(6.0));
        // Inherited from the parent where Islands sets nothing.
        let parent = UiTheme::bundled(true);
        assert_eq!(
            t.number("Breakpoint.iconHoverAlpha"),
            parent.number("Breakpoint.iconHoverAlpha")
        );
        let s = ColorScheme::for_theme(ThemeId::IslandsDark);
        assert_eq!(s.chain(), ["Islands Dark", "Darcula", "Default"]);
        assert!(s.attributes("TEXT").background.is_some());
    }

    #[test]
    fn islands_light_inherits_from_the_light_header_theme() {
        let t = UiTheme::for_theme(ThemeId::IslandsLight);
        assert_eq!(t.name, "Islands Light");
        assert!(!t.dark);
        assert!(!ThemeId::IslandsLight.is_dark());
        let main = t.color("MainWindow.background").expect("own value");
        let dark = UiTheme::for_theme(ThemeId::IslandsDark)
            .color("MainWindow.background")
            .expect("own value");
        assert!(main.rgb > dark.rgb, "a light window background");
        assert_eq!(t.number("Island.arc"), Some(20.0));
        // From the light parent where Islands Light sets nothing.
        assert_eq!(
            t.number("Tree.rowHeight"),
            UiTheme::bundled(false).number("Tree.rowHeight")
        );
        let s = ColorScheme::for_theme(ThemeId::IslandsLight);
        assert_eq!(s.chain()[0], "Light");
    }

    #[test]
    fn malformed_input_is_an_error() {
        assert!(matches!(UiTheme::parse("{"), Err(ThemeError::Json(_))));
        let orphan = parse_schemes(r#"<scheme name="X" parent_scheme="Nope"></scheme>"#)
            .unwrap()
            .remove(0);
        assert!(matches!(
            ColorScheme::resolve(orphan, &[]),
            Err(ThemeError::Parent(_))
        ));
    }

    /// SPEC-0060 v10 AK-14: a colour that cannot be read costs its key alone — the parent's
    /// value or the `*` default counts — and is noted; the rest of the file is used.
    #[test]
    fn a_malformed_colour_falls_back_per_key() {
        let parent = UiTheme::parse(
            r##"{"name": "Base", "dark": false,
                "colors": {"ink": "#101010", "paper": "#fafafa"},
                "ui": {"*": {"background": "paper", "foreground": "ink"},
                       "Tree": {"selectionBackground": "#c0d0e0"},
                       "Popup": {"background": "#ffffff"}}}"##,
        )
        .unwrap();
        assert!(parent.notes().is_empty());
        let child = UiTheme::with_parent(
            r##"{"name": "Broken", "dark": false,
                "colors": {"ink": "#12", "accent": "#3366ccff"},
                "ui": {"Tree": {"selectionBackground": "#zzzzzz", "rowHeight": 24},
                       "Popup": {"background": "#12345"},
                       "Link": {"activeForeground": "nowhere"},
                       "Button": {"default": {"startBackground": "accent"}}}}"##,
            &parent,
        )
        .unwrap();
        // The parent's values count where the child's are no colours.
        assert_eq!(
            child.color("Tree.selectionBackground").unwrap().rgb,
            0xc0d0e0
        );
        assert_eq!(child.color("Popup.background").unwrap().rgb, 0xffffff);
        assert_eq!(child.color("*.foreground").unwrap().rgb, 0x101010);
        // Everything else of the child is used.
        assert_eq!(
            child.color("Button.default.startBackground").unwrap().rgb,
            0x3366cc
        );
        assert_eq!(child.number("Tree.rowHeight"), Some(24.0));
        // A name nobody defines gives no colour and is reported as such.
        assert_eq!(child.color("Link.activeForeground"), None);
        assert_eq!(child.unresolved("Link.activeForeground"), Some("nowhere"));
        assert_eq!(child.unresolved("Popup.background"), None);
        assert_eq!(child.unresolved("Nothing.at.all"), None);
        let notes = child.notes().join("\n");
        for part in [
            "theme `Broken`: `Tree.selectionBackground` has the malformed colour `#zzzzzz`",
            "theme `Broken`: `Popup.background` has the malformed colour `#12345`",
            "theme `Broken`: palette colour `ink` has the malformed colour `#12`",
        ] {
            assert!(notes.contains(part), "{part} not in:\n{notes}");
        }
        assert_eq!(child.notes().len(), 3, "{notes}");

        // Schemes: the parent scheme's colour counts, an attribute loses the one colour.
        let schemes = parse_schemes(
            r##"<scheme name="Base"><colors>
                  <option name="CARET_ROW_COLOR" value="f5f8fe"/>
                  <option name="GUTTER_BACKGROUND" value="eeeeee"/>
                </colors><attributes>
                  <option name="TEXT"><value>
                    <option name="FOREGROUND" value="080808"/>
                    <option name="BACKGROUND" value="ffffff"/>
                  </value></option>
                </attributes></scheme>"##,
        )
        .unwrap();
        let broken = parse_schemes(
            r##"<scheme name="Broken" parent_scheme="Base"><colors>
                  <option name="CARET_ROW_COLOR" value="light blue"/>
                  <option name="GUTTER_BACKGROUND" value=""/>
                  <option name="SELECTION_BACKGROUND" value="a6d2ff"/>
                </colors><attributes>
                  <option name="DEFAULT_KEYWORD"><value>
                    <option name="FOREGROUND" value="#0033gg"/>
                    <option name="FONT_TYPE" value="1"/>
                  </value></option>
                </attributes></scheme>"##,
        )
        .unwrap()
        .remove(0);
        let scheme = ColorScheme::resolve(broken, &schemes).unwrap();
        assert_eq!(scheme.color("CARET_ROW_COLOR"), Some(0xf5f8fe), "inherited");
        assert_eq!(
            scheme.color("GUTTER_BACKGROUND"),
            None,
            "an empty value still means none"
        );
        assert_eq!(scheme.color("SELECTION_BACKGROUND"), Some(0xa6d2ff));
        let keyword = scheme.attributes("DEFAULT_KEYWORD");
        assert!(keyword.bold && keyword.foreground.is_none());
        assert_eq!(scheme.capture_style("keyword").foreground, Some(0x080808));
        let notes = scheme.notes().join("\n");
        for part in [
            "scheme `Broken`: `CARET_ROW_COLOR` has the malformed colour `light blue`",
            "scheme `Broken`: `DEFAULT_KEYWORD` has the malformed FOREGROUND `#0033gg`",
        ] {
            assert!(notes.contains(part), "{part} not in:\n{notes}");
        }
        assert_eq!(scheme.notes().len(), 2, "{notes}");
        // The bundled files have no such value.
        for id in ThemeId::ALL {
            assert!(UiTheme::for_theme(id).notes().is_empty(), "{id:?}");
            assert!(ColorScheme::for_theme(id).notes().is_empty(), "{id:?}");
        }
    }

    /// SPEC-0060 v10 AK-15: a colour that does not read on a background moves towards the text
    /// colour until it does; one that reads stays.
    #[test]
    fn colours_are_made_readable() {
        assert_eq!(mix(0x000000, 0xffffff, 0.0), 0x000000);
        assert_eq!(mix(0x000000, 0xffffff, 1.0), 0xffffff);
        assert_eq!(mix(0x102030, 0x304050, 0.5), 0x203040);
        assert_eq!(over(0x000000, 0, 0xabcdef), 0xabcdef);
        assert_eq!(over(0x123456, 255, 0xabcdef), 0x123456);
        // Amber on white is unreadable (1.6); it darkens towards the black text.
        let amber = readable(0xf2c55c, 0xffffff, 0x080808, 4.5);
        assert_ne!(amber, 0xf2c55c);
        assert!(contrast(amber, 0xffffff) >= 4.5);
        assert!(contrast(amber, 0xffffff) < 7.0, "no further than needed");
        // The same amber reads on a dark background and stays.
        assert_eq!(readable(0xf2c55c, 0x1e1f22, 0xbcbec4, 4.5), 0xf2c55c);
        // Nothing reads where the text itself does not: the text is the answer.
        assert_eq!(readable(0x808080, 0x7f7f7f, 0x858585, 4.5), 0x858585);
    }

    #[test]
    fn theme_names_round_trip() {
        for t in ThemeId::ALL {
            assert_eq!(ThemeId::from_name(t.name()), Some(t));
        }
        assert_eq!(
            ThemeId::from_name(" islands dark "),
            Some(ThemeId::IslandsDark)
        );
        assert_eq!(ThemeId::from_name("Solarized"), None);
        assert!(!ThemeId::Light.is_dark() && ThemeId::Dark.is_dark());
        assert_eq!(
            UiTheme::for_theme(ThemeId::Light).dark,
            ThemeId::Light.is_dark()
        );
    }

    /// UI keys every view reads (`palette.rs`, `metrics.rs`): each theme must define them.
    const UI_COLOUR_KEYS: [&str; 22] = [
        "*.background",
        "*.foreground",
        "*.inactiveForeground",
        "MainWindow.background",
        "ToolWindow.background",
        "StatusBar.Widget.foreground",
        "EditorTabs.underTabsBorderColor",
        "EditorTabs.underlinedTabBackground",
        "EditorTabs.underlineColor",
        "EditorTabs.hoverBackground",
        "Tree.selectionBackground",
        "Tree.selectionInactiveBackground",
        "Tree.hoverBackground",
        "MainToolbar.Dropdown.transparentHoverBackground",
        "ToolWindow.Button.selectedBackground",
        "Popup.background",
        "Popup.borderColor",
        "Button.default.startBackground",
        "Button.default.foreground",
        "Link.activeForeground",
        "*.focusColor",
        "Borders.color",
    ];

    /// Editor colours every view reads.
    const SCHEME_COLOUR_KEYS: [&str; 6] = [
        "CARET_ROW_COLOR",
        "LINE_NUMBERS_COLOR",
        "LINE_NUMBER_ON_CARET_ROW_COLOR",
        "SELECTION_BACKGROUND",
        "CARET_COLOR",
        "CONSOLE_BACKGROUND_KEY",
    ];

    #[test]
    fn every_theme_defines_every_token() {
        for id in ThemeId::ALL {
            let ui = UiTheme::for_theme(id);
            assert_eq!(ui.name, id.name(), "theme file of {id:?}");
            assert_eq!(ui.dark, id.is_dark(), "{id:?}");
            // The brand themes define every UI colour the views read; the reference's themes leave
            // some to the palette's fallbacks (e.g. the light ones set no `*.foreground`).
            for key in UI_COLOUR_KEYS {
                if id.is_brand() {
                    assert!(ui.color(key).is_some(), "{id:?} lacks {key}");
                }
            }
            assert!(ui.color("*.background").is_some(), "{id:?}");
            let scheme = ColorScheme::for_theme(id);
            for key in SCHEME_COLOUR_KEYS {
                assert!(scheme.color(key).is_some(), "{id:?} lacks {key}");
            }
            let text = scheme.attributes("TEXT");
            assert!(
                text.foreground.is_some() && text.background.is_some(),
                "{id:?}"
            );
            for key in ["ERRORS_ATTRIBUTES", "WARNING_ATTRIBUTES"] {
                let s = scheme.attributes(key);
                assert!(
                    s.effect.or(s.background).or(s.foreground).is_some(),
                    "{id:?} lacks {key}"
                );
            }
        }
    }

    #[test]
    fn brand_themes_take_the_brand_tokens() {
        let dark = UiTheme::for_theme(ThemeId::IdecraftDark);
        let rgb = |t: &UiTheme, k: &str| t.color(k).map(|c| c.rgb);
        // DESIGN_SYSTEM.md tokens: background-primary, background-secondary, foreground-secondary,
        // accent-primary.
        assert_eq!(rgb(&dark, "ToolWindow.background"), Some(0x2e3440));
        assert_eq!(rgb(&dark, "MainWindow.background"), Some(0x3b4252));
        assert_eq!(rgb(&dark, "*.foreground"), Some(0xd8dee9));
        assert_eq!(rgb(&dark, "Button.default.startBackground"), Some(0x88c0d0));
        assert_eq!(rgb(&dark, "Link.activeForeground"), Some(0x88c0d0));
        assert_eq!(rgb(&dark, "*.focusColor"), Some(0x88c0d0));
        assert_eq!(
            rgb(&dark, "EditorTabs.underlinedBorderColor"),
            Some(0x88c0d0)
        );
        // Layout and sizes stay those of Islands Dark (ADR-0018).
        let islands = UiTheme::for_theme(ThemeId::IslandsDark);
        for key in [
            "Island.arc",
            "Island.borderWidth",
            "Tree.rowHeight",
            "EditorTabs.unselectedAlpha",
        ] {
            assert_eq!(dark.number(key), islands.number(key), "{key}");
        }
        assert_eq!(
            dark.insets("EditorTabs.tabInsets"),
            islands.insets("EditorTabs.tabInsets")
        );
        let scheme = ColorScheme::for_theme(ThemeId::IdecraftDark);
        assert_eq!(
            scheme.chain(),
            ["IDECraft Dark", "Islands Dark", "Darcula", "Default"]
        );
        assert_eq!(scheme.attributes("TEXT").background, Some(0x2e3440));
        assert_eq!(scheme.attributes("TEXT").foreground, Some(0xd8dee9));
        assert_eq!(
            scheme.attributes("ERRORS_ATTRIBUTES").effect,
            Some(0xde8c93)
        );
        // Light: background-light, foreground-primary-light, accent-primary-light.
        let light = UiTheme::for_theme(ThemeId::IdecraftLight);
        assert_eq!(rgb(&light, "ToolWindow.background"), Some(0xeceff4));
        assert_eq!(rgb(&light, "*.foreground"), Some(0x2e3440));
        assert_eq!(
            rgb(&light, "Button.default.startBackground"),
            Some(0x4c6a91)
        );
        assert_eq!(rgb(&light, "Link.activeForeground"), Some(0x4c6a91));
        assert_eq!(
            light.number("Island.arc"),
            UiTheme::for_theme(ThemeId::IslandsLight).number("Island.arc")
        );
        let scheme = ColorScheme::for_theme(ThemeId::IdecraftLight);
        assert_eq!(scheme.chain(), ["IDECraft Light", "Light", "Default"]);
        assert_eq!(scheme.attributes("TEXT").background, Some(0xeceff4));
        assert!(ThemeId::IdecraftDark.is_brand() && !ThemeId::IslandsDark.is_brand());
    }

    /// SPEC-0027 AK-11 (v9): fields stand out in the brand schemes, in the colour of constants.
    #[test]
    fn brand_schemes_give_fields_the_constant_colour() {
        for (id, colour) in [
            (ThemeId::IdecraftDark, 0xc3a2bd),
            (ThemeId::IdecraftLight, 0x85597e),
        ] {
            let s = ColorScheme::for_theme(id);
            let field = s.attributes("DEFAULT_INSTANCE_FIELD").foreground;
            assert_eq!(field, Some(colour), "{id:?}");
            assert_eq!(field, s.attributes("DEFAULT_CONSTANT").foreground, "{id:?}");
            assert_ne!(field, s.attributes("TEXT").foreground, "{id:?}");
            assert_eq!(s.capture_style("property").foreground, field, "{id:?}");
        }
    }

    /// SPEC-0027 AK-12 and AK-13 (v10), OBS-0047 T2/T3: doc comments, fields and static fields
    /// take the open scheme's colours — in `Islands Dark` doc comments `5f826b` italic, fields
    /// `c77dbb`, static fields `c77dbb` italic — and every bundled scheme gives the three roles
    /// the style of the scheme's own keys: doc comments and static fields italic, each different
    /// from the plain text colour.
    /// SPEC-0027 AK-14 (v11): reassigned locals and parameters keep the text colour and are
    /// underlined in the effect colour; a plain local is not.
    #[test]
    fn reassigned_keys_underline() {
        assert_eq!(
            capture_key("variable.reassigned"),
            "DEFAULT_REASSIGNED_LOCAL_VARIABLE"
        );
        assert_eq!(
            capture_key("variable.parameter.reassigned"),
            "DEFAULT_REASSIGNED_PARAMETER"
        );
        let islands = ColorScheme::for_theme(ThemeId::IslandsDark);
        for role in ["variable.reassigned", "variable.parameter.reassigned"] {
            let s = islands.capture_style(role);
            assert_eq!(
                (s.foreground, s.effect, s.underline),
                (Some(0xbcbec4), Some(0x84868c), true),
                "{role}"
            );
        }
        assert!(!islands.capture_style("variable").underline);
        assert!(!islands.capture_style("variable.parameter").underline);
    }

    #[test]
    fn doc_comments_and_fields_take_the_scheme_colours() {
        assert_eq!(capture_key("comment.documentation"), "DEFAULT_DOC_COMMENT");
        assert_eq!(capture_key("property"), "DEFAULT_INSTANCE_FIELD");
        assert_eq!(capture_key("property.static"), "DEFAULT_STATIC_FIELD");
        let islands = ColorScheme::for_theme(ThemeId::IslandsDark);
        let doc = islands.capture_style("comment.documentation");
        assert_eq!((doc.foreground, doc.italic), (Some(0x5f826b), true));
        let field = islands.capture_style("property");
        assert_eq!((field.foreground, field.italic), (Some(0xc77dbb), false));
        let fixed = islands.capture_style("property.static");
        assert_eq!((fixed.foreground, fixed.italic), (Some(0xc77dbb), true));
        // A plain comment stays grey and upright.
        let comment = islands.capture_style("comment");
        assert_eq!(
            (comment.foreground, comment.italic),
            (Some(0x7a7e85), false)
        );
        for id in ThemeId::ALL {
            let s = ColorScheme::for_theme(id);
            let text = s.attributes("TEXT").foreground;
            for (role, key) in [
                ("comment.documentation", "DEFAULT_DOC_COMMENT"),
                ("property", "DEFAULT_INSTANCE_FIELD"),
                ("property.static", "DEFAULT_STATIC_FIELD"),
            ] {
                let style = s.capture_style(role);
                assert_eq!(
                    style.foreground,
                    s.attributes(key).foreground,
                    "{id:?} {role}"
                );
                assert!(style.foreground.is_some(), "{id:?} {role}");
                assert_ne!(style.foreground, text, "{id:?} {role}");
            }
            assert!(s.capture_style("comment.documentation").italic, "{id:?}");
            assert!(s.capture_style("property.static").italic, "{id:?}");
        }
        // The brand schemes give static fields their field colour, in italics.
        for id in [ThemeId::IdecraftDark, ThemeId::IdecraftLight] {
            let s = ColorScheme::for_theme(id);
            assert_eq!(
                s.capture_style("property.static").foreground,
                s.capture_style("property").foreground,
                "{id:?}"
            );
        }
    }

    /// WCAG AA (4.5 : 1) for text in the brand themes (`SPEC-0060` v6 behaviour 11).
    #[test]
    fn brand_text_meets_wcag_aa() {
        assert!((contrast(0xffffff, 0x000000) - 21.0).abs() < 0.01);
        assert!((contrast(0xeceff4, 0x2e3440) - 10.84).abs() < 0.01);
        for id in [ThemeId::IdecraftDark, ThemeId::IdecraftLight] {
            let ui = UiTheme::for_theme(id);
            let scheme = ColorScheme::for_theme(id);
            let u = |k: &str| ui.color(k).expect(k).rgb;
            let editor = scheme
                .attributes("TEXT")
                .background
                .expect("editor background");
            let island = u("ToolWindow.background");
            let window = u("MainWindow.background");
            let popup = u("Popup.background");
            let text = u("*.foreground");
            let muted = u("StatusBar.Widget.foreground");
            let mut pairs = vec![
                ("text on islands", text, island),
                ("text on the window", text, window),
                ("text on popups", text, popup),
                ("muted text on islands", muted, island),
                ("muted text on the window", muted, window),
                ("muted text on popups", muted, popup),
                ("links on islands", u("Link.activeForeground"), island),
                (
                    "primary button",
                    u("Button.default.foreground"),
                    u("Button.default.startBackground"),
                ),
                ("selected tree row", text, u("Tree.selectionBackground")),
                (
                    "selected tab",
                    text,
                    u("EditorTabs.underlinedTabBackground"),
                ),
                (
                    "inactive selection",
                    text,
                    u("Tree.selectionInactiveBackground"),
                ),
            ];
            let code = scheme.attributes("TEXT").foreground.expect("editor text");
            pairs.push(("editor text", code, editor));
            let caret_row = scheme.color("CARET_ROW_COLOR").expect("caret row");
            pairs.push(("editor text on the caret row", code, caret_row));
            let selection = scheme.color("SELECTION_BACKGROUND").expect("selection");
            pairs.push(("editor text in a selection", code, selection));
            for key in [
                "DEFAULT_KEYWORD",
                "DEFAULT_STRING",
                "DEFAULT_NUMBER",
                "DEFAULT_LINE_COMMENT",
                "DEFAULT_DOC_COMMENT",
                "DEFAULT_FUNCTION_DECLARATION",
                "DEFAULT_CLASS_NAME",
                "DEFAULT_METADATA",
                "DEFAULT_VALID_STRING_ESCAPE",
            ] {
                let fg = scheme.attributes(key).foreground.expect(key);
                pairs.push((key, fg, editor));
            }
            for (what, fg, bg) in pairs {
                let ratio = contrast(fg, bg);
                assert!(
                    ratio >= 4.5,
                    "{id:?}: {what} {fg:06x} on {bg:06x} is {ratio:.2}"
                );
            }
        }
    }

    #[test]
    fn new_installs_get_the_brand_theme_and_choices_are_kept() {
        assert_eq!(ThemeId::DEFAULT, ThemeId::IdecraftDark);
        assert_eq!(ThemeId::ALL[0], ThemeId::DEFAULT, "the default comes first");
        assert_eq!(ThemeId::from_saved(None), ThemeId::IdecraftDark);
        assert_eq!(
            ThemeId::from_saved(Some("Solarized")),
            ThemeId::IdecraftDark
        );
        for t in ThemeId::ALL {
            assert_eq!(
                ThemeId::from_saved(Some(t.name())),
                t,
                "a saved choice is kept"
            );
        }
        assert_eq!(
            ThemeId::from_saved(Some("Islands Dark")),
            ThemeId::IslandsDark
        );
    }

    /// SPEC-0027 v7: SQL built-ins take the function-declaration colour of every bundled scheme and
    /// are always italic.
    #[test]
    fn sql_built_ins_fall_back_to_the_declaration_colour_in_italics() {
        assert_eq!(capture_key("function.builtin.sql"), "SQL_BUILTIN_FUNCTION");
        for id in ThemeId::ALL {
            let s = ColorScheme::for_theme(id);
            let builtin = s.capture_style("function.builtin.sql");
            let declaration = s.capture_style("function");
            assert_eq!(builtin.foreground, declaration.foreground, "{id:?}");
            assert!(builtin.italic, "{id:?}");
        }
    }

    /// SPEC-0290 AK-7 (v3): Kotlin files take the Java keys, and the syntax roles of SPEC-0027 v8
    /// resolve, in every bundled scheme, to the style the server's token gives the same name, so
    /// functions and properties keep their colour when the tokens arrive.
    #[test]
    fn kotlin_tokens_take_the_java_roles() {
        for path in ["Main.kt", "src/zoo/Animals.KT", "build.gradle.kts"] {
            assert_eq!(KeyFamily::of_path(path), KeyFamily::Kotlin, "{path}");
        }
        for (ty, mods) in [
            ("function", &["declaration"][..]),
            ("function", &[]),
            ("property", &["readonly"]),
            ("property", &["declaration"]),
            ("parameter", &["declaration", "readonly"]),
            ("variable", &["readonly"]),
            ("class", &["declaration", "abstract"]),
            ("interface", &[]),
            ("enumMember", &[]),
        ] {
            assert_eq!(
                semantic_key_in(ty, mods, KeyFamily::Kotlin),
                semantic_key_in(ty, mods, KeyFamily::Java),
                "{ty} {mods:?}"
            );
        }
        assert_eq!(
            semantic_key_in("function", &["declaration"], KeyFamily::Kotlin),
            Some("METHOD_DECLARATION_ATTRIBUTES")
        );
        assert_eq!(
            semantic_key_in("property", &[], KeyFamily::Kotlin),
            Some("INSTANCE_FIELD_ATTRIBUTES")
        );
        assert_eq!(semantic_key_in("keyword", &[], KeyFamily::Kotlin), None);
        for id in ThemeId::ALL {
            let s = ColorScheme::for_theme(id);
            let text = s.attributes("TEXT").foreground;
            let token = |key: &str| {
                let a = s.attributes(key);
                (a.foreground.or(text), a.italic, a.bold)
            };
            let role = |kind: &str| {
                let a = s.capture_style(kind);
                (a.foreground, a.italic, a.bold)
            };
            for (kind, key) in [
                ("function", "METHOD_DECLARATION_ATTRIBUTES"),
                ("function.call", "METHOD_CALL_ATTRIBUTES"),
                ("property", "INSTANCE_FIELD_ATTRIBUTES"),
            ] {
                assert_eq!(role(kind), token(key), "{id:?} {kind}");
            }
        }
        // Where the scheme gives functions and fields a colour of their own, Kotlin shows it.
        let dark = ColorScheme::bundled(true);
        let text = dark.attributes("TEXT").foreground;
        assert_ne!(dark.capture_style("function").foreground, text);
        assert_ne!(dark.capture_style("property").foreground, text);
    }

    /// SPEC-0290 AK-6 (v2): in JavaScript and TypeScript the server's global objects and type
    /// parameters keep the styles of their syntax roles (SPEC-0027 v6); other files keep v1.
    #[test]
    fn script_semantic_tokens_keep_the_syntax_roles() {
        for path in ["a.ts", "src/b.tsx", "c.js", "d.MJS", "e.jsx", "f.cts"] {
            assert_eq!(KeyFamily::of_path(path), KeyFamily::Script, "{path}");
        }
        assert_eq!(KeyFamily::of_path("A.java"), KeyFamily::Java);
        assert_eq!(KeyFamily::of_path("x.py"), KeyFamily::Neutral);
        assert_eq!(KeyFamily::of_path("Makefile"), KeyFamily::Neutral);
        let script = KeyFamily::Script;
        let lib = ["readonly", "defaultLibrary"];
        assert_eq!(
            semantic_key_in("variable", &lib, script),
            Some("JS.GLOBAL_VARIABLE")
        );
        assert_eq!(
            semantic_key_in("property", &["defaultLibrary"], script),
            Some("JS.GLOBAL_VARIABLE")
        );
        assert_eq!(
            semantic_key_in("typeParameter", &["declaration"], script),
            Some("TYPE_PARAMETER_NAME_ATTRIBUTES")
        );
        // Everything else in script files maps by the neutral column.
        assert_eq!(
            semantic_key_in("variable", &["readonly"], script),
            Some("DEFAULT_LOCAL_VARIABLE")
        );
        assert_eq!(
            semantic_key_in("method", &["defaultLibrary"], script),
            Some("DEFAULT_FUNCTION_CALL")
        );
        assert_eq!(
            semantic_key_in("property", &[], script),
            Some("DEFAULT_INSTANCE_FIELD")
        );
        // Other languages keep the v1 mapping.
        for family in [KeyFamily::Neutral, KeyFamily::Java] {
            assert_ne!(
                semantic_key_in("variable", &lib, family),
                Some("JS.GLOBAL_VARIABLE")
            );
        }
        assert_eq!(
            semantic_key_in("typeParameter", &[], KeyFamily::Neutral),
            Some("DEFAULT_PARAMETER")
        );
        // In `Dark` the server's key gives the same style as the syntax role.
        let dark = ColorScheme::bundled(true);
        let text = dark.attributes("TEXT").foreground;
        let over = |key: &str| {
            let s = dark.attributes(key);
            (s.foreground.or(text), s.italic)
        };
        let role = |kind: &str| {
            let s = dark.capture_style(kind);
            (s.foreground, s.italic)
        };
        assert_eq!(over("JS.GLOBAL_VARIABLE"), role("variable.global.js"));
        assert_eq!(over("JS.GLOBAL_VARIABLE"), (Some(0xc77dba), true));
        assert_eq!(
            over("TYPE_PARAMETER_NAME_ATTRIBUTES"),
            role("type.parameter")
        );
        assert_eq!(
            over("TYPE_PARAMETER_NAME_ATTRIBUTES"),
            (Some(0x16baac), false)
        );
    }

    /// SPEC-0290 AK-2: every row of the semantic table has its key, Java keys fall back to the
    /// neutral ones, and static members are italic in the default dark schemes.
    #[test]
    fn semantic_tokens_map_to_scheme_keys() {
        let cases: [(&str, &[&str], &str, &str); 14] = [
            (
                "method",
                &["declaration", "public"],
                "METHOD_DECLARATION_ATTRIBUTES",
                "DEFAULT_FUNCTION_DECLARATION",
            ),
            (
                "function",
                &["static"],
                "STATIC_METHOD_ATTRIBUTES",
                "DEFAULT_STATIC_METHOD",
            ),
            (
                "method",
                &[],
                "METHOD_CALL_ATTRIBUTES",
                "DEFAULT_FUNCTION_CALL",
            ),
            (
                "property",
                &["static", "readonly"],
                "STATIC_FINAL_FIELD_ATTRIBUTES",
                "DEFAULT_STATIC_FIELD",
            ),
            (
                "enumMember",
                &[],
                "STATIC_FINAL_FIELD_ATTRIBUTES",
                "DEFAULT_STATIC_FIELD",
            ),
            (
                "property",
                &["static"],
                "STATIC_FIELD_ATTRIBUTES",
                "DEFAULT_STATIC_FIELD",
            ),
            (
                "recordComponent",
                &["declaration"],
                "INSTANCE_FIELD_ATTRIBUTES",
                "DEFAULT_INSTANCE_FIELD",
            ),
            (
                "parameter",
                &[],
                "PARAMETER_ATTRIBUTES",
                "DEFAULT_PARAMETER",
            ),
            (
                "variable",
                &["static"],
                "STATIC_FIELD_ATTRIBUTES",
                "DEFAULT_GLOBAL_VARIABLE",
            ),
            (
                "variable",
                &["readonly"],
                "LOCAL_VARIABLE_ATTRIBUTES",
                "DEFAULT_LOCAL_VARIABLE",
            ),
            (
                "interface",
                &[],
                "INTERFACE_NAME_ATTRIBUTES",
                "DEFAULT_INTERFACE_NAME",
            ),
            ("record", &[], "CLASS_NAME_ATTRIBUTES", "DEFAULT_CLASS_NAME"),
            (
                "typeParameter",
                &[],
                "TYPE_PARAMETER_NAME_ATTRIBUTES",
                "DEFAULT_PARAMETER",
            ),
            (
                "decorator",
                &[],
                "ANNOTATION_NAME_ATTRIBUTES",
                "DEFAULT_METADATA",
            ),
        ];
        for (ty, mods, java, neutral) in cases {
            assert_eq!(semantic_key(ty, mods, true), Some(java), "{ty} {mods:?}");
            assert_eq!(
                semantic_key(ty, mods, false),
                Some(neutral),
                "{ty} {mods:?}"
            );
        }
        for ty in [
            "namespace",
            "keyword",
            "modifier",
            "comment",
            "string",
            "number",
        ] {
            assert_eq!(semantic_key(ty, &[], true), None, "{ty}");
        }
        for id in [ThemeId::IslandsDark, ThemeId::IdecraftDark] {
            let s = ColorScheme::for_theme(id);
            assert_eq!(
                s.attributes("METHOD_DECLARATION_ATTRIBUTES"),
                s.attributes("DEFAULT_FUNCTION_DECLARATION")
            );
            assert!(
                s.attributes("METHOD_DECLARATION_ATTRIBUTES")
                    .foreground
                    .is_some()
            );
            assert!(s.attributes("STATIC_FIELD_ATTRIBUTES").italic, "{id:?}");
            assert!(s.attributes("STATIC_METHOD_ATTRIBUTES").italic, "{id:?}");
            // Fields take the scheme's own field colour, not the light root's (v9): Islands Dark
            // colours them, the IDECraft brand scheme draws them like text on purpose.
            let field = s.attributes("INSTANCE_FIELD_ATTRIBUTES").foreground;
            assert_eq!(field, s.attributes("DEFAULT_INSTANCE_FIELD").foreground);
            assert!(field.is_some() && field != Some(0x660e7a), "{id:?}");
            if id == ThemeId::IslandsDark {
                assert_eq!(field, Some(0xc77dbb));
            }
        }
    }
}
