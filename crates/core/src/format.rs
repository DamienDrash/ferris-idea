//! IDECraft file formats (`SPEC-0303`, ADR-0021): the `[idecraft]` envelope with `kind` and
//! `format`, the registry of kinds with their extensions, body shapes and migrations, and a reader
//! that reports every problem at once with its key path.
//!
//! Exchange files (`.idcrun`, `.idckeymap`, …) must carry the envelope and stay under 64 KiB.
//! Project files (`.idecraft/*.toml`, the user's settings and keymap) may lack it: such a file is
//! format 0 of its kind and is migrated in memory. Writers always write the current format and
//! never replace a file of a newer one.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Largest exchange file that is read (behaviour 2).
pub const MAX_EXCHANGE_BYTES: usize = 64 * 1024;

/// One problem of a file: where (a key path such as `run.command[1]`, or `(file)`) and what.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    /// Key path, or `(file)` for the file as a whole.
    pub path: String,
    /// What is wrong, in words.
    pub message: String,
}

impl Problem {
    /// A problem at `path`.
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }

    /// A problem of the whole file.
    pub fn file(message: impl Into<String>) -> Self {
        Self::new("(file)", message)
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// The problems joined into one line, for error messages.
pub fn problems_text(problems: &[Problem]) -> String {
    problems
        .iter()
        .map(Problem::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// The `[idecraft]` table (behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// The kind, see [`KINDS`].
    pub kind: String,
    /// Format version of the kind.
    pub format: u32,
    /// Stable id chosen by the author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Name shown in the import preview.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Who wrote it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// SPDX licence expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    /// Web page of the file (`https://`), shown and never fetched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Oldest IDECraft version the author expects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_version: Option<String>,
}

impl Envelope {
    /// The envelope of `kind` in its current format.
    ///
    /// # Panics
    /// When `kind` is not in [`KINDS`] (a programming error).
    pub fn current(kind: &str) -> Self {
        let k = find_kind(kind).unwrap_or_else(|| panic!("unknown file kind `{kind}`"));
        Self {
            kind: k.id.into(),
            format: k.current,
            ..Self::default()
        }
    }

    /// The same envelope with `name` set.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

/// The shape of a TOML value, used to check bodies and to write the JSON Schemas (behaviour 4).
#[derive(Debug)]
pub enum Shape {
    /// Any string.
    Str,
    /// An integer within the bounds.
    Int {
        /// Smallest value.
        min: Option<i64>,
        /// Largest value.
        max: Option<i64>,
    },
    /// A number (integer or float).
    Float,
    /// `true` or `false`.
    Bool,
    /// One of these strings.
    Enum(&'static [&'static str]),
    /// An array of values of one shape.
    Array(&'static Shape),
    /// A table with exactly these keys.
    Table(&'static [Field]),
    /// A table with free keys whose values have one shape.
    Map(&'static Shape),
}

/// One key of a [`Shape::Table`].
#[derive(Debug)]
pub struct Field {
    /// Key.
    pub name: &'static str,
    /// Shape of its value.
    pub shape: Shape,
    /// Whether the key must be present.
    pub required: bool,
    /// One-line description for the schema.
    pub doc: &'static str,
}

const fn opt(name: &'static str, shape: Shape, doc: &'static str) -> Field {
    Field {
        name,
        shape,
        required: false,
        doc,
    }
}

const fn req(name: &'static str, shape: Shape, doc: &'static str) -> Field {
    Field {
        name,
        shape,
        required: true,
        doc,
    }
}

const INT: Shape = Shape::Int {
    min: None,
    max: None,
};

/// One migration step of a kind: from format `from` to `from + 1`, on the body (behaviour 2).
#[derive(Debug)]
pub struct Migration {
    /// Format the step starts from.
    pub from: u32,
    /// Changes the body in place.
    pub apply: fn(&mut toml::Table),
}

/// A kind of IDECraft file (behaviours 6–7).
#[derive(Debug)]
pub struct Kind {
    /// Id written as `kind`.
    pub id: &'static str,
    /// Name in words.
    pub title: &'static str,
    /// Own extension of an exchange kind, without the dot.
    pub extension: Option<&'static str>,
    /// The format this version writes and reads up to.
    pub current: u32,
    /// The body's shape (its top-level keys besides `idecraft`); `None`: checked by its reader.
    pub body: Option<&'static Shape>,
    /// Migration steps, in order.
    pub migrations: &'static [Migration],
    /// Whether this version imports files of this kind (exchange kinds only).
    pub importable: bool,
    /// The file is a ZIP archive, not TOML (`plugin`).
    pub zip: bool,
}

impl Kind {
    /// The file name for `stem` with this kind's extension.
    pub fn file_name(&self, stem: &str) -> String {
        match self.extension {
            Some(ext) => format!("{}.{ext}", safe_stem(stem)),
            None => format!("{}.toml", safe_stem(stem)),
        }
    }
}

/// `name` made safe as a file name: letters, digits, `-`, `_`, `.` and spaces as `-`.
pub fn safe_stem(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches(['-', '.']).to_owned();
    if s.is_empty() { "export".into() } else { s }
}

/// Shape of the envelope.
pub const ENVELOPE: Shape = Shape::Table(&[
    req("kind", Shape::Str, "The kind of file."),
    req(
        "format",
        Shape::Int {
            min: Some(1),
            max: None,
        },
        "Format version of the kind; raised only on an incompatible change.",
    ),
    opt("id", Shape::Str, "Stable id chosen by the author."),
    opt("name", Shape::Str, "Name shown in the import preview."),
    opt("author", Shape::Str, "Who wrote the file."),
    opt("licence", Shape::Str, "SPDX licence expression."),
    opt(
        "homepage",
        Shape::Str,
        "Web page of the file (https://); shown, never fetched.",
    ),
    opt(
        "min_version",
        Shape::Str,
        "Oldest IDECraft version the author expects.",
    ),
]);

/// One run configuration (`SPEC-0047`).
pub const RUN_CONFIG: Shape = Shape::Table(&[
    req("name", Shape::Str, "Unique name."),
    opt(
        "command",
        Shape::Array(&Shape::Str),
        "Program and arguments; may be empty with `url`.",
    ),
    opt(
        "cwd",
        Shape::Str,
        "Working directory, relative to the workspace.",
    ),
    opt(
        "env",
        Shape::Map(&Shape::Str),
        "Extra environment variables.",
    ),
    opt(
        "kind",
        Shape::Enum(&["run", "test"]),
        "A program or tests with a report.",
    ),
    opt(
        "report",
        Shape::Enum(&["libtest", "tap", "junit"]),
        "Report format of a test configuration.",
    ),
    opt(
        "report_file",
        Shape::Str,
        "JUnit file written by the tool, relative to the workspace.",
    ),
    opt(
        "before",
        Shape::Array(&Shape::Str),
        "Configurations to run first.",
    ),
    opt(
        "timeout",
        Shape::Int {
            min: Some(1),
            max: None,
        },
        "Timeout in seconds.",
    ),
    opt("url", Shape::Str, "A page to open (http:// or https://)."),
]);

/// Body of a `run` file.
pub const RUN_BODY: Shape = Shape::Table(&[req("run", RUN_CONFIG, "The run configuration.")]);

/// A keymap: name, parent and shortcuts by action.
pub const KEYMAP: Shape = Shape::Table(&[
    req("name", Shape::Str, "Keymap name."),
    opt("parent", Shape::Str, "Bundled keymap it changes."),
    opt(
        "actions",
        Shape::Map(&Shape::Array(&Shape::Str)),
        "Shortcuts by action id: \"control alt S\", \"control K, control C\", \"mouse: …\", \"gesture: dblClick shift SHIFT\"; an empty array removes them.",
    ),
]);

/// Body of a `keymap` file.
pub const KEYMAP_BODY: Shape = Shape::Table(&[req("keymap", KEYMAP, "The keymap.")]);

/// Body of a `theme` file.
pub const THEME_BODY: Shape = Shape::Table(&[req(
    "theme",
    Shape::Table(&[
        req(
            "name",
            Shape::Str,
            "A bundled theme, e.g. \"IDECraft Light\".",
        ),
        opt("compact_mode", Shape::Bool, "Compact mode (SPEC-0165)."),
    ]),
    "The theme.",
)]);

/// One data source of a `sources` file: a connection profile without secrets (`SPEC-0008`).
pub const CONNECTION: Shape = Shape::Table(&[
    req("id", Shape::Str, "Id of the connection."),
    req("name", Shape::Str, "Name shown."),
    req(
        "dialect",
        Shape::Str,
        "Protocol family: postgresql, mysql, mariadb, sqlserver, oracle, sqlite, duckdb.",
    ),
    opt("system", Shape::Str, "Connector id (SPEC-0300)."),
    opt("host", Shape::Str, "Server host."),
    opt(
        "port",
        Shape::Int {
            min: Some(1),
            max: Some(65535),
        },
        "Server port.",
    ),
    opt("database", Shape::Str, "Database name."),
    opt("user", Shape::Str, "User name."),
    opt(
        "path",
        Shape::Str,
        "Database file, relative to the workspace.",
    ),
    opt(
        "ssl",
        Shape::Enum(&["disable", "prefer", "require", "verify-full"]),
        "TLS mode.",
    ),
    opt("read_only", Shape::Bool, "Open read-only."),
]);

/// Body of a `sources` file.
pub const SOURCES_BODY: Shape = Shape::Table(&[req(
    "connection",
    Shape::Array(&CONNECTION),
    "The data sources.",
)]);

const INDENT: Shape = Shape::Table(&[
    req("indent", INT, "Indent size."),
    req("tab", INT, "Tab size."),
    req("use_tabs", Shape::Bool, "Indent with tabs."),
]);

/// The shareable sections of the IDE settings (`SPEC-0129`).
pub const SETTINGS: Shape = Shape::Table(&[
    opt(
        "editor",
        Shape::Table(&[
            opt("font_family", Shape::Str, "Editor font."),
            opt("font_size", Shape::Float, "Font size in pixels."),
            opt("line_spacing", Shape::Float, "Line height factor."),
            opt("ligatures", Shape::Bool, "Font ligatures (off by default)."),
            opt("strip_trailing_spaces", Shape::Bool, ""),
            opt("ensure_line_break_at_end", Shape::Bool, ""),
            opt("spelling_dictionary", Shape::Str, ""),
            opt("semantic_highlighting", Shape::Bool, ""),
            opt("parameter_hints", Shape::Bool, ""),
            opt("fold_one_line_methods", Shape::Bool, ""),
            opt("fold_imports", Shape::Bool, ""),
            opt("config_values", Shape::Bool, ""),
            opt(
                "folding_arrows",
                Shape::Enum(&["always", "hover"]),
                "When the folding arrows in the gutter are shown (default: hover).",
            ),
            opt("usage_counts", Shape::Bool, ""),
            opt("usage_counts_above", Shape::Bool, ""),
            opt("code_authors", Shape::Bool, ""),
            opt("highlight_usages_at_caret", Shape::Bool, ""),
            opt(
                "tabs",
                Shape::Table(&[
                    opt("placement", Shape::Enum(&["top", "none"]), ""),
                    opt("rows", Shape::Enum(&["scroll", "squeeze", "wrap"]), ""),
                    opt("pinned_row", Shape::Bool, ""),
                    opt("show_icon", Shape::Bool, ""),
                    opt("show_extension", Shape::Bool, ""),
                    opt("show_folder_for_equal_names", Shape::Bool, ""),
                    opt("mark_modified", Shape::Bool, ""),
                    opt("path_tooltip", Shape::Bool, ""),
                    opt("close_button", Shape::Enum(&["right", "left", "none"]), ""),
                    opt("close_button_on_all", Shape::Bool, ""),
                    opt("sort_alphabetically", Shape::Bool, ""),
                    opt("open_at_end", Shape::Bool, ""),
                    opt("preview_tab", Shape::Bool, ""),
                    opt("limit", INT, "Most tabs per group, 1–100."),
                    opt(
                        "limit_rule",
                        Shape::Enum(&["close_unchanged", "close_unused"]),
                        "",
                    ),
                    opt(
                        "activate_on_close",
                        Shape::Enum(&["left", "right", "recent"]),
                        "",
                    ),
                ]),
                "Editor Tabs (SPEC-0059 v9).",
            ),
        ]),
        "Editor settings.",
    ),
    opt(
        "appearance",
        Shape::Table(&[
            opt("theme", Shape::Str, "The chosen theme."),
            opt("sync_with_os", Shape::Bool, ""),
            opt("dark_theme", Shape::Str, ""),
            opt("light_theme", Shape::Str, ""),
            opt("compact_mode", Shape::Bool, ""),
            opt(
                "editor_scheme",
                Shape::Str,
                "The editor colour scheme; empty for the theme's own (SPEC-0337).",
            ),
        ]),
        "Appearance settings.",
    ),
    opt(
        "vcs",
        Shape::Table(&[
            opt("update_method", Shape::Enum(&["merge", "rebase"]), ""),
            opt("allow_force_push", Shape::Bool, ""),
            opt("protected_branches", Shape::Str, ""),
        ]),
        "Version control settings.",
    ),
    opt(
        "code_style",
        Shape::Table(&[
            opt("right_margin", INT, ""),
            opt("show_right_margin", Shape::Bool, ""),
            opt("indents", Shape::Map(&INDENT), "Indents by language."),
        ]),
        "Code style.",
    ),
    opt(
        "project_view",
        Shape::Table(&[opt(
            "compact_middle_packages",
            Shape::Bool,
            "Packages that hold only one sub-package share its row (on by default).",
        )]),
        "The project tree (SPEC-0275 v10).",
    ),
    opt(
        "system",
        Shape::Table(&[
            opt(
                "confirm_quit",
                Shape::Bool,
                "Ask before quitting (off by default).",
            ),
            opt(
                "reopen_last_project",
                Shape::Bool,
                "Open the last project at start-up (on by default).",
            ),
            opt(
                "save_on_frame_deactivation",
                Shape::Bool,
                "Save files when the window loses the focus (off by default).",
            ),
        ]),
        "System Settings (SPEC-0337).",
    ),
    opt(
        "notifications",
        Shape::Table(&[opt(
            "log_info",
            Shape::Bool,
            "Keep informational notes in the Notifications tool window (on by default).",
        )]),
        "Notifications (SPEC-0337).",
    ),
]);

/// Body of a `settings` file.
pub const SETTINGS_BODY: Shape = Shape::Table(&[
    req("settings", SETTINGS, "IDE settings."),
    opt("keymap", KEYMAP, "The user keymap."),
]);

/// Body of `.idecraft/breakpoints.toml` (behaviour 14).
pub const BREAKPOINTS_BODY: Shape = Shape::Table(&[opt(
    "breakpoint",
    Shape::Array(&Shape::Table(&[
        req("file", Shape::Str, "File, relative to the workspace."),
        req(
            "line",
            Shape::Int {
                min: Some(1),
                max: None,
            },
            "1-based line.",
        ),
        opt("condition", Shape::Str, "Stop only when this is true."),
        opt("hit_condition", Shape::Str, "Pass count."),
        opt("log_message", Shape::Str, "Log instead of stopping."),
        opt("enabled", Shape::Bool, "false: kept but not set."),
        opt(
            "rules",
            Shape::Table(&[
                opt(
                    "suspend",
                    Shape::Enum(&["all", "thread", "none"]),
                    "Which threads stop; none logs and goes on.",
                ),
                opt("log_hit", Shape::Bool, "Log that the breakpoint was hit."),
                opt("log_stack", Shape::Bool, "Log the call stack."),
                opt(
                    "remove_once_hit",
                    Shape::Bool,
                    "Remove after the first hit.",
                ),
                opt(
                    "depends_on",
                    Shape::Str,
                    "Inactive until this breakpoint (file:line) is hit.",
                ),
                opt("depends_keep", Shape::Bool, "Stay active after that hit."),
                opt("instance_filter", Shape::Str, "Only for these object ids."),
                opt("class_filter", Shape::Str, "Only in these classes."),
                opt(
                    "caller_filter",
                    Shape::Str,
                    "Only when called from these methods.",
                ),
            ]),
            "Further options (SPEC-0127 v3).",
        ),
    ])),
    "Line breakpoints.",
)]);

/// Body of `.idecraft/bookmarks.toml` (`SPEC-0331` behaviour 10).
pub const BOOKMARKS_BODY: Shape = Shape::Table(&[opt(
    "bookmark",
    Shape::Array(&Shape::Table(&[
        req(
            "file",
            Shape::Str,
            "File, relative to the project, `/` between the parts.",
        ),
        req(
            "line",
            Shape::Int {
                min: Some(1),
                max: None,
            },
            "1-based line.",
        ),
        opt(
            "mnemonic",
            Shape::Str,
            "One of 1-9, 0, A-Z; unique in the project.",
        ),
        opt(
            "description",
            Shape::Str,
            "One line, at most 200 characters.",
        ),
    ])),
    "Line bookmarks, newest first.",
)]);

/// `python.toml` format 0 kept `interpreter` at the top; format 1 keeps it in `[python]`.
fn python_0_to_1(body: &mut toml::Table) {
    if let Some(v) = body.remove("interpreter") {
        let mut t = toml::Table::new();
        t.insert("interpreter".into(), v);
        body.insert("python".into(), toml::Value::Table(t));
    }
}

const fn exchange(
    id: &'static str,
    title: &'static str,
    ext: &'static str,
    body: Option<&'static Shape>,
) -> Kind {
    Kind {
        id,
        title,
        extension: Some(ext),
        current: 1,
        body,
        migrations: &[],
        importable: body.is_some(),
        zip: false,
    }
}

const fn project(id: &'static str, title: &'static str, body: Option<&'static Shape>) -> Kind {
    Kind {
        id,
        title,
        extension: None,
        current: 1,
        body,
        migrations: &[],
        importable: false,
        zip: false,
    }
}

/// Every kind (behaviours 6–7).
pub const KINDS: &[Kind] = &[
    exchange("connector", "Database connector", "idcconnector", None),
    exchange("sources", "Data sources", "idcsources", Some(&SOURCES_BODY)),
    exchange("run", "Run configuration", "idcrun", Some(&RUN_BODY)),
    exchange("keymap", "Keymap", "idckeymap", Some(&KEYMAP_BODY)),
    exchange("theme", "Theme", "idctheme", Some(&THEME_BODY)),
    exchange("code-style", "Code style", "idcstyle", None),
    exchange("live-templates", "Live templates", "idctemplates", None),
    exchange(
        "settings",
        "Settings bundle",
        "idcsettings",
        Some(&SETTINGS_BODY),
    ),
    Kind {
        zip: true,
        ..exchange("plugin", "Plugin", "idcplugin", None)
    },
    project("run-configurations", "Run configurations", None),
    project("connections", "Connections", None),
    project("sql-files", "SQL file bindings", None),
    Kind {
        migrations: &[Migration {
            from: 0,
            apply: python_0_to_1,
        }],
        ..project("python", "Python interpreter", None)
    },
    project("breakpoints", "Breakpoints", Some(&BREAKPOINTS_BODY)),
    project("bookmarks", "Bookmarks", Some(&BOOKMARKS_BODY)),
    project("ide-settings", "IDE settings", None),
    // A script of UI driver steps; it runs only through `idecraft drive run`, never by opening or
    // importing it (SPEC-0319 behaviour 30).
    project("ui-script", "UI script", None),
];

/// The kind `id`.
pub fn find_kind(id: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.id == id)
}

/// The exchange kind a file name says: its `.idc…` extension, also before a final `.toml`
/// (`x.idcrun.toml`).
pub fn kind_for_path(path: &Path) -> Option<&'static Kind> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let name = name.strip_suffix(".toml").unwrap_or(&name);
    let ext = Path::new(name).extension()?.to_str()?;
    KINDS.iter().find(|k| k.extension == Some(ext))
}

/// Whether `path` is an exchange file: an `.idc…` name, or a `.toml` whose envelope names an
/// exchange kind.
pub fn is_exchange_file(path: &Path) -> bool {
    if kind_for_path(path).is_some() {
        return true;
    }
    path.extension().is_some_and(|e| e == "toml")
        && std::fs::read_to_string(path).is_ok_and(|t| {
            t.len() <= MAX_EXCHANGE_BYTES
                && sniff_kind(&t)
                    .and_then(|k| find_kind(&k))
                    .is_some_and(|k| k.extension.is_some())
        })
}

/// The `kind` an envelope names, without checking anything else.
pub fn sniff_kind(text: &str) -> Option<String> {
    let table: toml::Table = toml::from_str(text).ok()?;
    table
        .get("idecraft")?
        .get("kind")?
        .as_str()
        .map(str::to_owned)
}

/// The message for a file of a newer format (behaviour 2).
pub fn newer_message(kind: &str, found: i64, reads: u32) -> String {
    format!(
        "written by a newer IDECraft (format {found}); this version reads {kind} files up to format {reads}"
    )
}

/// A file read and migrated to the current format.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// The envelope, its `format` now the current one.
    pub envelope: Envelope,
    /// Everything besides `idecraft`.
    pub body: toml::Table,
    /// The format the file had when it was older.
    pub migrated_from: Option<u32>,
}

impl Document {
    /// The body as `T`.
    pub fn body_as<T: serde::de::DeserializeOwned>(&self) -> Result<T, Vec<Problem>> {
        toml::Value::Table(self.body.clone())
            .try_into()
            .map_err(|e: toml::de::Error| vec![Problem::file(e.message().to_owned())])
    }
}

/// Where a file is read for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    /// An exchange file: envelope required, at most [`MAX_EXCHANGE_BYTES`].
    Exchange,
    /// A project or user file: a missing envelope is format 0.
    Project,
}

fn line_col(text: &str, at: usize) -> (usize, usize) {
    let before = &text[..at.min(text.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

/// The TOML syntax error of `text` with line and column.
pub fn syntax_problem(text: &str, e: &toml::de::Error) -> Problem {
    match e.span() {
        Some(span) => {
            let (line, col) = line_col(text, span.start);
            Problem::file(format!("line {line}, column {col}: {}", e.message()))
        }
        None => Problem::file(e.message().to_owned()),
    }
}

/// Reads `text` as a file of `expected` (or of any kind when `None`) through [`KINDS`]
/// (behaviour 2).
pub fn read(text: &str, expected: Option<&str>, used: Use) -> Result<Document, Vec<Problem>> {
    read_with(text, expected, used, KINDS)
}

/// [`read`] with another registry (tests).
pub fn read_with(
    text: &str,
    expected: Option<&str>,
    used: Use,
    kinds: &[Kind],
) -> Result<Document, Vec<Problem>> {
    if used == Use::Exchange && text.len() > MAX_EXCHANGE_BYTES {
        return Err(vec![Problem::file(format!(
            "larger than 64 KiB ({} bytes); not read",
            text.len()
        ))]);
    }
    let mut body: toml::Table = toml::from_str(text).map_err(|e| vec![syntax_problem(text, &e)])?;
    let mut problems = Vec::new();
    let (kind_id, format, mut envelope) = match body.remove("idecraft") {
        None if used == Use::Exchange => {
            return Err(vec![Problem::new(
                "idecraft",
                "not an IDECraft file: the [idecraft] table with kind and format is missing",
            )]);
        }
        None => {
            let Some(kind) = expected else {
                return Err(vec![Problem::new("idecraft", "missing")]);
            };
            (kind.to_owned(), 0i64, Envelope::default())
        }
        Some(value) => {
            check(&value, &ENVELOPE, "idecraft", &mut problems);
            if !problems.is_empty() {
                return Err(problems);
            }
            let kind = value["kind"].as_str().unwrap_or_default().to_owned();
            let format = value["format"].as_integer().unwrap_or_default();
            let envelope: Envelope = value.try_into().unwrap_or_default();
            (kind, format, envelope)
        }
    };
    let Some(kind) = kinds.iter().find(|k| k.id == kind_id) else {
        return Err(vec![Problem::new(
            "idecraft.kind",
            format!("unknown kind `{kind_id}`"),
        )]);
    };
    if let Some(expected) = expected
        && expected != kind.id
    {
        return Err(vec![Problem::new(
            "idecraft.kind",
            format!("is a `{}` file, not `{expected}`", kind.id),
        )]);
    }
    if format > i64::from(kind.current) {
        return Err(vec![Problem::new(
            "idecraft.format",
            newer_message(kind.id, format, kind.current),
        )]);
    }
    let format = format as u32;
    for step in format..kind.current {
        if let Some(m) = kind.migrations.iter().find(|m| m.from == step) {
            (m.apply)(&mut body);
        }
    }
    if let Some(shape) = kind.body {
        check(&toml::Value::Table(body.clone()), shape, "", &mut problems);
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    envelope.kind = kind.id.into();
    envelope.format = kind.current;
    Ok(Document {
        envelope,
        body,
        migrated_from: (format < kind.current).then_some(format),
    })
}

/// The format of a project file of `kind` from its text: 0 without envelope; problems for another
/// kind or a newer or bad format. Syntax errors count as 0 (the file's reader reports them).
pub fn project_format(text: &str, kind: &str) -> Result<u32, Vec<Problem>> {
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        return Ok(0);
    };
    let Some(env) = table.get("idecraft") else {
        return Ok(0);
    };
    let mut problems = Vec::new();
    check(env, &ENVELOPE, "idecraft", &mut problems);
    if !problems.is_empty() {
        return Err(problems);
    }
    let k = find_kind(kind).unwrap_or_else(|| panic!("unknown file kind `{kind}`"));
    let found = env["kind"].as_str().unwrap_or_default();
    if found != kind {
        return Err(vec![Problem::new(
            "idecraft.kind",
            format!("is a `{found}` file, not `{kind}`"),
        )]);
    }
    let format = env["format"].as_integer().unwrap_or_default();
    if format > i64::from(k.current) {
        return Err(vec![Problem::new(
            "idecraft.format",
            newer_message(kind, format, k.current),
        )]);
    }
    Ok(format as u32)
}

/// Removes the envelope of a project file of `kind` from `table` and checks it.
pub fn take_envelope(table: &mut toml::Table, kind: &str) -> Result<u32, Vec<Problem>> {
    let Some(env) = table.remove("idecraft") else {
        return Ok(0);
    };
    let mut t = toml::Table::new();
    t.insert("idecraft".into(), env);
    project_format(&toml::to_string(&t).unwrap_or_default(), kind)
}

/// The text of a file: the envelope, then `body` (behaviour 3). The body's top level must be
/// tables (or arrays of tables), so nothing falls into `[idecraft]`.
pub fn write(envelope: &Envelope, body: &impl Serialize) -> Result<String, String> {
    #[derive(Serialize)]
    struct Head<'a> {
        idecraft: &'a Envelope,
    }
    let value = toml::Value::try_from(body).map_err(|e| e.to_string())?;
    let mut table = match value {
        toml::Value::Table(t) => t,
        _ => return Err("the body is not a table".into()),
    };
    // An empty list says nothing; readers default it.
    let before = table.len();
    table.retain(|_, v| !v.as_array().is_some_and(Vec::is_empty));
    let dropped = table.len() != before;
    if let Some((key, _)) = table.iter().find(|(_, v)| match v {
        toml::Value::Table(_) => false,
        toml::Value::Array(a) => !a.iter().all(toml::Value::is_table),
        _ => true,
    }) {
        return Err(format!(
            "`{key}` is not a table and would fall into [idecraft]"
        ));
    }
    let mut out = toml::to_string(&Head { idecraft: envelope }).map_err(|e| e.to_string())?;
    // Straight from the body keeps its key order; a `toml::Table` sorts the keys.
    let body = if dropped {
        toml::to_string(&table)
    } else {
        toml::to_string(body)
    }
    .map_err(|e| e.to_string())?;
    if !body.trim().is_empty() {
        out.push('\n');
        out.push_str(&body);
    }
    Ok(out)
}

/// `path` with `.bak` added (behaviour 15).
pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

/// Before a project file of `kind` is replaced: refuses a file of another kind or a newer format,
/// and copies a file without envelope to `<name>.bak` once (behaviours 3, 15).
pub fn prepare_write(path: &Path, kind: &str) -> std::io::Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    match project_format(&text, kind) {
        Err(p) => Err(std::io::Error::other(format!(
            "{}: {}; not overwritten",
            path.display(),
            problems_text(&p)
        ))),
        Ok(0) if !text.trim().is_empty() => {
            let bak = backup_path(path);
            if !bak.exists() {
                std::fs::copy(path, bak)?;
            }
            Ok(())
        }
        Ok(_) => Ok(()),
    }
}

/// After the new form of a renamed file was written: moves the old file to `<old>.bak`, once; an
/// old file beside an existing backup stays and is ignored (behaviour 15).
pub fn retire(old: &Path) -> std::io::Result<()> {
    if !old.exists() {
        return Ok(());
    }
    let bak = backup_path(old);
    if bak.exists() {
        return Ok(());
    }
    std::fs::rename(old, bak)
}

/// Checks `value` against `shape` at `path`, adding every problem (behaviour 2).
pub fn check(value: &toml::Value, shape: &Shape, path: &str, problems: &mut Vec<Problem>) {
    let at = |p: &str| {
        if p.is_empty() {
            "(file)".to_owned()
        } else {
            p.to_owned()
        }
    };
    let wrong = |want: &str, problems: &mut Vec<Problem>| {
        problems.push(Problem::new(
            at(path),
            format!("expected {want}, found {}", type_name(value)),
        ));
    };
    match shape {
        Shape::Str => {
            if !value.is_str() {
                wrong("a string", problems);
            }
        }
        Shape::Bool => {
            if !value.is_bool() {
                wrong("true or false", problems);
            }
        }
        Shape::Float => {
            if !value.is_float() && !value.is_integer() {
                wrong("a number", problems);
            }
        }
        Shape::Int { min, max } => match value.as_integer() {
            None => wrong("an integer", problems),
            Some(n) => {
                if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                    let range = match (min, max) {
                        (Some(a), Some(b)) => format!("{a}–{b}"),
                        (Some(a), None) => format!("at least {a}"),
                        (None, Some(b)) => format!("at most {b}"),
                        (None, None) => String::new(),
                    };
                    problems.push(Problem::new(at(path), format!("{n} is not {range}")));
                }
            }
        },
        Shape::Enum(options) => match value.as_str() {
            Some(s) if options.contains(&s) => {}
            Some(s) => problems.push(Problem::new(
                at(path),
                format!("unknown value `{s}` (one of {})", options.join(", ")),
            )),
            None => wrong("a string", problems),
        },
        Shape::Array(item) => match value.as_array() {
            None => wrong("an array", problems),
            Some(items) => {
                for (i, v) in items.iter().enumerate() {
                    check(v, item, &format!("{path}[{i}]"), problems);
                }
            }
        },
        Shape::Map(item) => match value.as_table() {
            None => wrong("a table", problems),
            Some(t) => {
                for (k, v) in t {
                    check(v, item, &join(path, k), problems);
                }
            }
        },
        Shape::Table(fields) => match value.as_table() {
            None => wrong("a table", problems),
            Some(t) => {
                for (k, v) in t {
                    match fields.iter().find(|f| f.name == k) {
                        Some(f) => check(v, &f.shape, &join(path, k), problems),
                        None => problems.push(Problem::new(join(path, k), "unknown key")),
                    }
                }
                for f in fields.iter().filter(|f| f.required) {
                    if !t.contains_key(f.name) {
                        problems.push(Problem::new(join(path, f.name), "missing"));
                    }
                }
            }
        },
    }
}

fn join(path: &str, key: &str) -> String {
    let bare = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    let key = if bare {
        key.to_owned()
    } else {
        format!("\"{}\"", key.replace('"', "\\\""))
    };
    if path.is_empty() {
        key
    } else {
        format!("{path}.{key}")
    }
}

fn type_name(v: &toml::Value) -> &'static str {
    match v {
        toml::Value::String(_) => "a string",
        toml::Value::Integer(_) => "an integer",
        toml::Value::Float(_) => "a float",
        toml::Value::Boolean(_) => "a boolean",
        toml::Value::Datetime(_) => "a date",
        toml::Value::Array(_) => "an array",
        toml::Value::Table(_) => "a table",
    }
}

/// The JSON Schema of `shape`.
pub fn shape_schema(shape: &Shape) -> serde_json::Value {
    use serde_json::json;
    match shape {
        Shape::Str => json!({"type": "string"}),
        Shape::Bool => json!({"type": "boolean"}),
        Shape::Float => json!({"type": "number"}),
        Shape::Int { min, max } => {
            let mut v = json!({"type": "integer"});
            if let Some(m) = min {
                v["minimum"] = json!(m);
            }
            if let Some(m) = max {
                v["maximum"] = json!(m);
            }
            v
        }
        Shape::Enum(options) => json!({"enum": options}),
        Shape::Array(item) => json!({"type": "array", "items": shape_schema(item)}),
        Shape::Map(item) => json!({"type": "object", "additionalProperties": shape_schema(item)}),
        Shape::Table(fields) => {
            let mut props = serde_json::Map::new();
            for f in *fields {
                let mut s = shape_schema(&f.shape);
                if !f.doc.is_empty() {
                    s["description"] = json!(f.doc);
                }
                props.insert(f.name.into(), s);
            }
            let required: Vec<&str> = fields
                .iter()
                .filter(|f| f.required)
                .map(|f| f.name)
                .collect();
            let mut v =
                json!({"type": "object", "properties": props, "additionalProperties": false});
            if !required.is_empty() {
                v["required"] = json!(required);
            }
            v
        }
    }
}

/// `value` with every object's keys in sorted order, whatever map serde_json was built with (a
/// workspace build may turn on `preserve_order`).
fn sorted(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(m) => {
            let mut entries: Vec<_> = m.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            serde_json::Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        serde_json::Value::Array(a) => {
            serde_json::Value::Array(a.into_iter().map(sorted).collect())
        }
        other => other,
    }
}

/// The JSON Schema of a whole file of `kind` (behaviour 4), or `None` for a kind without a body;
/// keys sorted, so the published files do not depend on build features.
pub fn json_schema(kind: &Kind) -> Option<serde_json::Value> {
    json_schema_unsorted(kind).map(sorted)
}

fn json_schema_unsorted(kind: &Kind) -> Option<serde_json::Value> {
    use serde_json::json;
    let Shape::Table(fields) = kind.body? else {
        return None;
    };
    let mut schema = shape_schema(kind.body?);
    let mut envelope = shape_schema(&ENVELOPE);
    envelope["properties"]["kind"] = json!({"const": kind.id, "description": "The kind of file."});
    envelope["properties"]["format"] = json!({
        "type": "integer", "minimum": if kind.extension.is_some() { 1 } else { 0 },
        "maximum": kind.current,
        "description": "Format version; older ones are migrated when read."
    });
    schema["properties"]["idecraft"] = envelope;
    let mut required = vec![json!("idecraft")];
    required.extend(fields.iter().filter(|f| f.required).map(|f| json!(f.name)));
    schema["required"] = json!(required);
    let mut out = serde_json::Map::new();
    out.insert(
        "$schema".into(),
        json!("https://json-schema.org/draft/2020-12/schema"),
    );
    out.insert(
        "title".into(),
        json!(format!(
            "IDECraft {} (kind `{}`, format {})",
            kind.title.to_lowercase(),
            kind.id,
            kind.current
        )),
    );
    out.insert(
        "description".into(),
        json!(match kind.extension {
            Some(ext) => format!(
                "Exchange file `.{ext}` (SPEC-0303). TOML; validated on import with every problem and its key path."
            ),
            None => "Project file (SPEC-0303). TOML; files without [idecraft] are format 0 and still read.".to_owned(),
        }),
    );
    if let serde_json::Value::Object(m) = schema {
        out.extend(m);
    }
    Some(serde_json::Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUN: &str = "[idecraft]\nkind = \"run\"\nformat = 1\n\n[run]\nname = \"test\"\ncommand = [\"cargo\", \"test\"]\n";

    fn messages(r: Result<Document, Vec<Problem>>) -> Vec<String> {
        r.unwrap_err().iter().map(Problem::to_string).collect()
    }

    /// SPEC-0303 AK-1: kind, format, size, syntax and body problems.
    #[test]
    fn envelope_is_checked_and_problems_are_collected() {
        let doc = read(RUN, Some("run"), Use::Exchange).unwrap();
        assert_eq!(doc.envelope.kind, "run");
        assert_eq!(doc.migrated_from, None);
        assert_eq!(doc.body["run"]["name"].as_str(), Some("test"));

        assert_eq!(
            messages(read(RUN, Some("keymap"), Use::Exchange)),
            ["idecraft.kind: is a `run` file, not `keymap`"]
        );
        assert_eq!(
            messages(read(
                &RUN.replace("\"run\"\nformat", "\"song\"\nformat"),
                None,
                Use::Exchange
            )),
            ["idecraft.kind: unknown kind `song`"]
        );
        assert_eq!(
            messages(read("[run]\nname = \"x\"\n", Some("run"), Use::Exchange)),
            [
                "idecraft: not an IDECraft file: the [idecraft] table with kind and format is missing"
            ]
        );
        assert_eq!(
            messages(read(
                &RUN.replace("format = 1", "format = 2"),
                None,
                Use::Exchange
            )),
            [
                "idecraft.format: written by a newer IDECraft (format 2); this version reads run files up to format 1"
            ]
        );
        assert_eq!(
            messages(read(
                &RUN.replace("format = 1", "format = 0"),
                None,
                Use::Exchange
            )),
            ["idecraft.format: 0 is not at least 1"]
        );
        assert_eq!(
            messages(read(
                &RUN.replace("format = 1", "format = \"1\"\nauthr = \"me\""),
                None,
                Use::Exchange
            )),
            [
                "idecraft.authr: unknown key",
                "idecraft.format: expected an integer, found a string"
            ]
        );
        let big = format!("{RUN}# {}\n", "x".repeat(70 * 1024));
        assert!(messages(read(&big, None, Use::Exchange))[0].contains("larger than 64 KiB"));
        assert!(
            read(&big, Some("run"), Use::Project).is_ok(),
            "no limit for project files"
        );
        let syntax = messages(read("[idecraft]\nkind = \n", None, Use::Exchange));
        assert!(
            syntax[0].starts_with("(file): line 2, column"),
            "{syntax:?}"
        );
        let body = RUN.replace(
            "command = [\"cargo\", \"test\"]",
            "command = [\"cargo\", 1]\ntimeout = 0\nkind = \"bench\"\ncolour = \"red\"",
        );
        assert_eq!(
            messages(read(&body, None, Use::Exchange)),
            [
                "run.colour: unknown key",
                "run.command[1]: expected a string, found an integer",
                "run.kind: unknown value `bench` (one of run, test)",
                "run.timeout: 0 is not at least 1",
            ]
        );
        assert_eq!(
            messages(read(
                "[idecraft]\nkind = \"run\"\nformat = 1\n",
                None,
                Use::Exchange
            )),
            ["run: missing"]
        );
    }

    /// SPEC-0303 AK-1: older formats are migrated in memory, step by step.
    #[test]
    fn older_formats_are_migrated_in_memory() {
        fn rename(body: &mut toml::Table) {
            if let Some(v) = body.remove("colour") {
                body.insert("color".into(), v);
            }
        }
        fn upper(body: &mut toml::Table) {
            if let Some(toml::Value::String(s)) = body.get_mut("color") {
                *s = s.to_uppercase();
            }
        }
        const BODY: Shape = Shape::Table(&[req("color", Shape::Str, "")]);
        let kinds = [Kind {
            current: 3,
            migrations: &[
                Migration {
                    from: 1,
                    apply: rename,
                },
                Migration {
                    from: 2,
                    apply: upper,
                },
            ],
            ..exchange("paint", "Paint", "idcpaint", Some(&BODY))
        }];
        let old = "[idecraft]\nkind = \"paint\"\nformat = 1\ncolour = \"red\"\n";
        // `colour` is a top-level key after the table header, so it belongs to [idecraft]:
        assert!(read_with(old, None, Use::Exchange, &kinds).is_err());
        let old = "colour = \"red\"\n[idecraft]\nkind = \"paint\"\nformat = 1\n";
        let doc = read_with(old, None, Use::Exchange, &kinds).unwrap();
        assert_eq!(doc.migrated_from, Some(1));
        assert_eq!(doc.envelope.format, 3);
        assert_eq!(doc.body["color"].as_str(), Some("RED"));
        let two = "color = \"blue\"\n[idecraft]\nkind = \"paint\"\nformat = 2\n";
        let doc = read_with(two, None, Use::Exchange, &kinds).unwrap();
        assert_eq!(doc.migrated_from, Some(2));
        assert_eq!(doc.body["color"].as_str(), Some("BLUE"));
    }

    #[test]
    fn project_files_without_envelope_are_format_zero() {
        let doc = read(
            "interpreter = \".venv/bin/python\"\n",
            Some("python"),
            Use::Project,
        )
        .unwrap();
        assert_eq!(doc.migrated_from, Some(0));
        assert_eq!(
            doc.body["python"]["interpreter"].as_str(),
            Some(".venv/bin/python")
        );
        assert_eq!(
            project_format("[[run]]\nname = \"a\"\n", "run-configurations"),
            Ok(0)
        );
        let text = write(
            &Envelope::current("run-configurations"),
            &toml::toml! { [[run]]
            name = "a" },
        )
        .unwrap();
        assert!(text.starts_with("[idecraft]\nkind = \"run-configurations\"\nformat = 1\n"));
        assert_eq!(project_format(&text, "run-configurations"), Ok(1));
        assert!(project_format(&text, "connections").is_err());
        let newer = text.replace("format = 1", "format = 9");
        assert!(
            project_format(&newer, "run-configurations").unwrap_err()[0]
                .message
                .contains("format 9")
        );
        assert!(
            write(
                &Envelope::current("python"),
                &toml::toml! { interpreter = "x" }
            )
            .is_err(),
            "top-level values would fall into [idecraft]"
        );
        // run.toml is read with or without envelope; a newer one is a problem (behaviour 16).
        let run = "[idecraft]\nkind = \"run-configurations\"\nformat = 2\n\n[[run]]\nname = \"a\"\ncommand = [\"x\"]\n";
        let newer = crate::parse_run_configs(run);
        assert!(newer.configs.is_empty());
        assert!(
            newer.problems[0].contains("newer IDECraft (format 2)"),
            "{newer:?}"
        );
        assert_eq!(
            crate::parse_run_configs(&run.replace("format = 2", "format = 1"))
                .configs
                .len(),
            1
        );
        // The IDE settings, too; saving keeps the old file once.
        let dir = std::env::temp_dir().join(format!("idecraft-format-ide-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.toml");
        std::fs::write(&file, "[editor]\nfont_size = 15\n").unwrap();
        let s = crate::load_ide_settings(&file);
        assert_eq!(s.editor.font_size, 15.0);
        crate::save_ide_settings(&file, &s).unwrap();
        assert!(dir.join("settings.toml.bak").exists());
        assert_eq!(crate::load_ide_settings(&file), s);
        assert_eq!(
            project_format(&std::fs::read_to_string(&file).unwrap(), "ide-settings"),
            Ok(1)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backups_are_kept_once_and_newer_files_are_not_replaced() {
        let dir = std::env::temp_dir().join(format!("idecraft-format-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sql-files.toml");
        prepare_write(&file, "sql-files").unwrap();
        std::fs::write(&file, "[files]\n\"a.sql\" = \"db\"\n").unwrap();
        prepare_write(&file, "sql-files").unwrap();
        assert!(dir.join("sql-files.toml.bak").exists());
        std::fs::write(&file, "[files]\n\"b.sql\" = \"db\"\n").unwrap();
        prepare_write(&file, "sql-files").unwrap();
        assert!(
            std::fs::read_to_string(dir.join("sql-files.toml.bak"))
                .unwrap()
                .contains("a.sql"),
            "the first backup is kept"
        );
        std::fs::write(&file, "[idecraft]\nkind = \"sql-files\"\nformat = 5\n").unwrap();
        let e = prepare_write(&file, "sql-files").unwrap_err().to_string();
        assert!(
            e.contains("format 5") && e.contains("not overwritten"),
            "{e}"
        );
        let old = dir.join("theme");
        std::fs::write(&old, "Light\n").unwrap();
        retire(&old).unwrap();
        assert!(!old.exists() && dir.join("theme.bak").exists());
        std::fs::write(&old, "Dark\n").unwrap();
        retire(&old).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("theme.bak")).unwrap(),
            "Light\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extensions_name_their_kind() {
        let k = |p: &str| kind_for_path(Path::new(p)).map(|k| k.id);
        assert_eq!(k("a/b/test.idcrun"), Some("run"));
        assert_eq!(k("Dark.IDCTHEME"), Some("theme"));
        assert_eq!(k("my.idckeymap.toml"), Some("keymap"));
        assert_eq!(k("crdb.idcconnector"), Some("connector"));
        assert_eq!(k("run.toml"), None);
        assert_eq!(k("idcrun"), None);
        let exts: Vec<_> = KINDS.iter().filter_map(|k| k.extension).collect();
        assert_eq!(exts.len(), 9);
        assert!(exts.iter().all(|e| e.starts_with("idc")));
        assert_eq!(
            find_kind("run").unwrap().file_name("cargo test"),
            "cargo-test.idcrun"
        );
        assert_eq!(safe_stem("../"), "export");
    }

    /// SPEC-0303 AK-2: the published schemas are the ones the shapes give. With
    /// `IDECRAFT_WRITE_SCHEMAS=1` the test writes them.
    #[test]
    fn published_schemas_match_the_shapes() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/formats");
        let write = std::env::var_os("IDECRAFT_WRITE_SCHEMAS").is_some();
        let mut n = 0;
        for kind in KINDS {
            let Some(schema) = json_schema(kind) else {
                continue;
            };
            n += 1;
            let text = serde_json::to_string_pretty(&schema).unwrap() + "\n";
            let file = dir.join(format!("{}.schema.json", kind.id));
            if write {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(&file, &text).unwrap();
            }
            let published = std::fs::read_to_string(&file)
                .unwrap_or_else(|_| panic!("{} missing", file.display()));
            assert_eq!(
                published,
                text,
                "{} differs from the code; run with IDECRAFT_WRITE_SCHEMAS=1",
                file.display()
            );
        }
        assert_eq!(n, 7);
    }

    /// The settings shape accepts everything the IDE settings write (drift guard).
    #[test]
    fn settings_shape_covers_the_ide_settings() {
        let mut s = crate::IdeSettings::default();
        s.appearance.theme = Some("IDECraft Light".into());
        s.code_style
            .indents
            .insert("Go".into(), crate::CodeStyleSettings::default_indent("Go"));
        let mut table = match toml::Value::try_from(&s).unwrap() {
            toml::Value::Table(t) => t,
            _ => unreachable!(),
        };
        // `git` holds the Azure DevOps key exchange exception (ADR-0025 section 1a): a security
        // setting that a shared settings file never carries; `ssh` holds the own-ProxyCommand
        // exception (ADR-0025 v5) for the same reason.
        // `ui` holds whether agents may operate the window (SPEC-0319 behaviour 32): only the user
        // switches it, never a file.
        for private in ["ai", "mcp", "terminal", "database", "git", "ssh", "ui"] {
            table.remove(private);
        }
        let mut problems = Vec::new();
        check(
            &toml::Value::Table(table),
            &SETTINGS,
            "settings",
            &mut problems,
        );
        assert_eq!(problems, []);
    }
}
