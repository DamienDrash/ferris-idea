//! The connection dialog (`SPEC-0108`): name, database system, server or file, user and password,
//! transport security and read-only; Test connects, Save writes the profile and keeps the password
//! in the system keychain. The database system comes from a searchable, grouped list of connectors
//! with their Supported/Beta status (`SPEC-0300` behaviour 7).

pub use ferris_idea_database::Dialect;
use ferris_idea_database::connector::{Field, Origin, Status};
use ferris_idea_database::connector_share;
use ferris_idea_database::{ConnectionForm, ConnectionProfile, Connector, SslMode};
use gpui::{Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};

use crate::editor_view::EditorView;
use crate::palette::Palette;
use crate::semantics::{Annotated, AxAction, Effect, Role, Semantics};
use crate::workbench_view::c;

/// What the dialog asks the workbench to do.
pub enum ConnectionDialogEvent {
    /// Save the form; `id` is the edited connection's, `password` a newly typed one.
    Save {
        /// The edited connection, `None` for a new one.
        id: Option<String>,
        /// The fields.
        form: ConnectionForm,
        /// A password typed into the dialog.
        password: Option<String>,
    },
    /// Connect with the form as it is.
    Test {
        /// The edited connection, `None` for a new one.
        id: Option<String>,
        /// The fields.
        form: ConnectionForm,
        /// A password typed into the dialog.
        password: Option<String>,
    },
    /// Create the empty database file the form names (`SPEC-0108` v13 behaviour 23): SQLite and
    /// DuckDB, only on the user's click.
    CreateFile {
        /// The fields.
        form: ConnectionForm,
    },
    /// Open "Report a problem with this database" (`SPEC-0301`) for the chosen system.
    Report {
        /// The edited connection, `None` for a new one.
        id: Option<String>,
        /// The fields.
        form: ConnectionForm,
        /// The last failed test's message, if any.
        error: Option<String>,
    },
    /// Close without saving.
    Close,
}

/// Text fields of the dialog in Tab order (v4: General, SSH/SSL and Options pages).
const FIELDS: [&str; 20] = [
    "Name",
    "Host",
    "Port",
    "Database",
    "User",
    "Password",
    "File",
    "Comment",
    "URL",
    "SSH host",
    "SSH port",
    "SSH user",
    "CA file",
    "Cert file",
    "Key file",
    "Startup script",
    "Connect timeout",
    "Query timeout",
    "Keep-alive",
    "Schemas",
];

/// Index of the URL field (v4 behaviour 9).
const URL_FIELD: usize = 8;

/// The pages of the dialog (`SPEC-0108` v4 behaviour 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogPage {
    /// System, server, user, password, comment, colour, URL, driver.
    General,
    /// SSH tunnel, SSL mode and certificate files.
    SshSsl,
    /// Auto-commit, startup script, time limits, schema filter.
    Options,
}

impl DialogPage {
    const ALL: [Self; 3] = [Self::General, Self::SshSsl, Self::Options];

    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::SshSsl => "SSH/SSL",
            Self::Options => "Options",
        }
    }
}

/// A whole number of seconds from a field; empty is `None`.
fn seconds(field: &str, text: &str) -> Result<Option<u32>, String> {
    match text.trim() {
        "" => Ok(None),
        t => t
            .parse::<u32>()
            .map(Some)
            .map_err(|_| format!("{field} must be a whole number of seconds")),
    }
}

/// Adds or edits one connection (behaviour 1).
pub struct ConnectionDialog {
    id: Option<String>,
    dialect: Dialect,
    /// Connector on top of the family; `None` is the family's own system.
    system: Option<String>,
    /// The open system list: search field and selected row.
    picker: Option<Picker>,
    /// The open "Import database system…" panel (`SPEC-0302`).
    import: Option<(Entity<crate::ConnectorImport>, gpui::Subscription)>,
    ssl: SslMode,
    read_only: bool,
    /// The shown page (v4).
    page: DialogPage,
    /// Consoles and data editors start in Auto (v4 behaviour 10).
    auto_commit: bool,
    /// The connection's colour (v4 behaviour 9).
    color: Option<ferris_idea_database::ConnectionColor>,
    /// Whether the loaded startup script may run, and the script as loaded (v4 behaviour 10).
    startup: (bool, Option<String>),
    fields: Vec<Entity<EditorView>>,
    /// Whether the edited connection already has a password in the keychain.
    has_password: bool,
    note: Option<(String, bool)>,
    /// The form the last test was made with; a note shown after it is outdated once the fields
    /// differ (v9 behaviour 18).
    tested: Option<ConnectionForm>,
    /// The detail lines of the last successful test and the note they belong to (`SPEC-0108` v3):
    /// shown only while that note is.
    details: Option<(String, Vec<String>)>,
    focus: FocusHandle,
}

impl EventEmitter<ConnectionDialogEvent> for ConnectionDialog {}

/// The New Data Source list (`SPEC-0300` behaviour 7).
struct Picker {
    search: Entity<EditorView>,
    selected: usize,
    _watch: gpui::Subscription,
}

/// The connector of `dialect` and `system`, or the family's own one.
fn connector_of(dialect: Dialect, system: Option<&str>) -> Option<Connector> {
    ferris_idea_database::catalogue()
        .for_profile(dialect, system)
        .cloned()
}

/// Rows of the list for `query`: category headings (`None`) and connectors, in display order.
pub fn picker_rows(query: &str) -> Vec<(String, Option<Connector>)> {
    let catalogue = ferris_idea_database::catalogue();
    let mut rows = Vec::new();
    for (category, items) in catalogue.grouped(query) {
        rows.push((category.label().to_owned(), None));
        for c in items {
            rows.push((c.name.clone(), Some(c.clone())));
        }
    }
    rows
}

fn ssl_label(ssl: SslMode) -> &'static str {
    match ssl {
        SslMode::Disable => "disable",
        SslMode::Prefer => "prefer",
        SslMode::Require => "require",
        SslMode::VerifyCa => "verify-ca",
        SslMode::VerifyFull => "verify-full",
    }
}

/// Width of the field labels: the longest, "Connect timeout", fits on one line.
const LABEL_WIDTH: f32 = 120.;

/// Height of the area under the fields that shows the result of Test Connection or why saving
/// failed (`SPEC-0108` v10 behaviour 20): the note's line and the five detail lines of a test
/// (server, driver, round trip, TLS, file). The area is there from the start, so nothing under it
/// moves when a result arrives; a longer text scrolls inside it.
pub const CONNECTION_RESULT_HEIGHT: f32 = 112.;

/// The next SSL mode of the button (v9 behaviour 17): from `prefer` first to the stricter modes
/// — require, verify-ca, verify-full — and only then to `disable`; one more press comes back to
/// `prefer`. Until v8 the first press from `prefer` turned encryption off.
fn next_ssl(ssl: SslMode) -> SslMode {
    match ssl {
        SslMode::Prefer => SslMode::Require,
        SslMode::Require => SslMode::VerifyCa,
        SslMode::VerifyCa => SslMode::VerifyFull,
        SslMode::VerifyFull => SslMode::Disable,
        SslMode::Disable => SslMode::Prefer,
    }
}

/// Whether the shown test result no longer describes the fields: the form changed since the test
/// started (v9 behaviour 18).
fn test_outdated(tested: Option<&ConnectionForm>, now: &ConnectionForm) -> bool {
    tested.is_some_and(|t| t != now)
}

impl ConnectionDialog {
    /// A dialog for a new connection, or for `profile`.
    pub fn new(profile: Option<&ConnectionProfile>, cx: &mut Context<Self>) -> Self {
        let form = profile.map_or_else(
            || ConnectionForm::new(Dialect::PostgreSql),
            ConnectionForm::from_profile,
        );
        let e = &form.extras;
        let path = |p: &Option<std::path::PathBuf>| {
            p.as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        };
        let secs = |v: Option<u32>| v.map(|v| v.to_string()).unwrap_or_default();
        let texts = [
            form.name.clone(),
            form.host.clone(),
            form.port.clone(),
            form.database.clone(),
            form.user.clone(),
            String::new(),
            form.path.clone(),
            e.comment.clone(),
            // The URL field is for pasting; the current URL is shown under it (v4 behaviour 9).
            String::new(),
            e.ssh.as_ref().map(|s| s.host.clone()).unwrap_or_default(),
            e.ssh
                .as_ref()
                .and_then(|s| s.port)
                .map(|p| p.to_string())
                .unwrap_or_default(),
            e.ssh
                .as_ref()
                .and_then(|s| s.user.clone())
                .unwrap_or_default(),
            path(&e.tls.ca_file),
            path(&e.tls.cert_file),
            path(&e.tls.key_file),
            e.options.startup_script.clone().unwrap_or_default(),
            secs(e.options.connect_timeout_secs),
            secs(e.options.query_timeout_secs),
            secs(e.options.keep_alive_secs),
            e.options.schemas.join(", "),
        ];
        let fields = texts
            .iter()
            .enumerate()
            .map(|(i, text)| {
                cx.new(|cx| {
                    let mut f = if FIELDS[i] == "Password" {
                        EditorView::password_field(cx)
                    } else {
                        EditorView::field(cx)
                    };
                    f.set_text(text, cx);
                    f
                })
            })
            .collect();
        Self {
            id: profile.map(|p| p.id.clone()),
            dialect: form.dialect,
            system: form.system.clone(),
            picker: None,
            import: None,
            ssl: form.ssl,
            read_only: form.read_only,
            page: DialogPage::General,
            auto_commit: form.extras.options.auto_commit,
            color: form.extras.color,
            startup: (
                form.extras.options.startup_allowed,
                form.extras.options.startup_script.clone(),
            ),
            fields,
            has_password: profile.is_some_and(|p| p.secret.is_some()),
            note: None,
            tested: None,
            details: None,
            focus: cx.focus_handle(),
        }
    }

    /// Focus handle of the first field.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.fields[0].read(cx).focus()
    }

    /// The fields as a form; numbers that do not parse are left out (see [`Self::problem`]).
    pub fn form(&self, cx: &gpui::App) -> ConnectionForm {
        let text = |i: usize| self.fields[i].read(cx).state.buffer.text();
        let path = |i: usize| {
            let t = text(i);
            (!t.trim().is_empty()).then(|| std::path::PathBuf::from(t.trim()))
        };
        let opt = |i: usize| {
            let t = text(i);
            (!t.trim().is_empty()).then(|| t.trim().to_owned())
        };
        let ssh = opt(9).map(|host| ferris_idea_database::SshTunnel {
            host,
            port: text(10).trim().parse().ok(),
            user: opt(11),
        });
        let extras = ferris_idea_database::ConnectionExtras {
            ssh,
            tls: ferris_idea_database::TlsFiles {
                ca_file: path(12),
                cert_file: path(13),
                key_file: path(14),
            },
            options: ferris_idea_database::ConnectionOptions {
                auto_commit: self.auto_commit,
                startup_script: opt(15),
                connect_timeout_secs: seconds("", &text(16)).ok().flatten(),
                query_timeout_secs: seconds("", &text(17)).ok().flatten(),
                keep_alive_secs: seconds("", &text(18)).ok().flatten(),
                schemas: text(19)
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
                // A script read from an untrusted project stays blocked until the user changes it
                // (SPEC-0108 v4 behaviour 10).
                startup_allowed: self.startup.0 || opt(15) != self.startup.1,
                // Testing and saving name the workspace themselves.
                base: None,
            },
            color: self.color,
            comment: text(7).trim().to_owned(),
        };
        ConnectionForm {
            name: text(0),
            dialect: self.dialect,
            system: self.system.clone(),
            host: text(1),
            port: text(2),
            database: text(3),
            user: text(4),
            path: text(6),
            ssl: self.ssl,
            read_only: self.read_only,
            extras,
        }
    }

    /// Why the number fields of SSH/SSL and Options cannot be saved, if one does not parse.
    pub fn problem(&self, cx: &gpui::App) -> Option<String> {
        let text = |i: usize| self.fields[i].read(cx).state.buffer.text();
        if !text(10).trim().is_empty() && text(10).trim().parse::<u16>().is_err() {
            return Some("SSH port must be between 1 and 65535".into());
        }
        if text(9).trim().is_empty() && !(text(10).trim().is_empty() && text(11).trim().is_empty())
        {
            return Some("SSH port and user need an SSH host".into());
        }
        for (i, name) in [
            (16, "Connect timeout"),
            (17, "Query timeout"),
            (18, "Keep-alive"),
        ] {
            if let Err(e) = seconds(name, &text(i)) {
                return Some(e);
            }
        }
        None
    }

    /// The shown page (v4 behaviour 1).
    pub fn page(&self) -> DialogPage {
        self.page
    }

    /// Shows `page` and focuses its first field.
    pub fn set_page(&mut self, page: DialogPage, window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        if let Some(&first) = self.shown().first() {
            window.focus(&self.fields[first].read(cx).focus());
        }
        cx.notify();
    }

    /// Chooses the SSL mode (tests; the button cycles).
    pub fn set_ssl(&mut self, ssl: SslMode, cx: &mut Context<Self>) {
        self.ssl = ssl;
        cx.notify();
    }

    /// The URL of the form as it is, never with a password (v4 behaviour 9).
    pub fn url(&self, cx: &gpui::App) -> Option<String> {
        self.form(cx).url()
    }

    /// Takes server, port, database, user and SSL mode from the URL field (v4 behaviour 9); a URL
    /// with a password is refused, changes nothing and the field is emptied, so the password is
    /// neither kept nor shown.
    pub fn apply_url_field(&mut self, cx: &mut Context<Self>) {
        let typed = self.fields[URL_FIELD].read(cx).state.buffer.text();
        if typed.trim().is_empty() {
            return;
        }
        let mut form = self.form(cx);
        match form.apply_url(&typed) {
            Ok(()) => {
                self.dialect = form.dialect;
                self.system = form.system.clone();
                self.ssl = form.ssl;
                for (i, value) in [
                    (1, &form.host),
                    (2, &form.port),
                    (3, &form.database),
                    (4, &form.user),
                    (6, &form.path),
                ] {
                    self.fields[i].update(cx, |f, cx| f.set_text(value, cx));
                }
                self.note = Some(("Fields taken from the URL".into(), false));
            }
            Err(e) => self.note = Some((e.to_string(), true)),
        }
        self.fields[URL_FIELD].update(cx, |f, cx| f.set_text("", cx));
        cx.notify();
    }

    /// Turns automatic commit on or off for this connection's consoles and editors (v4).
    pub fn toggle_auto_commit(&mut self, cx: &mut Context<Self>) {
        self.auto_commit = !self.auto_commit;
        cx.notify();
    }

    /// The next colour (none after the last, v4 behaviour 9).
    pub fn next_color(&mut self, cx: &mut Context<Self>) {
        self.color = ferris_idea_database::ConnectionColor::next(self.color);
        cx.notify();
    }

    fn password(&self, cx: &gpui::App) -> Option<String> {
        let p = self.fields[5].read(cx).state.buffer.text();
        (!p.is_empty()).then_some(p)
    }

    /// Sets the text of field `name` (one of the labels), for tests and pasting a URL.
    pub fn set_field(&mut self, name: &str, text: &str, cx: &mut Context<Self>) {
        if let Some(i) = FIELDS.iter().position(|f| *f == name) {
            self.fields[i].update(cx, |f, cx| f.set_text(text, cx));
        }
    }

    /// Fills the dialog for a new connection from `form` (a data source of the project's `.idea`,
    /// `SPEC-0089` v16 behaviour 28) and says where it came from; nothing is saved here.
    pub fn fill(&mut self, form: &ConnectionForm, note: &str, cx: &mut Context<Self>) {
        self.dialect = form.dialect;
        self.system = form.system.clone();
        self.ssl = form.ssl;
        self.read_only = form.read_only;
        for (i, value) in [
            (0, &form.name),
            (1, &form.host),
            (2, &form.port),
            (3, &form.database),
            (4, &form.user),
            (6, &form.path),
        ] {
            self.fields[i].update(cx, |f, cx| f.set_text(value, cx));
        }
        self.note = Some((note.to_owned(), false));
        cx.notify();
    }

    /// Chooses the family's own database system (behaviour 2).
    pub fn set_dialect(&mut self, dialect: Dialect, cx: &mut Context<Self>) {
        self.dialect = dialect;
        self.system = None;
        cx.notify();
    }

    /// The chosen database system.
    pub fn connector(&self) -> Option<Connector> {
        connector_of(self.dialect, self.system.as_deref())
    }

    /// Chooses the connector with `id` (`SPEC-0300` behaviour 7): its family, and its defaults for
    /// database, user and SSL mode where the form is still empty. Unknown ids change nothing.
    pub fn set_system(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(connector) = ferris_idea_database::catalogue().get(id).cloned() else {
            return;
        };
        let defaults = ConnectionForm::for_connector(&connector);
        self.dialect = defaults.dialect;
        self.system = defaults.system.clone();
        for (i, value) in [(3, &defaults.database), (4, &defaults.user)] {
            if !value.is_empty() && self.fields[i].read(cx).state.buffer.text().is_empty() {
                self.fields[i].update(cx, |f, cx| f.set_text(value, cx));
            }
        }
        if let Some(ssl) = connector.connection.ssl {
            self.ssl = ssl;
        }
        if connector.has_field(Field::Host)
            && self.fields[1].read(cx).state.buffer.text().is_empty()
        {
            self.fields[1].update(cx, |f, cx| f.set_text("localhost", cx));
        }
        cx.notify();
    }

    /// Opens "Import database system…" (`SPEC-0302` behaviour 1) in place of the system list.
    pub fn open_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picker = None;
        let panel = cx.new(crate::ConnectorImport::new);
        let watch = cx.subscribe_in(
            &panel,
            window,
            |this, _, event: &crate::ConnectorImportEvent, window, cx| {
                match event {
                    crate::ConnectorImportEvent::Imported(id) => {
                        this.set_system(id, cx);
                        let name = this.connector().map(|c| c.name).unwrap_or_default();
                        this.note = Some((format!("Imported {name}"), false));
                    }
                    crate::ConnectorImportEvent::Cancel => {}
                }
                this.import = None;
                window.focus(&this.fields[0].read(cx).focus());
                cx.notify();
            },
        );
        window.focus(&panel.read(cx).focus(cx));
        self.import = Some((panel, watch));
        cx.notify();
    }

    /// The open import panel.
    pub fn import_panel(&self) -> Option<Entity<crate::ConnectorImport>> {
        self.import.as_ref().map(|(p, _)| p.clone())
    }

    /// Writes the chosen system's definition to `path` (`SPEC-0302` behaviour 4).
    pub fn export_definition_to(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let Some(c) = self.connector() else {
            return;
        };
        self.note = Some(match std::fs::write(path, connector_share::export(&c)) {
            Ok(()) => (format!("Exported {} to {}", c.name, path.display()), false),
            Err(e) => (format!("{}: {e}", path.display()), true),
        });
        cx.notify();
    }

    /// Asks where to save the chosen system's definition, then writes it.
    pub fn export_definition(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.connector() else {
            return;
        };
        let dir = std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join("Downloads"))
            .filter(|d| d.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let chosen = cx.prompt_for_new_path(&dir, Some(&connector_share::export_file_name(&c)));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = chosen.await {
                let _ = this.update(cx, |this, cx| this.export_definition_to(&path, cx));
            }
        })
        .detach();
    }

    /// Copies the definition and opens a prefilled issue to share it (`SPEC-0302` behaviour 4).
    pub fn share_definition(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.connector() else {
            return;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(connector_share::export(&c)));
        let (url, cut) = connector_share::share_url(&c);
        cx.open_url(&url);
        self.note = Some((
            if cut {
                "Opened the issue form; the definition is on the clipboard — attach it as a file."
                    .into()
            } else {
                "Opened the issue form in the browser; the definition is also on the clipboard."
                    .into()
            },
            false,
        ));
        cx.notify();
    }

    /// Removes the chosen user or imported system (`SPEC-0302` behaviour 6).
    pub fn remove_system(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.connector().filter(|c| c.origin != Origin::BuiltIn) else {
            return;
        };
        let Some(dir) = ferris_idea_database::connector::connectors_dir() else {
            return;
        };
        let result = connector_share::remove(&dir, &c.id, &ferris_idea_database::catalogue());
        ferris_idea_database::reload_catalogue();
        self.note = Some(match result {
            Ok(name) => {
                let family = c.family.dialect().unwrap_or(Dialect::PostgreSql);
                self.set_dialect(family, cx);
                (format!("Removed {name}"), false)
            }
            Err(e) => (e, true),
        });
        cx.notify();
    }

    /// Whether the system list is open.
    pub fn picker_open(&self) -> bool {
        self.picker.is_some()
    }

    /// Opens the system list with its search field focused, or closes it.
    pub fn toggle_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.take().is_some() {
            window.focus(&self.fields[0].read(cx).focus());
        } else {
            let search = cx.new(EditorView::field);
            let watch = cx.observe(&search, |this, _, cx| {
                if let Some(p) = &mut this.picker {
                    p.selected = 0;
                }
                cx.notify();
            });
            window.focus(&search.read(cx).focus());
            let current = self.connector().map(|c| c.id);
            let rows = picker_rows("");
            let selected = rows
                .iter()
                .filter(|(_, c)| c.is_some())
                .position(|(_, c)| c.as_ref().map(|c| &c.id) == current.as_ref())
                .unwrap_or(0);
            self.picker = Some(Picker {
                search,
                selected,
                _watch: watch,
            });
        }
        cx.notify();
    }

    /// Types `text` into the open system list's search field (tests).
    pub fn set_picker_query(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.picker {
            p.selected = 0;
            p.search.update(cx, |f, cx| f.set_text(text, cx));
        }
        cx.notify();
    }

    /// The connectors the open list shows, in order, and the selected one's index.
    pub fn picker_items(&self, cx: &gpui::App) -> (Vec<Connector>, usize) {
        let Some(p) = &self.picker else {
            return (Vec::new(), 0);
        };
        let query = p.search.read(cx).state.buffer.text();
        let items: Vec<Connector> = picker_rows(&query)
            .into_iter()
            .filter_map(|(_, c)| c)
            .collect();
        let selected = p.selected.min(items.len().saturating_sub(1));
        (items, selected)
    }

    fn pick_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (items, selected) = self.picker_items(cx);
        if let Some(c) = items.get(selected) {
            let id = c.id.clone();
            self.set_system(&id, cx);
        }
        self.picker = None;
        window.focus(&self.fields[0].read(cx).focus());
        cx.notify();
    }

    /// The note under the fields: the test result or why saving failed, and whether it is an error.
    pub fn note(&self) -> Option<&(String, bool)> {
        self.note.as_ref()
    }

    /// Shows `text` under the fields.
    pub fn set_note(&mut self, text: String, error: bool, cx: &mut Context<Self>) {
        self.note = Some((text, error));
        cx.notify();
    }

    /// Shows a successful test (behaviour 6, v3): `summary` as the note and `details` (server and
    /// version, driver, round trip, TLS, file) under it.
    pub fn set_test_result(
        &mut self,
        summary: String,
        details: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        self.details = (!details.is_empty()).then(|| (summary.clone(), details));
        self.note = Some((summary, false));
        cx.notify();
    }

    /// Whether the note shown is a test result the fields no longer match (v9 behaviour 18).
    pub fn note_outdated(&self, cx: &gpui::App) -> bool {
        test_outdated(self.tested.as_ref(), &self.form(cx))
    }

    /// The detail lines of the test result shown, empty when none is (v3).
    pub fn test_details(&self) -> &[String] {
        match (&self.details, &self.note) {
            (Some((owner, lines)), Some((note, false))) if owner == note => lines,
            _ => &[],
        }
    }

    /// The shown test result as plain text, what Copy puts on the clipboard (v3). Holds no password:
    /// the lines come from the test, which never sees the password field.
    pub fn test_text(&self) -> Option<String> {
        let lines = self.test_details();
        let (note, _) = self.note.as_ref().filter(|_| !lines.is_empty())?;
        Some(
            std::iter::once(note.as_str())
                .chain(lines.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    /// Copies the shown test result (v3).
    pub fn copy_test_result(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.test_text() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        }
    }

    /// Asks the workbench to connect with the form (behaviour 6).
    pub fn test(&mut self, cx: &mut Context<Self>) {
        self.details = None;
        self.note = Some(("Connecting…".into(), false));
        self.tested = Some(self.form(cx));
        cx.emit(ConnectionDialogEvent::Test {
            id: self.id.clone(),
            form: self.form(cx),
            password: self.password(cx),
        });
        cx.notify();
    }

    /// Whether "Create File" is offered (`SPEC-0108` v13 behaviour 23): for a database in a file
    /// (SQLite, DuckDB).
    pub fn offers_create_file(&self) -> bool {
        self.dialect.file_based() && self.page == DialogPage::General
    }

    /// Asks the workbench to create the empty database file the File field names (v13 behaviour
    /// 23). Testing never does.
    pub fn create_file(&mut self, cx: &mut Context<Self>) {
        if !self.offers_create_file() {
            return;
        }
        self.details = None;
        self.note = Some(("Creating the file…".into(), false));
        cx.emit(ConnectionDialogEvent::CreateFile {
            form: self.form(cx),
        });
        cx.notify();
    }

    /// Whether "Report a problem with this database…" is offered (`SPEC-0301` behaviour 1): after a
    /// failed test, and for systems Ferris IDEA does not test (Beta, User, Imported).
    pub fn offers_report(&self) -> bool {
        self.note.as_ref().is_some_and(|(_, error)| *error)
            || self
                .connector()
                .is_some_and(|c| c.status != Status::Supported)
    }

    /// Asks the workbench to open the problem report.
    pub fn report(&mut self, cx: &mut Context<Self>) {
        let error = self
            .note
            .as_ref()
            .filter(|(_, error)| *error)
            .map(|(text, _)| text.clone());
        cx.emit(ConnectionDialogEvent::Report {
            id: self.id.clone(),
            form: self.form(cx),
            error,
        });
    }

    /// Asks the workbench to save the form (behaviour 3).
    pub fn save(&mut self, cx: &mut Context<Self>) {
        if let Some(problem) = self.problem(cx) {
            self.set_note(problem, true, cx);
            return;
        }
        cx.emit(ConnectionDialogEvent::Save {
            id: self.id.clone(),
            form: self.form(cx),
            password: self.password(cx),
        });
    }

    /// The fields shown on the page for the chosen database system (v4: per page).
    fn shown(&self) -> Vec<usize> {
        match self.page {
            DialogPage::General => {
                let mut shown = self.general_fields();
                shown.extend([7, URL_FIELD]);
                shown
            }
            DialogPage::SshSsl => ssh_ssl_fields(
                self.dialect,
                self.connector().is_none_or(|c| c.has_field(Field::Host)),
            ),
            DialogPage::Options => vec![15, 16, 17, 18, 19],
        }
    }

    /// Whether the chosen system has a tunnel and certificate files (v4 behaviour 8).
    fn has_ssh_ssl(&self) -> bool {
        matches!(
            self.dialect,
            Dialect::PostgreSql | Dialect::MySql | Dialect::MariaDb
        ) && self.connector().is_none_or(|c| c.has_field(Field::Host))
    }

    /// The fields of the General page for the chosen database system.
    fn general_fields(&self) -> Vec<usize> {
        let Some(connector) = self.connector() else {
            return if self.dialect.file_based() {
                vec![0, 6]
            } else {
                vec![0, 1, 2, 3, 4, 5]
            };
        };
        let mut shown = vec![0];
        for (field, i) in [
            (Field::Host, 1),
            (Field::Port, 2),
            (Field::Database, 3),
            (Field::User, 4),
            (Field::Password, 5),
            (Field::File, 6),
        ] {
            if connector.has_field(field) {
                shown.push(i);
            }
        }
        shown
    }

    /// Tab and Shift+Tab move between the fields, Enter saves, Escape closes; taken before the
    /// fields see them (behaviour 1).
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if let Some((panel, _)) = &self.import {
            let panel = panel.clone();
            match k.key.as_str() {
                "escape" => {
                    self.import = None;
                    window.focus(&self.fields[0].read(cx).focus());
                }
                "enter" => panel.update(cx, |p, cx| {
                    if p.review().is_some_and(|r| r.ok()) {
                        p.save(cx);
                    } else {
                        p.load(cx);
                    }
                }),
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.picker.is_some() {
            match k.key.as_str() {
                "up" | "down" => {
                    let (items, selected) = self.picker_items(cx);
                    let n = items.len().max(1);
                    let next = if k.key == "up" {
                        (selected + n - 1) % n
                    } else {
                        (selected + 1) % n
                    };
                    if let Some(p) = &mut self.picker {
                        p.selected = next;
                    }
                }
                "enter" => self.pick_selected(window, cx),
                "escape" => self.toggle_picker(window, cx),
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        match k.key.as_str() {
            "tab" => {
                let shown = self.shown();
                let at = shown
                    .iter()
                    .position(|&i| self.fields[i].read(cx).focus().is_focused(window))
                    .unwrap_or(0);
                let n = shown.len();
                if n == 0 {
                    return;
                }
                let next = if k.modifiers.shift {
                    (at + n - 1) % n
                } else {
                    (at + 1) % n
                };
                window.focus(&self.fields[shown[next]].read(cx).focus());
            }
            // Enter in the URL field takes the URL apart instead of saving (v4 behaviour 9).
            "enter" if self.fields[URL_FIELD].read(cx).focus().is_focused(window) => {
                self.apply_url_field(cx)
            }
            "enter" => self.save(cx),
            "escape" => cx.emit(ConnectionDialogEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for ConnectionDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        let dialect = self.dialect;
        let button =
            |id: &'static str, label: String, on: bool| crate::metrics::button(id, label, on);
        let connector = self.connector();
        let system_name = connector
            .as_ref()
            .map_or_else(|| dialect.label().to_owned(), |c| c.name.clone());
        let status = connector.as_ref().map_or(Status::Supported, |c| c.status);
        let default_port = connector
            .as_ref()
            .and_then(|c| c.connection.default_port)
            .map(|p| format!("{p} when empty"))
            .unwrap_or_else(|| "default port when empty".into());
        let note = connector.as_ref().map(system_line);
        let show_ssl = connector
            .as_ref()
            .map_or(!dialect.file_based(), |c| c.has_field(Field::Ssl));
        let picker = self.picker.as_ref().map(|picker| {
            let query = picker.search.read(cx).state.buffer.text();
            let (items, selected) = self.picker_items(cx);
            let selected_id = items.get(selected).map(|c| c.id.clone());
            let list = picker_rows(&query)
                .into_iter()
                .map(|(label, item)| match item {
                    None => div()
                        .pt_1()
                        .text_xs()
                        .text_color(c(p.ui_dim))
                        .child(label)
                        .into_any_element(),
                    Some(item) => {
                        let on = selected_id.as_deref() == Some(item.id.as_str());
                        let id = item.id.clone();
                        div()
                            .id(gpui::SharedString::from(format!("conn-system-{}", item.id)))
                            .flex()
                            .items_center()
                            .h(px(24.))
                            .px_2()
                            .rounded(px(4.))
                            .when(on, |d| d.bg(c(p.tree_selection)))
                            .child(div().flex_1().child(label))
                            .when_some(item.status.badge(), |d, b| {
                                d.child(status_badge(b, item.status))
                            })
                            .when_some(item.shown_note(), |d, note| {
                                d.tooltip(move |_, cx| cx.new(|_| NoteTip(note.clone())).into())
                            })
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                                this.set_system(&id, cx);
                                this.picker = None;
                                window.focus(&this.fields[0].read(cx).focus());
                                cx.notify();
                            }))
                            .into_any_element()
                    }
                });
            div()
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .border_1()
                .border_color(c(p.separator))
                .rounded(px(6.))
                .child(crate::metrics::field_frame().child(picker.search.clone()))
                .child(
                    div()
                        .id("conn-system-list")
                        .max_h(px(260.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .children(list),
                )
                .child(
                    div()
                        .id("conn-system-import")
                        .px_2()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .text_color(c(p.link))
                        .cursor_pointer()
                        .child("Import database system…")
                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                            this.open_import(window, cx)
                        })),
                )
        });
        let import = self.import.as_ref().map(|(panel, _)| panel.clone());
        let user_system = connector
            .as_ref()
            .is_some_and(|c| c.origin != Origin::BuiltIn);
        let link = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .text_xs()
                .text_color(c(p.link))
                .cursor_pointer()
                .child(label)
        };
        let actions = div()
            .flex()
            .gap_3()
            .child(link("conn-system-export", "Export definition…").on_click(
                cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.export_definition(cx)),
            ))
            .child(link("conn-system-share", "Share…").on_click(
                cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.share_definition(cx)),
            ))
            .when(user_system, |d| {
                d.child(link("conn-system-remove", "Remove").on_click(
                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.remove_system(cx)),
                ))
            });
        let rows = self.shown().into_iter().map(|i| {
            let hint = match (FIELDS[i], self.has_password) {
                ("Password", true) => "stored in the keychain; type to replace".to_owned(),
                ("Password", false) => "kept in the system keychain".to_owned(),
                ("Port", _) => default_port.clone(),
                ("URL", _) => "paste a URL, Enter fills the fields".to_owned(),
                ("SSH host", _) => "alias or host; keys and agent only".to_owned(),
                ("SSH port" | "SSH user", _) => "from ~/.ssh/config when empty".to_owned(),
                ("CA file", _) => "PEM; with verify-ca or verify-full".to_owned(),
                ("Cert file", _) => "PEM, or .p12 (MySQL: .p12)".to_owned(),
                ("Key file", _) => "PEM, mode 600, no passphrase".to_owned(),
                ("Connect timeout", _) => "seconds, 10 when empty".to_owned(),
                ("Query timeout" | "Keep-alive", _) => "seconds".to_owned(),
                ("Schemas", _) => "e.g. app*, audit; all when empty".to_owned(),
                _ => String::new(),
            };
            div()
                .flex()
                .items_center()
                .gap_2()
                .h(px(28.))
                // Wide enough for "Connect timeout" on one line (v9 behaviour 19).
                .child(
                    div()
                        .w(px(LABEL_WIDTH))
                        .flex_none()
                        .whitespace_nowrap()
                        .text_color(c(p.ui_muted))
                        .child(FIELDS[i]),
                )
                .child(
                    crate::metrics::field_frame()
                        .flex_1()
                        .child(self.fields[i].clone()),
                )
                .when(!hint.is_empty(), |d| {
                    d.child(
                        div()
                            .w(px(200.))
                            .text_xs()
                            .text_color(c(p.ui_dim))
                            .child(hint),
                    )
                })
        });
        div()
            .id("connection-dialog")
            .track_focus(&self.focus)
            .occlude()
            .capture_key_down(cx.listener(Self::on_key))
            .w(px(620.))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(c(p.island))
            .border_1()
            .border_color(c(p.separator))
            .rounded(px(8.))
            .shadow_lg()
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(if self.id.is_some() {
                        "Edit Connection"
                    } else {
                        "New Connection"
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(28.))
                    .child(
                        div()
                            .w(px(LABEL_WIDTH))
                            .flex_none()
                            .whitespace_nowrap()
                            .text_color(c(p.ui_muted))
                            .child("System"),
                    )
                    .child(
                        crate::metrics::button(
                            "conn-system",
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(system_name)
                                .when_some(status.badge(), |d, b| d.child(status_badge(b, status)))
                                .child("▾"),
                            false,
                        )
                        .on_click(cx.listener(
                            |this, _: &gpui::ClickEvent, window, cx| this.toggle_picker(window, cx),
                        )),
                    ),
            )
            .when_some(picker, |d, picker| d.child(picker))
            .when_some(import, |d, import| d.child(import))
            .when(self.picker.is_none() && self.import.is_none(), |d| {
                d.child(actions)
            })
            .when_some(note, |d, note| {
                d.child(
                    div()
                        .id("conn-system-note")
                        .text_xs()
                        .text_color(c(p.ui_muted))
                        .child(note),
                )
            })
            // SPEC-0108 v4 behaviour 1: General, SSH/SSL and Options.
            .child(
                div().flex().gap_1().children(DialogPage::ALL.map(|page| {
                    button(
                        match page {
                            DialogPage::General => "conn-page-general",
                            DialogPage::SshSsl => "conn-page-ssh-ssl",
                            DialogPage::Options => "conn-page-options",
                        },
                        page.label().into(),
                        page == self.page,
                    )
                    .on_click(cx.listener(
                        move |this, _: &gpui::ClickEvent, window, cx| {
                            this.set_page(page, window, cx)
                        },
                    ))
                })),
            )
            .children(rows)
            .when(self.page == DialogPage::General, |d| {
                let url = self.url(cx).unwrap_or_else(|| "—".into());
                d.child(
                    div()
                        .id("conn-general-extras")
                        .flex()
                        .flex_col()
                        .gap_1()
                        .text_xs()
                        .text_color(c(p.ui_muted))
                        .child(div().id("conn-url").child(format!("URL: {url}")))
                        .child(div().id("conn-driver").child(format!(
                            "Driver: {}",
                            ferris_idea_database::driver_label(dialect)
                        ))),
                )
                .child(
                    button(
                        "conn-color",
                        format!(
                            "Colour: {}",
                            self.color.map_or("none", |c| c.name())
                        ),
                        false,
                    )
                    .when_some(self.color, |b, color| {
                        b.border_l_4().border_color(c(ferris_idea_ui::theme::Rgba {
                            rgb: color.rgb(),
                            alpha: 255,
                        }))
                    })
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        this.next_color(cx)
                    })),
                )
            })
            .when(self.page == DialogPage::SshSsl && !self.has_ssh_ssl(), |d| {
                d.child(
                    div()
                        .text_xs()
                        .text_color(c(p.ui_muted))
                        .child(if self.dialect == Dialect::SqlServer {
                            "SQL Server takes a CA file; SSH tunnels and client certificates are available for PostgreSQL, MySQL and MariaDB."
                        } else {
                            "SSH tunnels and certificate files are available for PostgreSQL, MySQL and MariaDB."
                        }),
                )
            })
            .when(self.page == DialogPage::Options, |d| {
                d.child(
                    button(
                        "conn-auto-commit",
                        format!(
                            "{} Auto-commit in consoles and editors",
                            if self.auto_commit { "☑" } else { "☐" }
                        ),
                        false,
                    )
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        this.toggle_auto_commit(cx)
                    })),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(show_ssl && self.page != DialogPage::Options, |d| {
                        d.child(
                            button("conn-ssl", format!("SSL: {}", ssl_label(self.ssl)), false)
                                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    this.ssl = next_ssl(this.ssl);
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        button(
                            "conn-read-only",
                            format!("{} Read-only", if self.read_only { "☑" } else { "☐" }),
                            false,
                        )
                        .on_click(cx.listener(
                            |this, _: &gpui::ClickEvent, _, cx| {
                                this.read_only = !this.read_only;
                                cx.notify();
                            },
                        )),
                    ),
            )
            // The result area has its height from the start and scrolls a longer text, so the
            // buttons under it stay where they are when a test ends (v10 behaviour 20).
            .child(
                div()
                    .id("conn-result")
                    .relative()
                    .flex_none()
                    .h(px(CONNECTION_RESULT_HEIGHT))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .semantics(
                        Semantics::new(Role::Status)
                            .name("Test result")
                            .test_id("conn-result"),
                    )
                    .when_some(self.note.clone(), |d, (text, error)| {
                        // A test result the fields no longer match is marked outdated, not shown
                        // as if it still held (v9 behaviour 18).
                        let outdated = self.note_outdated(cx);
                        d.child(
                            div()
                                .when(error && !outdated, |d| {
                                    d.text_color(c(p.status_text(crate::palette::Status::Error)))
                                })
                                .when(!error || outdated, |d| d.text_color(c(p.ui_muted)))
                                .child(if outdated {
                                    format!("Outdated — the fields changed since the test: {text}")
                                } else {
                                    text
                                }),
                        )
                    })
                    .when(
                        !self.test_details().is_empty() && !self.note_outdated(cx),
                        |d| {
                            d.child(
                                div()
                                    .id("conn-test-details")
                                    .flex()
                                    .items_start()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(c(p.ui_muted))
                                    .child(
                                        div().flex().flex_col().children(
                                            self.test_details()
                                                .iter()
                                                .cloned()
                                                .map(|l| div().child(l)),
                                        ),
                                    )
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .id("conn-test-copy")
                                            .relative()
                                            .text_color(c(p.link))
                                            .cursor_pointer()
                                            .child("Copy")
                                            .semantics(
                                                Semantics::new(Role::Button)
                                                    .name("Copy")
                                                    .test_id("conn-test-copy")
                                                    .action(AxAction::Click),
                                            )
                                            .on_click(cx.listener(
                                                |this, _: &gpui::ClickEvent, _, cx| {
                                                    this.copy_test_result(cx)
                                                },
                                            )),
                                    ),
                            )
                        },
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button("conn-test", "Test Connection".into(), false)
                            .relative()
                            .semantics(
                                Semantics::new(Role::Button)
                                    .name("Test Connection")
                                    .test_id("conn-test")
                                    .action(AxAction::Click),
                            )
                            .on_click(
                                cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.test(cx)),
                            ),
                    )
                    .when(self.offers_create_file(), |d| {
                        d.child(
                            button("conn-create-file", "Create File".into(), false)
                                .relative()
                                .semantics(
                                    Semantics::new(Role::Button)
                                        .name("Create File")
                                        .test_id("conn-create-file")
                                        .action(AxAction::Click)
                                        .effects(&[Effect::Write]),
                                )
                                .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                    this.create_file(cx)
                                })),
                        )
                    })
                    .when(self.offers_report(), |d| {
                        d.child(
                            div()
                                .id("conn-report")
                                .text_xs()
                                .text_color(c(p.link))
                                .cursor_pointer()
                                .child("Report a problem with this database…")
                                .on_click(
                                    cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                        this.report(cx)
                                    }),
                                ),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        button("conn-cancel", "Cancel".into(), false)
                            .relative()
                            .semantics(
                                Semantics::new(Role::Button)
                                    .name("Cancel")
                                    .test_id("conn-cancel")
                                    .action(AxAction::Click)
                                    .effects(&[Effect::None]),
                            )
                            .on_click(cx.listener(|_, _: &gpui::ClickEvent, _, cx| {
                                cx.emit(ConnectionDialogEvent::Close)
                            })),
                    )
                    .child(
                        button("conn-save", "Save".into(), true)
                            .relative()
                            .semantics(
                                Semantics::new(Role::Button)
                                    .name("Save")
                                    .test_id("conn-save")
                                    .action(AxAction::Click)
                                    .effects(&[Effect::Write]),
                            )
                            .on_click(
                                cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.save(cx)),
                            ),
                    ),
            )
    }
}

/// The small status label of a list row (`SPEC-0300` behaviour 7).
fn status_badge(text: &'static str, status: Status) -> gpui::Div {
    let p = Palette::current();
    let colour = match status {
        Status::Beta => p.status_text(crate::palette::Status::Warning),
        _ => p.status_text(crate::palette::Status::Info),
    }
    .rgb;
    div()
        .px_1()
        .text_xs()
        .rounded(px(3.))
        .border_1()
        .border_color(c(ferris_idea_ui::theme::Rgba {
            rgb: colour,
            alpha: 255,
        }))
        .text_color(c(ferris_idea_ui::theme::Rgba {
            rgb: colour,
            alpha: 255,
        }))
        .child(text)
}

/// Tooltip of a list row: the system's note.
struct NoteTip(String);

impl Render for NoteTip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::current();
        div()
            .max_w(px(360.))
            .px_2()
            .py_1()
            .rounded(px(4.))
            .border_1()
            .border_color(c(p.popup_border))
            .bg(c(p.popup))
            .text_color(c(p.ui_text))
            .text_size(px(12.))
            .child(self.0.clone())
    }
}

/// The line under the system: how far Ferris IDEA vouches for it, then the definition's note
/// (`SPEC-0116` v3 behaviour 10).
pub fn system_line(connector: &Connector) -> String {
    match connector.shown_note() {
        Some(note) => format!("{}. {note}", connector.test_level()),
        None => connector.test_level(),
    }
}

/// The fields of the SSH/SSL page for `dialect` (v4 behaviour 8): tunnel and certificate files
/// for the PostgreSQL and MySQL families, only the CA file for SQL Server (`SPEC-0116` v6
/// behaviour 15), none for the others or for a system without a server address.
fn ssh_ssl_fields(dialect: Dialect, has_host: bool) -> Vec<usize> {
    match dialect {
        _ if !has_host => Vec::new(),
        Dialect::PostgreSql | Dialect::MySql | Dialect::MariaDb => vec![9, 10, 11, 12, 13, 14],
        Dialect::SqlServer => vec![12],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0116 v6 AK-10: SQL Server's SSH/SSL page has the CA file only.
    #[test]
    fn sql_server_shows_only_the_ca_file() {
        assert_eq!(ssh_ssl_fields(Dialect::SqlServer, true), [12]);
        assert_eq!(FIELDS[12], "CA file");
        assert_eq!(
            ssh_ssl_fields(Dialect::PostgreSql, true),
            [9, 10, 11, 12, 13, 14]
        );
        assert!(ssh_ssl_fields(Dialect::Oracle, true).is_empty());
        assert!(ssh_ssl_fields(Dialect::SqlServer, false).is_empty());
    }

    /// SPEC-0116 v3 AK-6: the line names the tested version and the note.
    #[test]
    fn the_test_level_line() {
        let c = ferris_idea_database::Catalogue::built_in();
        let find = |id: &str| c.connectors.iter().find(|k| k.id == id).unwrap().clone();
        let line = system_line(&find("sqlserver"));
        assert!(line.starts_with("Tested with SQL Server 2022"), "{line}");
        assert!(line.contains("do not run on every build"), "{line}");
        let line = system_line(&find("db2"));
        assert!(
            line.starts_with("Beta: Ferris IDEA has not tested"),
            "{line}"
        );
        assert!(line.contains("ODBC"), "{line}");
        assert_eq!(
            system_line(&find("postgresql")),
            format!("Tested with {}", find("postgresql").tested.unwrap())
        );
    }

    /// SPEC-0108 v9 AK-20: the SSL button goes from `prefer` to the stricter modes first and
    /// reaches `disable` last; a test result is outdated once the form differs.
    #[test]
    fn ssl_goes_stricter_first_and_a_changed_form_outdates_the_test() {
        let mut mode = SslMode::Prefer;
        let mut seen = Vec::new();
        for _ in 0..5 {
            mode = next_ssl(mode);
            seen.push(ssl_label(mode));
        }
        assert_eq!(
            seen,
            ["require", "verify-ca", "verify-full", "disable", "prefer"]
        );
        let tested = ConnectionForm::new(Dialect::PostgreSql);
        assert!(!test_outdated(None, &tested));
        assert!(!test_outdated(Some(&tested), &tested.clone()));
        let mut changed = tested.clone();
        changed.host = "db.example.org".into();
        assert!(test_outdated(Some(&tested), &changed));
        // The longest label fits in the label column at the dialog's font (about 7 px a letter).
        let longest = FIELDS.iter().map(|f| f.chars().count()).max().unwrap();
        assert!(longest as f32 * 7.5 <= LABEL_WIDTH, "{longest}");
    }

    #[test]
    fn typing_redis_selects_redis_and_picking_it_makes_a_store_form() {
        let first = picker_rows("redis")
            .into_iter()
            .find_map(|(_, c)| c)
            .expect("a connector matches");
        assert_eq!(first.id, "redis");
        assert!(first.is_store());
        // What Enter does: `pick_selected` → `set_system` → `ConnectionForm::for_connector`.
        let form = ConnectionForm::for_connector(&first);
        assert_eq!(form.system.as_deref(), Some("redis"));
        assert!(connector_of(form.dialect, form.system.as_deref()).is_some_and(|c| c.is_store()));
    }
}
