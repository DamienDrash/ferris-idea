//! Terminal screen model fed by `vte` (`SPEC-0038` behaviours 2–6, 8 and 9).

use std::collections::VecDeque;

use serde::Serialize;

/// A colour of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase", tag = "kind", content = "value")]
pub enum Color {
    /// The terminal's default colour.
    #[default]
    Default,
    /// One of the 256 palette colours (0–15 are the basic and bright colours).
    Indexed(u8),
    /// True colour.
    Rgb(u8, u8, u8),
}

/// Style of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Style {
    /// Foreground colour.
    pub fg: Color,
    /// Background colour.
    pub bg: Color,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
    /// Foreground and background swapped.
    pub inverse: bool,
    /// Hyperlink set with OSC 8 (`Screen::link_uri`); 0 for none (`SPEC-0153` behaviour 2).
    #[serde(skip_serializing_if = "is_zero")]
    pub link: u16,
}

fn is_zero(n: &u16) -> bool {
    *n == 0
}

/// One character cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cell {
    /// The character (a space when blank).
    pub ch: char,
    /// Combining marks drawn on top of `ch`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub combining: String,
    /// Columns taken: 1, 2 for a wide character, 0 for the cell right of a wide character.
    pub width: u8,
    /// Its style.
    pub style: Style,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            combining: String::new(),
            width: 1,
            style: Style::default(),
        }
    }
}

type Row = Vec<Cell>;

/// Which mouse events the program asked for (`SPEC-0118`, xterm private modes 9, 1000, 1002, 1003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseMode {
    /// No reports; the mouse selects text.
    #[default]
    Off,
    /// Presses only, without modifiers (mode 9).
    X10,
    /// Presses, releases and the wheel (mode 1000).
    Press,
    /// Also motion while a button is held (mode 1002).
    Drag,
    /// Also motion without a button (mode 1003).
    Motion,
}

/// How mouse reports are encoded (modes 1005, 1006; default otherwise).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseEncoding {
    /// `CSI M` with bytes offset by 32; coordinates up to 223.
    #[default]
    Default,
    /// `CSI M` with UTF-8 encoded values; coordinates up to 2015.
    Utf8,
    /// `CSI < b ; x ; y M/m`; any coordinate, releases keep their button.
    Sgr,
}

/// Saved primary screen while the alternate screen is shown.
#[derive(Debug, Clone)]
struct Saved {
    grid: Vec<Row>,
}

/// What save cursor keeps (DECSC, `CSI s`, modes 1048 and 1049): the position with the pending
/// wrap, the rendition, the origin mode and the character sets.
#[derive(Debug, Clone, Copy, Default)]
struct SavedCursor {
    cursor: (usize, usize),
    wrap_pending: bool,
    style: Style,
    origin: bool,
    graphics: [bool; 2],
    shift_out: bool,
}

/// Longest window title kept, in characters.
const MAX_TITLE: usize = 1024;
/// Longest hyperlink address (OSC 8) kept, in bytes; the limit other terminals use.
const MAX_LINK: usize = 2083;
/// Bytes of hyperlink addresses kept in all.
const MAX_LINK_BYTES: usize = 1 << 20;
/// Longest reported folder (OSC 7) kept, in bytes.
const MAX_DIR: usize = 4096;
/// Combining marks kept on one cell.
const MAX_COMBINING: usize = 16;
/// Bytes of answers kept until the session collects them.
const MAX_RESPONSES: usize = 4096;
/// Longest operating system command read, in bytes; a longer one is dropped whole.
const MAX_OSC: usize = 64 * 1024;

/// What the terminal shows.
#[derive(Debug, Clone)]
pub struct Screen {
    rows: usize,
    cols: usize,
    grid: Vec<Row>,
    scrollback: VecDeque<Row>,
    max_scrollback: usize,
    /// Cursor as (row, column), 0-based.
    cursor: (usize, usize),
    wrap_pending: bool,
    style: Style,
    scroll_top: usize,
    scroll_bottom: usize,
    /// Saved cursor of the primary screen and of the alternate screen.
    saved_cursor: [SavedCursor; 2],
    alternate: Option<Saved>,
    cursor_visible: bool,
    app_cursor: bool,
    app_keypad: bool,
    bracketed_paste: bool,
    /// Origin mode (DECOM): positions count from the scroll region.
    origin: bool,
    /// Autowrap (DECAWM).
    autowrap: bool,
    /// Insert mode (IRM): printed characters push the rest of the line right.
    insert: bool,
    /// Tab stop per column.
    tabs: Vec<bool>,
    /// Whether G0 and G1 hold the DEC special graphics (line drawing) set.
    graphics: [bool; 2],
    /// Whether G1 is in use (after SO).
    shift_out: bool,
    /// The last printed character, for REP.
    last_printed: Option<char>,
    /// Bytes of the addresses in `links`.
    link_bytes: usize,
    /// Set by [`Terminal::feed`] while it ends an over-long operating system command.
    discard_osc: bool,
    mouse_mode: MouseMode,
    mouse_encoding: MouseEncoding,
    title: String,
    bells: usize,
    responses: Vec<u8>,
    /// Addresses of OSC 8 hyperlinks; `Style::link` n is `links[n - 1]`.
    links: Vec<String>,
    /// Current folder the shell reported with OSC 7 (`SPEC-0161`).
    cwd: Option<String>,
    /// How many OSC 7 reports came, so that a repeated report is seen (`SPEC-0161` v2).
    cwd_reports: u64,
    /// History lines that left the front of the scrollback; history line `i` has the stable
    /// number `dropped + i` (`SPEC-0206` behaviour 2).
    dropped: u64,
    /// OSC 133 prompt marks, oldest first.
    prompts: Vec<Prompt>,
    /// Stable line and column where the command of the last prompt started (OSC 133 `B`).
    command_start: Option<(u64, usize)>,
    /// Stable lines after the command start that the text filled to the last column and then
    /// continued on the next line: their trailing spaces belong to the command.
    command_wraps: Vec<u64>,
}

/// A prompt the shell marked with OSC 133 (`SPEC-0206` behaviour 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// Stable history line of the prompt start (`A`); see [`Screen::history_base`].
    pub line: u64,
    /// The command line typed after the prompt, once it ran (`C`).
    pub command: Option<String>,
    /// Exit status of the command (`D;n`), once it ended.
    pub exit: Option<i32>,
    /// Stable history line where its output started (`C`), once it ran (`SPEC-0313`).
    pub output_line: Option<u64>,
}

/// The last command the shell marked, with its output (`SPEC-0313` behaviour 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastCommand {
    /// The command line.
    pub command: String,
    /// Its output lines, without trailing empty lines.
    pub output: Vec<String>,
    /// Its exit status; `None` while it runs.
    pub exit: Option<i32>,
}

/// Default scrollback length.
pub const DEFAULT_SCROLLBACK: usize = 10_000;

impl Screen {
    /// A blank screen of `rows` × `cols` (each at least 1).
    pub fn new(rows: usize, cols: usize) -> Self {
        let (rows, cols) = (rows.max(1), cols.max(1));
        Self {
            rows,
            cols,
            grid: vec![vec![Cell::default(); cols]; rows],
            scrollback: VecDeque::new(),
            max_scrollback: DEFAULT_SCROLLBACK,
            cursor: (0, 0),
            wrap_pending: false,
            style: Style::default(),
            scroll_top: 0,
            scroll_bottom: rows - 1,
            saved_cursor: [SavedCursor::default(); 2],
            alternate: None,
            cursor_visible: true,
            app_cursor: false,
            app_keypad: false,
            bracketed_paste: false,
            origin: false,
            autowrap: true,
            insert: false,
            tabs: default_tabs(0, cols).collect(),
            graphics: [false; 2],
            shift_out: false,
            last_printed: None,
            link_bytes: 0,
            discard_osc: false,
            mouse_mode: MouseMode::Off,
            mouse_encoding: MouseEncoding::Default,
            title: String::new(),
            bells: 0,
            responses: Vec::new(),
            links: Vec::new(),
            cwd: None,
            cwd_reports: 0,
            dropped: 0,
            prompts: Vec::new(),
            command_start: None,
            command_wraps: Vec::new(),
        }
    }

    /// Size as (rows, columns).
    pub fn size(&self) -> (usize, usize) {
        (self.rows, self.cols)
    }

    /// Cursor as (row, column), 0-based.
    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
    }

    /// Whether the cursor is shown.
    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    /// Whether arrow keys send application sequences (`ESC O A`, DECCKM).
    pub fn app_cursor(&self) -> bool {
        self.app_cursor
    }

    /// Whether the program asked for the application keypad (DECKPAM, `ESC =`).
    pub fn app_keypad(&self) -> bool {
        self.app_keypad
    }

    /// Whether pasted text is wrapped in `ESC [200~` … `ESC [201~`.
    pub fn bracketed_paste(&self) -> bool {
        self.bracketed_paste
    }

    /// Window title set by the program.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Number of bells rung.
    pub fn bells(&self) -> usize {
        self.bells
    }

    /// Whether the alternate screen is shown.
    pub fn alternate(&self) -> bool {
        self.alternate.is_some()
    }

    /// Cell at (row, column).
    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        self.grid.get(row)?.get(col)
    }

    /// Visible rows as text, trailing blanks removed, joined with `\n`.
    pub fn text(&self) -> String {
        let mut lines: Vec<String> = self.grid.iter().map(row_text).collect();
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines.join("\n")
    }

    /// The folder the shell last reported with OSC 7 (`file://host/path`), decoded (`SPEC-0161`).
    pub fn reported_dir(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    /// How many OSC 7 reports came so far: changes with every report, the same folder again
    /// included (`SPEC-0161` v2 behaviour 4).
    pub fn dir_reports(&self) -> u64 {
        self.cwd_reports
    }

    /// Address of the OSC 8 hyperlink `id` (`Style::link`).
    pub fn link_uri(&self, id: u16) -> Option<&str> {
        self.links
            .get(usize::from(id).checked_sub(1)?)
            .map(String::as_str)
    }

    /// Mouse events the program asked for.
    pub fn mouse_mode(&self) -> MouseMode {
        self.mouse_mode
    }

    /// Encoding of mouse reports.
    pub fn mouse_encoding(&self) -> MouseEncoding {
        self.mouse_encoding
    }

    /// Number of scrollback lines.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Line `index` of the whole history: scrollback lines first (oldest at 0), then the screen
    /// rows (`SPEC-0118` behaviour 3).
    pub fn line(&self, index: usize) -> Option<&[Cell]> {
        match index.checked_sub(self.scrollback.len()) {
            None => self.scrollback.get(index).map(Vec::as_slice),
            Some(row) => self.grid.get(row).map(Vec::as_slice),
        }
    }

    /// Stable number of history line 0: lines that left the scrollback keep their numbers
    /// (`SPEC-0206` behaviour 2).
    pub fn history_base(&self) -> u64 {
        self.dropped
    }

    /// Number of history lines: scrollback and screen rows.
    pub fn history_len(&self) -> usize {
        self.scrollback.len() + self.grid.len()
    }

    /// The prompts marked with OSC 133 whose lines are still in the history, oldest first.
    pub fn prompts(&self) -> &[Prompt] {
        &self.prompts
    }

    /// The last command the shell marked with OSC 133: its command line, its output (from the
    /// output-start mark up to the next prompt or the end) and its exit status (`SPEC-0313`
    /// behaviour 2); `None` without marks or before the first command.
    pub fn last_command(&self) -> Option<LastCommand> {
        let at = self.prompts.iter().rposition(|p| p.command.is_some())?;
        let p = &self.prompts[at];
        let start = p.output_line?;
        let end = self
            .prompts
            .get(at + 1)
            .map(|next| next.line)
            .unwrap_or(self.dropped + self.history_len() as u64);
        let mut output: Vec<String> = (start.max(self.dropped)..end)
            .filter_map(|l| self.line_text_from(l, 0))
            .collect();
        while output.last().is_some_and(String::is_empty) {
            output.pop();
        }
        Some(LastCommand {
            command: p.command.clone().unwrap_or_default(),
            output,
            exit: p.exit,
        })
    }

    /// The last `max` history lines as text (scrollback and screen), without trailing empty
    /// lines (`SPEC-0205` behaviour 6).
    pub fn history_lines(&self, max: usize) -> Vec<String> {
        let mut lines: Vec<String> = self
            .scrollback
            .iter()
            .chain(self.grid.iter())
            .map(row_text)
            .collect();
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        let skip = lines.len().saturating_sub(max);
        lines.split_off(skip)
    }

    /// Stable number of the cursor's line.
    fn cursor_line(&self) -> u64 {
        self.dropped + (self.scrollback.len() + self.cursor.0) as u64
    }

    /// Text of stable line `line` from column `from`, if it is still in the history.
    fn line_text_from(&self, line: u64, from: usize) -> Option<String> {
        let index = usize::try_from(line.checked_sub(self.dropped)?).ok()?;
        let cells = self.line(index)?;
        let row: Row = cells.iter().skip(from).cloned().collect();
        Some(row_text(&row))
    }

    /// Forgets lines that left the history: they keep their numbers, marks on them go.
    fn drop_front(&mut self, n: usize) {
        self.dropped += n as u64;
        let base = self.dropped;
        self.prompts.retain(|p| p.line >= base);
    }

    /// OSC 133 (`SPEC-0206` behaviour 2): `A` prompt start, `B` command start, `C` output
    /// start, `D;n` command end. `C` may carry the command line the shell ran as
    /// `cmdline_url=<percent-encoded>` (the form kitty and fish 4 send); it is taken instead of
    /// the screen text, which the line editor may have scrolled or cut (zsh in a terminal of
    /// fewer than three rows shows one line, scrolled sideways).
    fn prompt_mark(&mut self, kind: &[u8], rest: &[&[u8]]) {
        let status = rest.first().copied();
        if self.alternate.is_some() {
            return;
        }
        match kind.first() {
            Some(b'A') => {
                let line = self.cursor_line();
                // A prompt drawn again on its line is the same prompt, and there are never
                // more prompts than lines of history.
                if self
                    .prompts
                    .last()
                    .is_some_and(|p| p.line == line && p.command.is_none())
                {
                    self.prompts.pop();
                }
                if self.prompts.len() >= self.max_scrollback + self.rows {
                    self.prompts.remove(0);
                }
                self.prompts.push(Prompt {
                    line,
                    command: None,
                    exit: None,
                    output_line: None,
                });
                self.command_start = None;
                self.command_wraps.clear();
            }
            Some(b'B') => {
                self.command_start = Some((self.cursor_line(), self.cursor.1));
                self.command_wraps.clear();
            }
            Some(b'C') => {
                let reported = rest
                    .iter()
                    .find_map(|p| p.strip_prefix(b"cmdline_url="))
                    .map(|url| percent_decode(&String::from_utf8_lossy(url)));
                let start = self.command_start.take();
                let wraps = std::mem::take(&mut self.command_wraps);
                let command = match (reported, start) {
                    (Some(command), _) => command.trim().to_owned(),
                    (None, Some((line, col))) => self.command_text(line, col, &wraps),
                    (None, None) => return,
                };
                let output = self.cursor_line();
                if let Some(p) = self.prompts.last_mut() {
                    p.command = Some(command);
                    p.output_line = Some(output);
                }
            }
            Some(b'D') => {
                if let Some(p) = self.prompts.last_mut()
                    && p.command.is_some()
                    && p.exit.is_none()
                {
                    p.exit = Some(
                        status
                            .and_then(|s| std::str::from_utf8(s).ok())
                            .and_then(|s| s.trim().parse().ok())
                            .unwrap_or(0),
                    );
                }
            }
            _ => {}
        }
    }

    /// The command line on the screen from stable line `line`, column `col` to the cursor; lines
    /// in `wraps` continue on the next line with their trailing spaces.
    fn command_text(&self, line: u64, col: usize, wraps: &[u64]) -> String {
        let end = self.cursor_line();
        let last = if end > line && self.cursor.1 == 0 {
            end - 1
        } else {
            end
        };
        let mut text = String::new();
        for l in line..=last.max(line) {
            let from = if l == line { col } else { 0 };
            let part = if wraps.contains(&l) && l < last {
                self.line_cells_from(l, from)
            } else {
                self.line_text_from(l, from)
            };
            text.extend(part);
        }
        text.trim().to_owned()
    }

    /// Like [`Screen::line_text_from`], with trailing spaces kept.
    fn line_cells_from(&self, line: u64, from: usize) -> Option<String> {
        let index = usize::try_from(line.checked_sub(self.dropped)?).ok()?;
        let cells = self.line(index)?;
        let mut text = String::new();
        for cell in cells.iter().skip(from).filter(|c| c.width > 0) {
            text.push(cell.ch);
            text.push_str(&cell.combining);
        }
        Some(text)
    }

    /// Scrollback lines, oldest first.
    pub fn scrollback(&self) -> Vec<String> {
        self.scrollback.iter().map(row_text).collect()
    }

    /// Bytes the terminal must send back to the program (answers to queries).
    pub fn take_responses(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.responses)
    }

    /// Changes the size without reflow (`SPEC-0038` behaviour 8): lines are cut or padded, rows
    /// of the primary screen that no longer fit go to the scrollback, also while the alternate
    /// screen is shown.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        let (rows, cols) = (rows.max(1), cols.max(1));
        let shown = usize::from(self.alternate.is_some());
        let mut cursor = self.cursor;
        let off = fit(&mut self.grid, &mut cursor, rows, cols);
        self.cursor = cursor;
        if let Some(mut saved) = self.alternate.take() {
            // The rows pushed off the alternate screen are gone; those of the primary screen
            // behind it are history. Its cursor is the one saved for it.
            let mut cursor = self.saved_cursor[0].cursor;
            for row in fit(&mut saved.grid, &mut cursor, rows, cols) {
                self.push_scrollback(row);
            }
            self.saved_cursor[0].cursor = cursor;
            self.alternate = Some(saved);
        } else {
            for row in off {
                self.push_scrollback(row);
            }
        }
        let known = self.tabs.len();
        self.tabs.truncate(cols);
        self.tabs.extend(default_tabs(known, cols));
        (self.rows, self.cols) = (rows, cols);
        self.scroll_top = 0;
        self.scroll_bottom = rows - 1;
        self.wrap_pending = false;
        self.saved_cursor[shown].wrap_pending = false;
    }

    fn blank(&self) -> Cell {
        Cell {
            style: Style {
                bg: self.style.bg,
                ..Style::default()
            },
            ..Cell::default()
        }
    }

    fn push_scrollback(&mut self, row: Row) {
        if self.max_scrollback == 0 {
            return;
        }
        if self.scrollback.len() == self.max_scrollback {
            self.scrollback.pop_front();
            self.drop_front(1);
        }
        self.scrollback.push_back(row);
    }

    fn scroll_up(&mut self, n: usize) {
        for _ in 0..n.min(self.scroll_bottom - self.scroll_top + 1) {
            let row = self.grid.remove(self.scroll_top);
            if self.scroll_top == 0 && self.alternate.is_none() {
                self.push_scrollback(row);
            }
            let blank = vec![self.blank(); self.cols];
            self.grid.insert(self.scroll_bottom, blank);
        }
    }

    fn scroll_down(&mut self, n: usize) {
        for _ in 0..n.min(self.scroll_bottom - self.scroll_top + 1) {
            self.grid.remove(self.scroll_bottom);
            let blank = vec![self.blank(); self.cols];
            self.grid.insert(self.scroll_top, blank);
        }
    }

    fn linefeed(&mut self) {
        self.wrap_pending = false;
        if self.cursor.0 == self.scroll_bottom {
            self.scroll_up(1);
        } else if self.cursor.0 + 1 < self.rows {
            self.cursor.0 += 1;
        }
    }

    fn reverse_index(&mut self) {
        self.wrap_pending = false;
        if self.cursor.0 == self.scroll_top {
            self.scroll_down(1);
        } else {
            self.cursor.0 = self.cursor.0.saturating_sub(1);
        }
    }

    fn goto(&mut self, row: usize, col: usize) {
        self.cursor = (row.min(self.rows - 1), col.min(self.cols - 1));
        self.wrap_pending = false;
    }

    /// Moves to a row a program addressed (CUP, HVP, VPA): in origin mode it counts from the
    /// top of the scroll region and stays inside it.
    fn goto_addressed(&mut self, row: usize, col: usize) {
        if self.origin {
            let row = (self.scroll_top + row).min(self.scroll_bottom);
            self.goto(row, col);
        } else {
            self.goto(row, col);
        }
    }

    fn save_cursor(&mut self) {
        self.saved_cursor[usize::from(self.alternate.is_some())] = SavedCursor {
            cursor: self.cursor,
            wrap_pending: self.wrap_pending,
            style: self.style,
            origin: self.origin,
            graphics: self.graphics,
            shift_out: self.shift_out,
        };
    }

    /// Restores what [`Screen::save_cursor`] kept for the shown screen; a hyperlink that is
    /// open stays as it is (only OSC 8 changes it).
    fn restore_cursor(&mut self) {
        let saved = self.saved_cursor[usize::from(self.alternate.is_some())];
        self.goto(saved.cursor.0, saved.cursor.1);
        self.wrap_pending = saved.wrap_pending && self.cursor == saved.cursor;
        self.style = Style {
            link: self.style.link,
            ..saved.style
        };
        self.origin = saved.origin;
        self.graphics = saved.graphics;
        self.shift_out = saved.shift_out;
    }

    /// The column of the `n`th tab stop right of `col`, or the last column.
    fn tab_forward(&self, col: usize, n: usize) -> usize {
        let mut col = col;
        for _ in 0..n {
            match (col + 1..self.cols).find(|&c| self.tabs[c]) {
                Some(stop) => col = stop,
                None => return self.cols - 1,
            }
        }
        col
    }

    /// The column of the `n`th tab stop left of `col`, or the first column.
    fn tab_backward(&self, col: usize, n: usize) -> usize {
        let mut col = col;
        for _ in 0..n {
            match (0..col).rev().find(|&c| self.tabs[c]) {
                Some(stop) => col = stop,
                None => return 0,
            }
        }
        col
    }

    fn erase(&mut self, row: usize, from: usize, to: usize) {
        let blank = self.blank();
        for cell in &mut self.grid[row][from.min(self.cols)..to.min(self.cols)] {
            *cell = blank.clone();
        }
    }

    fn print(&mut self, ch: char) {
        let ch = if self.graphics[usize::from(self.shift_out)] {
            line_drawing(ch)
        } else {
            ch
        };
        let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        if width == 0 {
            self.combine(ch);
            return;
        }
        self.last_printed = Some(ch);
        // A wide character needs two columns; on a one-column screen it takes one.
        let width = width.min(2).min(self.cols);
        if !self.autowrap {
            // Nothing wraps: the character is written at the right edge.
            self.wrap_pending = false;
            self.cursor.1 = self.cursor.1.min(self.cols - width);
        }
        if self.wrap_pending || self.cursor.1 + width > self.cols {
            if self.wrap_pending && self.command_start.is_some() {
                let line = self.cursor_line();
                // A command line is a few lines; output that follows a command start without
                // an output mark must not be recorded line by line.
                if self.command_wraps.last() != Some(&line) && self.command_wraps.len() < 256 {
                    self.command_wraps.push(line);
                }
            }
            if !self.wrap_pending {
                // The last column stays blank, as in xterm.
                let (row, col) = self.cursor;
                self.put(row, col, self.blank());
            }
            self.cursor.1 = 0;
            self.linefeed();
        }
        let (row, col) = self.cursor;
        if self.insert {
            self.insert_blanks(row, col, width);
        }
        self.put(
            row,
            col,
            Cell {
                ch,
                combining: String::new(),
                width: width as u8,
                style: self.style,
            },
        );
        if width == 2 {
            self.put(
                row,
                col + 1,
                Cell {
                    width: 0,
                    style: self.style,
                    ..Cell::default()
                },
            );
        }
        if col + width == self.cols {
            self.cursor.1 = self.cols - 1;
            self.wrap_pending = self.autowrap;
        } else {
            self.cursor.1 += width;
        }
    }

    /// Opens `n` blank cells at (row, col); the cells pushed past the right edge go (ICH, and
    /// printing in insert mode).
    fn insert_blanks(&mut self, row: usize, col: usize, n: usize) {
        let n = n.min(self.cols - col);
        let blank = self.blank();
        let line = &mut self.grid[row];
        // Half of a wide character must not be left on either side of the gap.
        if line[col].width == 0 && col > 0 {
            line[col - 1] = blank.clone();
            line[col] = blank.clone();
        }
        line.splice(col..col, std::iter::repeat_n(blank.clone(), n));
        line.truncate(self.cols);
        if line.last().is_some_and(|c| c.width == 2) {
            line[self.cols - 1] = blank;
        }
    }

    /// Writes a cell, clearing the other half of a wide character it overwrites.
    fn put(&mut self, row: usize, col: usize, cell: Cell) {
        let line = &mut self.grid[row];
        match line[col].width {
            2 if col + 1 < line.len() => line[col + 1] = Cell::default(),
            0 if col > 0 => line[col - 1] = Cell::default(),
            _ => {}
        }
        line[col] = cell;
    }

    /// Attaches a combining mark to the character before the cursor.
    fn combine(&mut self, mark: char) {
        let (row, col) = self.cursor;
        let mut col = if self.wrap_pending {
            col
        } else {
            col.saturating_sub(1)
        };
        if self.grid[row][col].width == 0 && col > 0 {
            col -= 1;
        }
        let cell = &mut self.grid[row][col];
        // Output cannot make a cell grow without end.
        if cell.combining.chars().count() < MAX_COMBINING {
            cell.combining.push(mark);
        }
    }

    fn execute(&mut self, byte: u8) {
        self.last_printed = None;
        match byte {
            b'\r' => {
                self.cursor.1 = 0;
                self.wrap_pending = false;
            }
            b'\n' | 0x0b | 0x0c => self.linefeed(),
            0x08 => {
                self.cursor.1 = self.cursor.1.saturating_sub(1);
                self.wrap_pending = false;
            }
            b'\t' => self.cursor.1 = self.tab_forward(self.cursor.1, 1),
            0x0e => self.shift_out = true,
            0x0f => self.shift_out = false,
            0x07 => self.bells += 1,
            _ => {}
        }
    }

    fn set_alternate(&mut self, on: bool) {
        match (on, self.alternate.is_some()) {
            (true, false) => {
                let blank = vec![vec![Cell::default(); self.cols]; self.rows];
                let grid = std::mem::replace(&mut self.grid, blank);
                self.alternate = Some(Saved { grid });
            }
            (false, true) => {
                if let Some(saved) = self.alternate.take() {
                    self.grid = saved.grid;
                }
            }
            _ => {}
        }
        self.wrap_pending = false;
    }

    fn sgr(&mut self, params: &[Vec<u16>]) {
        // A hyperlink outlasts colour changes; only OSC 8 ends it.
        let link = self.style.link;
        if params.is_empty() {
            self.style = Style {
                link,
                ..Style::default()
            };
            return;
        }
        let mut i = 0;
        while i < params.len() {
            let param = &params[i];
            i += 1;
            let p = param.first().copied().unwrap_or(0);
            if param.len() > 1 {
                // Sub-parameters (`:`) belong to their parameter alone.
                match p {
                    4 => self.style.underline = param[1] != 0,
                    38 | 48 => {
                        // `38:5:n`, `38:2:r:g:b` and `38:2:<colour space>:r:g:b` (ITU T.416).
                        let color = match &param[1..] {
                            [5, n] => indexed(*n),
                            [2, r, g, b] | [2, _, r, g, b] => rgb(*r, *g, *b),
                            _ => None,
                        };
                        self.set_color(p, color);
                    }
                    _ => {}
                }
                continue;
            }
            match p {
                0 => {
                    self.style = Style {
                        link,
                        ..Style::default()
                    }
                }
                1 => self.style.bold = true,
                3 => self.style.italic = true,
                4 | 21 => self.style.underline = true,
                7 => self.style.inverse = true,
                22 => self.style.bold = false,
                23 => self.style.italic = false,
                24 => self.style.underline = false,
                27 => self.style.inverse = false,
                30..=37 => self.style.fg = Color::Indexed((p - 30) as u8),
                39 => self.style.fg = Color::Default,
                40..=47 => self.style.bg = Color::Indexed((p - 40) as u8),
                49 => self.style.bg = Color::Default,
                90..=97 => self.style.fg = Color::Indexed((p - 90 + 8) as u8),
                100..=107 => self.style.bg = Color::Indexed((p - 100 + 8) as u8),
                38 | 48 | 58 => {
                    // `38;5;n` and `38;2;r;g;b`: the colour takes the parameters after it, also
                    // for the underline colour (58), which the model does not keep.
                    let value = |k: usize| params.get(i + k).and_then(|v| v.first()).copied();
                    let color = match value(0) {
                        Some(5) => {
                            let c = value(1).and_then(indexed);
                            i += 2;
                            c
                        }
                        Some(2) => {
                            let c = match (value(1), value(2), value(3)) {
                                (Some(r), Some(g), Some(b)) => rgb(r, g, b),
                                _ => None,
                            };
                            i += 4;
                            c
                        }
                        _ => None,
                    };
                    self.set_color(p, color);
                }
                _ => {}
            }
        }
    }

    /// Sets the foreground (38) or background (48) colour; other targets and no colour change
    /// nothing.
    fn set_color(&mut self, target: u16, color: Option<Color>) {
        match (target, color) {
            (38, Some(color)) => self.style.fg = color,
            (48, Some(color)) => self.style.bg = color,
            _ => {}
        }
    }

    fn csi(&mut self, params: &[Vec<u16>], private: bool, action: char) {
        // First value of parameter `i`, with `default` for missing or zero.
        let arg = |i: usize, default: usize| -> usize {
            match params.get(i).and_then(|p| p.first()) {
                Some(&0) | None => default,
                Some(&v) => v as usize,
            }
        };
        let (row, col) = self.cursor;
        if action != 'b' {
            self.last_printed = None;
        }
        match (private, action) {
            (false, 'A') => self.goto(row.saturating_sub(arg(0, 1)).max(self.top_for(row)), col),
            (false, 'B') => self.goto((row + arg(0, 1)).min(self.bottom_for(row)), col),
            (false, 'C') => self.goto(row, col + arg(0, 1)),
            (false, 'D') => self.goto(row, col.saturating_sub(arg(0, 1))),
            (false, 'E') => self.goto((row + arg(0, 1)).min(self.bottom_for(row)), 0),
            (false, 'F') => self.goto(row.saturating_sub(arg(0, 1)).max(self.top_for(row)), 0),
            (false, 'G' | '`') => self.goto(row, arg(0, 1) - 1),
            (false, 'a') => self.goto(row, col + arg(0, 1)),
            (false, 'e') => self.goto((row + arg(0, 1)).min(self.bottom_for(row)), col),
            (false, 'H' | 'f') => self.goto_addressed(arg(0, 1) - 1, arg(1, 1) - 1),
            (false, 'd') => self.goto_addressed(arg(0, 1) - 1, col),
            (false, 'I') => self.goto(row, self.tab_forward(col, arg(0, 1))),
            (false, 'Z') => self.goto(row, self.tab_backward(col, arg(0, 1))),
            (false, 'g') => match params.first().and_then(|p| p.first()).copied().unwrap_or(0) {
                0 => self.tabs[col] = false,
                3 => self.tabs.fill(false),
                _ => {}
            },
            (false, 'b') => {
                // REP: the character before, again; a parameter holds at most 65 535.
                if let Some(ch) = self.last_printed {
                    for _ in 0..arg(0, 1) {
                        self.print(ch);
                    }
                }
            }
            (false, 'J') => {
                let mode = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
                match mode {
                    0 => {
                        self.erase(row, col, self.cols);
                        for r in row + 1..self.rows {
                            self.erase(r, 0, self.cols);
                        }
                    }
                    1 => {
                        for r in 0..row {
                            self.erase(r, 0, self.cols);
                        }
                        self.erase(row, 0, col + 1);
                    }
                    2 => {
                        for r in 0..self.rows {
                            self.erase(r, 0, self.cols);
                        }
                    }
                    3 => {
                        let n = self.scrollback.len();
                        self.scrollback.clear();
                        self.drop_front(n);
                    }
                    _ => {}
                }
            }
            (false, 'K') => {
                let mode = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
                match mode {
                    0 => self.erase(row, col, self.cols),
                    1 => self.erase(row, 0, col + 1),
                    2 => self.erase(row, 0, self.cols),
                    _ => {}
                }
            }
            (false, 'L' | 'M') if (self.scroll_top..=self.scroll_bottom).contains(&row) => {
                let n = arg(0, 1).min(self.scroll_bottom - row + 1);
                let blank = vec![self.blank(); self.cols];
                for _ in 0..n {
                    if action == 'L' {
                        self.grid.remove(self.scroll_bottom);
                        self.grid.insert(row, blank.clone());
                    } else {
                        self.grid.remove(row);
                        self.grid.insert(self.scroll_bottom, blank.clone());
                    }
                }
                self.goto(row, 0);
            }
            (false, 'P') => {
                let n = arg(0, 1).min(self.cols - col);
                let blank = self.blank();
                let line = &mut self.grid[row];
                line.drain(col..col + n);
                line.extend(std::iter::repeat_n(blank, n));
            }
            (false, '@') => self.insert_blanks(row, col, arg(0, 1)),
            (false, 'X') => self.erase(row, col, col + arg(0, 1)),
            (false, 'S') => self.scroll_up(arg(0, 1)),
            (false, 'T') => self.scroll_down(arg(0, 1)),
            (false, 'r') => {
                let top = arg(0, 1) - 1;
                let bottom = arg(1, self.rows).min(self.rows) - 1;
                if top < bottom {
                    (self.scroll_top, self.scroll_bottom) = (top, bottom);
                    self.goto_addressed(0, 0);
                }
            }
            (false, 's') => self.save_cursor(),
            (false, 'u') => self.restore_cursor(),
            (false, 'm') => self.sgr(params),
            // Device status: "ready", and the cursor position (relative in origin mode).
            (false, 'n') => match arg(0, 0) {
                5 => self.respond("\x1b[0n"),
                6 => {
                    let top = if self.origin { self.scroll_top } else { 0 };
                    let line = row.saturating_sub(top) + 1;
                    self.respond(&format!("\x1b[{line};{}R", col + 1));
                }
                _ => {}
            },
            // Primary device attributes: a VT100 with advanced video, the answer the terminfo
            // entry of `xterm-256color` names (u8). No other query is answered (`SPEC-0038`
            // behaviour 9).
            (false, 'c') if arg(0, 0) == 0 => self.respond("\x1b[?1;2c"),
            (false, 'h' | 'l') => {
                for p in params.iter().filter_map(|p| p.first()) {
                    if *p == 4 {
                        self.insert = action == 'h';
                    }
                }
            }
            (true, 'h' | 'l') => {
                let on = action == 'h';
                for p in params.iter().filter_map(|p| p.first()) {
                    match p {
                        1 => self.app_cursor = on,
                        6 => {
                            self.origin = on;
                            self.goto_addressed(0, 0);
                        }
                        7 => {
                            self.autowrap = on;
                            self.wrap_pending = false;
                        }
                        25 => self.cursor_visible = on,
                        2004 => self.bracketed_paste = on,
                        9 | 1000 | 1002 | 1003 => {
                            self.mouse_mode = match (on, p) {
                                (false, _) => MouseMode::Off,
                                (true, 9) => MouseMode::X10,
                                (true, 1000) => MouseMode::Press,
                                (true, 1002) => MouseMode::Drag,
                                _ => MouseMode::Motion,
                            }
                        }
                        1005 | 1006 => {
                            self.mouse_encoding = match (on, p) {
                                (false, _) => MouseEncoding::Default,
                                (true, 1005) => MouseEncoding::Utf8,
                                _ => MouseEncoding::Sgr,
                            }
                        }
                        // 1049 is 1048 (save and restore the cursor) around 1047 (the
                        // switch): the cursor of the primary screen is kept in its own slot.
                        1049 => {
                            if on {
                                if self.alternate.is_none() {
                                    self.save_cursor();
                                }
                                self.set_alternate(true);
                            } else {
                                self.set_alternate(false);
                                self.restore_cursor();
                            }
                        }
                        1048 => {
                            if on {
                                self.save_cursor();
                            } else {
                                self.restore_cursor();
                            }
                        }
                        47 | 1047 => self.set_alternate(on),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    /// Cursor movement stops at the scroll region when it starts inside it.
    fn top_for(&self, row: usize) -> usize {
        if row >= self.scroll_top {
            self.scroll_top
        } else {
            0
        }
    }

    fn bottom_for(&self, row: usize) -> usize {
        if row <= self.scroll_bottom {
            self.scroll_bottom
        } else {
            self.rows - 1
        }
    }

    /// Queues an answer to the program; answers nobody collects do not pile up.
    fn respond(&mut self, answer: &str) {
        if self.responses.len() + answer.len() <= MAX_RESPONSES {
            self.responses.extend_from_slice(answer.as_bytes());
        }
    }

    /// Soft reset (DECSTR): modes, rendition, region and saved cursor back to their defaults;
    /// the text, the mouse and paste modes and an open hyperlink stay.
    fn soft_reset(&mut self) {
        self.cursor_visible = true;
        self.app_cursor = false;
        self.app_keypad = false;
        self.origin = false;
        self.autowrap = true;
        self.insert = false;
        self.wrap_pending = false;
        self.graphics = [false; 2];
        self.shift_out = false;
        self.style = Style {
            link: self.style.link,
            ..Style::default()
        };
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;
        self.saved_cursor[usize::from(self.alternate.is_some())] = SavedCursor::default();
    }

    fn reset(&mut self) {
        let max = self.max_scrollback;
        let scrollback = std::mem::take(&mut self.scrollback);
        // Lines in the history keep their hyperlinks.
        let links = std::mem::take(&mut self.links);
        let (dropped, prompts) = (self.dropped, std::mem::take(&mut self.prompts));
        let cwd_reports = self.cwd_reports;
        *self = Self::new(self.rows, self.cols);
        // Counted on, so that a report after the reset is still seen as new.
        self.cwd_reports = cwd_reports;
        self.scrollback = scrollback;
        self.link_bytes = links.iter().map(String::len).sum();
        self.links = links;
        self.max_scrollback = max;
        self.dropped = dropped;
        self.prompts = prompts;
    }
}

/// Default tab stops (every 8 columns) for the columns `from..to`.
fn default_tabs(from: usize, to: usize) -> impl Iterator<Item = bool> {
    (from..to).map(|c| c % 8 == 0)
}

/// Brings `grid` to `rows` × `cols` without reflow and returns the rows pushed off the top:
/// blank space below the cursor goes first.
fn fit(grid: &mut Vec<Row>, cursor: &mut (usize, usize), rows: usize, cols: usize) -> Vec<Row> {
    for row in grid.iter_mut() {
        row.resize(cols, Cell::default());
        // A wide character cut in half by the new edge goes.
        if let Some(last) = row.last_mut()
            && last.width == 2
        {
            *last = Cell::default();
        }
    }
    let mut off = Vec::new();
    while grid.len() > rows {
        if grid.len() - 1 > cursor.0 {
            grid.pop();
        } else {
            off.push(grid.remove(0));
            cursor.0 = cursor.0.saturating_sub(1);
        }
    }
    while grid.len() < rows {
        grid.push(vec![Cell::default(); cols]);
    }
    *cursor = (cursor.0.min(rows - 1), cursor.1.min(cols - 1));
    off
}

/// A palette colour, if `n` is one.
fn indexed(n: u16) -> Option<Color> {
    u8::try_from(n).ok().map(Color::Indexed)
}

/// A true colour, if every part is in 0–255.
fn rgb(r: u16, g: u16, b: u16) -> Option<Color> {
    Some(Color::Rgb(
        u8::try_from(r).ok()?,
        u8::try_from(g).ok()?,
        u8::try_from(b).ok()?,
    ))
}

/// The DEC special graphics set (VT100 line drawing), which `ESC ( 0` puts in place of the
/// characters `_` to `~`.
fn line_drawing(ch: char) -> char {
    match ch {
        '_' => ' ',
        '`' => '◆',
        'a' => '▒',
        'b' => '␉',
        'c' => '␌',
        'd' => '␍',
        'e' => '␊',
        'f' => '°',
        'g' => '±',
        'h' => '␤',
        'i' => '␋',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        other => other,
    }
}

fn row_text(row: &Row) -> String {
    let mut text = String::new();
    for cell in row.iter().filter(|c| c.width > 0) {
        text.push(cell.ch);
        text.push_str(&cell.combining);
    }
    text.trim_end().to_owned()
}

impl vte::Perform for Screen {
    fn print(&mut self, c: char) {
        Screen::print(self, c);
    }

    fn execute(&mut self, byte: u8) {
        Screen::execute(self, byte);
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        if ignore {
            return;
        }
        let params: Vec<Vec<u16>> = params.iter().map(<[u16]>::to_vec).collect();
        match intermediates {
            [] => self.csi(&params, false, action),
            [b'?'] => self.csi(&params, true, action),
            [b'!'] if action == 'p' => self.soft_reset(),
            // Everything else with a private marker or an intermediate is not ours: `CSI > c`,
            // `CSI > 4 ; 2 m`, `CSI ? 1 $ p` and the like are ignored and never answered.
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        self.last_printed = None;
        match intermediates {
            [] => {}
            // Character set for G0 `(` and G1 `)`: `0` is line drawing, anything else is text.
            [b'('] => return self.graphics[0] = byte == b'0',
            [b')'] => return self.graphics[1] = byte == b'0',
            _ => return,
        }
        match byte {
            b'7' => self.save_cursor(),
            b'8' => self.restore_cursor(),
            b'=' => self.app_keypad = true,
            b'>' => self.app_keypad = false,
            b'H' => self.tabs[self.cursor.1] = true,
            b'D' => self.linefeed(),
            b'M' => self.reverse_index(),
            b'E' => {
                self.cursor.1 = 0;
                self.linefeed();
            }
            b'c' => self.reset(),
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        self.last_printed = None;
        if std::mem::take(&mut self.discard_osc) {
            return;
        }
        // Only the commands below are carried out. In particular the clipboard (OSC 52) is
        // never written or read, and no command is answered (`SPEC-0038` behaviour 9).
        let joined = |parts: &[&[u8]]| {
            parts
                .iter()
                .map(|p| String::from_utf8_lossy(p))
                .collect::<Vec<_>>()
                .join(";")
        };
        // OSC 0 and 2 ; title — the title may contain `;`.
        if let [kind, title @ ..] = params
            && (*kind == b"0" || *kind == b"2")
            && !title.is_empty()
        {
            self.title = joined(title)
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_TITLE)
                .collect();
        }
        // OSC 7 ; file://host/path — the shell's current folder (`SPEC-0161`).
        if let [kind, rest @ ..] = params
            && *kind == b"7"
        {
            let uri = joined(rest);
            if let Some(path) = uri
                .strip_prefix("file://")
                .and_then(|r| r.find('/').map(|i| &r[i..]))
                && path.len() <= MAX_DIR
            {
                self.cwd = Some(percent_decode(path));
                self.cwd_reports += 1;
            }
        }
        if let [kind, mark, rest @ ..] = params
            && *kind == b"133"
        {
            self.prompt_mark(mark, rest);
        }
        // OSC 8 ; params ; URI — the text that follows links to URI until an empty URI
        // (`SPEC-0153` behaviour 2). The URI may contain `;`.
        if let [kind, _params, uri @ ..] = params
            && *kind == b"8"
        {
            let uri = joined(uri);
            // An address that is too long, or one more than the store holds, is no link: the
            // text stays plain.
            self.style.link = if uri.is_empty() || uri.len() > MAX_LINK {
                0
            } else if let Some(i) = self.links.iter().position(|l| *l == uri) {
                (i + 1) as u16
            } else if self.links.len() < usize::from(u16::MAX)
                && self.link_bytes + uri.len() <= MAX_LINK_BYTES
            {
                self.link_bytes += uri.len();
                self.links.push(uri);
                self.links.len() as u16
            } else {
                0
            };
        }
    }
}

/// Decodes `%XX` escapes of a `file://` path.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Where [`Terminal::feed`] is in the stream, as far as operating system commands go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scan {
    /// Anywhere else.
    Text,
    /// After ESC.
    Escape,
    /// Inside an operating system command (after `ESC ]`) of so many bytes.
    Osc(usize),
}

/// A screen together with its escape-sequence parser.
pub struct Terminal {
    parser: vte::Parser,
    scan: Scan,
    /// The screen model.
    pub screen: Screen,
}

impl Terminal {
    /// A terminal of `rows` × `cols`.
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            parser: vte::Parser::new(),
            scan: Scan::Text,
            screen: Screen::new(rows, cols),
        }
    }

    /// Feeds program output.
    ///
    /// The parser collects an operating system command (`ESC ]` … BEL or ST) in memory until
    /// its end, without a limit of its own. So that output cannot make the terminal grow without
    /// bound, the bytes of a command beyond [`MAX_OSC`] are not passed on, and the command is
    /// dropped whole when it ends (`SPEC-0038` behaviour 9).
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut from = 0;
        for (i, &byte) in bytes.iter().enumerate() {
            self.scan = match (self.scan, byte) {
                // The ends of a command: BEL, CAN, SUB, and ESC (of ST or of a new sequence).
                (Scan::Osc(n), 0x07 | 0x18 | 0x1a | 0x1b) => {
                    self.screen.discard_osc = n > MAX_OSC;
                    self.parser.advance(&mut self.screen, &bytes[from..=i]);
                    self.screen.discard_osc = false;
                    from = i + 1;
                    if byte == 0x1b {
                        Scan::Escape
                    } else {
                        Scan::Text
                    }
                }
                (Scan::Osc(n), _) => {
                    if n >= MAX_OSC {
                        // Skip this byte.
                        self.parser.advance(&mut self.screen, &bytes[from..i]);
                        from = i + 1;
                    }
                    Scan::Osc(n.saturating_add(1).min(MAX_OSC + 1))
                }
                (_, 0x1b) => Scan::Escape,
                (Scan::Escape, b']') => Scan::Osc(0),
                // Control characters, DEL and bytes that are no ASCII leave the parser in its
                // escape state.
                (Scan::Escape, 0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f..=0xff) => Scan::Escape,
                _ => Scan::Text,
            };
        }
        self.parser.advance(&mut self.screen, &bytes[from..]);
    }

    /// Writes saved history lines as plain text, so a new shell's first prompt follows them
    /// (`SPEC-0205` behaviour 6). Control characters in the lines are dropped.
    pub fn restore(&mut self, lines: &[String]) {
        for line in lines {
            let clean: String = line.chars().filter(|c| !c.is_control()).collect();
            self.feed(clean.as_bytes());
            self.feed(b"\r\n");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(rows: usize, cols: usize, input: &str) -> Terminal {
        let mut t = Terminal::new(rows, cols);
        t.feed(input.as_bytes());
        t
    }

    #[test]
    fn prompt_marks_with_commands_statuses_and_stable_lines() {
        let mut t = Terminal::new(3, 20);
        t.screen.max_scrollback = 2;
        // First prompt: the shell's start without a command, then `ls` that fails.
        t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls x\r\n\x1b]133;C\x07no such file\r\n");
        t.feed(b"\x1b]133;D;2\x07\x1b]133;A\x07$ \x1b]133;B\x07");
        let p = t.screen.prompts();
        assert_eq!(p.len(), 2);
        assert_eq!(
            (p[0].line, p[0].command.as_deref(), p[0].exit),
            (0, Some("ls x"), Some(2))
        );
        assert_eq!(
            (p[1].line, p[1].command.as_deref(), p[1].exit),
            (2, None, None)
        );
        // `D` without a command (the first prompt of zsh) changes nothing.
        t.feed(b"\x1b]133;D;0\x07");
        assert_eq!(t.screen.prompts()[1].exit, None);
        // Lines leaving the scrollback keep their numbers; their marks go.
        t.feed(b"echo\r\n\x1b]133;C\x07\r\n\x1b]133;D;0\x07a\r\nb\r\nd");
        assert_eq!(t.screen.history_base(), 2);
        let p = t.screen.prompts();
        assert_eq!(p.len(), 1);
        assert_eq!(
            (p[0].line, p[0].command.as_deref(), p[0].exit),
            (2, Some("echo"), Some(0))
        );
        // ED 3 forgets the scrollback and its marks; numbering goes on.
        t.feed(b"\x1b[3J");
        assert_eq!(t.screen.history_base(), 4);
        assert!(t.screen.prompts().is_empty());
        // The alternate screen records no marks.
        t.feed(b"\x1b[?1049h\x1b]133;A\x07\x1b[?1049l");
        assert!(t.screen.prompts().is_empty());
    }

    /// SPEC-0206 behaviour 2: the command line the shell reports with `C` wins over the screen,
    /// and a line filled to the last column keeps its trailing space when the command goes on.
    #[test]
    fn reported_command_lines_and_wrapped_spaces() {
        let mut t = Terminal::new(2, 7);
        // zsh in a short terminal: the screen shows a sideways-scrolled part of the line.
        t.feed(b"\x1b]133;A\x07% \x1b]133;B\x07<0000\r\n");
        t.feed(b"\x1b]133;C;cmdline_url=echo%20a%3Bb%20%C3%A4\x07a\r\n\x1b]133;D;0\x07");
        let p = t.screen.prompts();
        assert_eq!(
            (p[0].command.as_deref(), p[0].exit),
            (Some("echo a;b ä"), Some(0))
        );
        // `% echo ` fills the 7 columns, the command goes on in the next row.
        t.feed(b"\x1b]133;A\x07% \x1b]133;B\x07echo hi\r\n\x1b]133;C\x07");
        assert_eq!(t.screen.prompts()[1].command.as_deref(), Some("echo hi"));
        // A row that merely ends in blanks before a line end is not a wrap.
        t.feed(b"\x1b]133;D;0\x07\x1b]133;A\x07% \x1b]133;B\x07a    \r\nb\r\n\x1b]133;C\x07");
        assert_eq!(t.screen.prompts()[2].command.as_deref(), Some("ab"));
    }

    /// SPEC-0313 AK-3: the last command with its output and exit status, while it runs, partly
    /// scrolled out, and nothing without marks.
    #[test]
    fn last_command_with_output_and_exit() {
        let mut t = Terminal::new(4, 30);
        assert_eq!(t.screen.last_command(), None);
        t.feed(b"plain output\r\n");
        assert_eq!(t.screen.last_command(), None, "no marks");
        t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07cargo test\r\n\x1b]133;C\x07");
        t.feed(b"running 2 tests\r\nerror: failed\r\n");
        let running = t.screen.last_command().unwrap();
        assert_eq!(running.command, "cargo test");
        assert_eq!(running.output, ["running 2 tests", "error: failed"]);
        assert_eq!(running.exit, None);
        t.feed(b"\x1b]133;D;101\x07\x1b]133;A\x07$ \x1b]133;B\x07");
        let done = t.screen.last_command().unwrap();
        assert_eq!(done.output, ["running 2 tests", "error: failed"]);
        assert_eq!(done.exit, Some(101));
        // A command whose prompt scrolled out of the history has no mark left: the caller falls
        // back to the last lines.
        t.screen.max_scrollback = 2;
        t.feed(b"seq 9\r\n\x1b]133;C\x07");
        for i in 1..=9 {
            t.feed(format!("{i}\r\n").as_bytes());
        }
        assert_eq!(t.screen.last_command(), None);
        assert_eq!(t.screen.history_lines(2), ["8", "9"]);
    }

    #[test]
    fn history_lines_and_restore() {
        let mut t = term(3, 10, "one\r\ntwo\r\nthree\r\nfour\r\n");
        assert_eq!(t.screen.history_lines(10), ["one", "two", "three", "four"]);
        assert_eq!(t.screen.history_lines(2), ["three", "four"]);
        let mut r = Terminal::new(3, 10);
        r.restore(&["saved\x1b[31m".to_owned(), "lines".to_owned()]);
        r.feed(b"$ ");
        assert_eq!(r.screen.history_lines(10), ["saved[31m", "lines", "$"]);
        assert_eq!(r.screen.cursor(), (2, 2));
        t.feed(b"x");
        assert_eq!(t.screen.history_lines(1), ["x"]);
    }

    #[test]
    fn osc7_reports_the_current_folder() {
        let t = term(2, 10, "\x1b]7;file://mac.local/Users/a/my%20dir\x07x");
        assert_eq!(t.screen.reported_dir(), Some("/Users/a/my dir"));
        assert_eq!(t.screen.text(), "x");
        assert_eq!(t.screen.dir_reports(), 1);
        assert_eq!(term(2, 10, "").screen.reported_dir(), None);
        // The same folder again is a new report (`SPEC-0161` v2).
        let twice = term(2, 10, "\x1b]7;file:///a\x07\x1b]7;file:///a\x07");
        assert_eq!(twice.screen.dir_reports(), 2);
    }

    #[test]
    fn printing_wrapping_and_scrollback() {
        let t = term(3, 5, "hello world\r\nab\tc");
        assert_eq!(t.screen.text(), " worl\nd\nab  c");
        assert_eq!(t.screen.scrollback(), ["hello"]);
        let t = term(2, 10, "a\tb\r\nxy\x08Z\r\n1\r\n2");
        assert_eq!(t.screen.text(), "1\n2");
        assert_eq!(t.screen.scrollback(), ["a       b", "xZ"]);
        // Deferred wrap: a full line followed by CR LF does not leave an empty line.
        let t = term(3, 3, "abc\r\ndef");
        assert_eq!(t.screen.text(), "abc\ndef");
        let t = term(2, 4, "\tx");
        assert_eq!(t.screen.cursor(), (0, 3));
    }

    #[test]
    fn cursor_erase_insert_delete() {
        let t = term(3, 6, "abcdef\x1b[2;3HXY\x1b[1;1H\x1b[K");
        assert_eq!(t.screen.text(), "\n  XY");
        let t = term(2, 6, "abcdef\x1b[1;3H\x1b[2P");
        assert_eq!(t.screen.text(), "abef");
        let t = term(2, 6, "abcd\x1b[1;2H\x1b[2@");
        assert_eq!(t.screen.text(), "a  bcd");
        let t = term(2, 6, "abcdef\x1b[1;2H\x1b[3X");
        assert_eq!(t.screen.text(), "a   ef");
        let t = term(3, 3, "1\r\n2\r\n3\x1b[2;1H\x1b[L");
        assert_eq!(t.screen.text(), "1\n\n2");
        let t = term(3, 3, "1\r\n2\r\n3\x1b[1;1H\x1b[M");
        assert_eq!(t.screen.text(), "2\n3");
        let t = term(3, 3, "abc\r\ndef\x1b[1;2H\x1b[1J");
        assert_eq!(t.screen.text(), "  c\ndef");
        let t = term(3, 3, "abc\r\ndef\x1b[2J");
        assert_eq!(t.screen.text(), "");
        let t = term(4, 4, "\x1b[3B\x1b[2C\x1b[A\x1b[DZ");
        assert_eq!(t.screen.text(), "\n\n Z");
        let t = term(4, 4, "\x1b[2;3r\x1b[3;1Ha\nb\nc");
        // The region scrolls; row 1 and 4 stay.
        assert_eq!(t.screen.text(), "\n b\n  c");
        assert!(t.screen.scrollback().is_empty());
        let t = term(3, 3, "a\x1b7\x1b[3;3Hz\x1b8b");
        assert_eq!(t.screen.text(), "ab\n\n  z");
    }

    #[test]
    fn sgr_styles_and_colours() {
        let t = term(
            1,
            10,
            "\x1b[1;4;31mA\x1b[38;5;200mB\x1b[38:2:1:2:3;44mC\x1b[0;7mD\x1b[mE",
        );
        let s = |c| t.screen.cell(0, c).unwrap().style;
        assert_eq!(
            (s(0).bold, s(0).underline, s(0).fg),
            (true, true, Color::Indexed(1))
        );
        assert_eq!(s(1).fg, Color::Indexed(200));
        assert_eq!((s(2).fg, s(2).bg), (Color::Rgb(1, 2, 3), Color::Indexed(4)));
        assert!(s(3).inverse && !s(3).bold);
        assert_eq!(s(4), Style::default());
        let t = term(1, 4, "\x1b[92;101mX\x1b[39;49mY");
        let s = |c| t.screen.cell(0, c).unwrap().style;
        assert_eq!((s(0).fg, s(0).bg), (Color::Indexed(10), Color::Indexed(9)));
        assert_eq!((s(1).fg, s(1).bg), (Color::Default, Color::Default));
    }

    #[test]
    fn alternate_screen_title_cursor_and_reports() {
        let mut t = term(
            3,
            10,
            "shell$ ls\x1b]0;my title\x07\x1b[?1049h\x1b[2;2Hfull\x1b[?25l",
        );
        assert!(t.screen.alternate());
        assert_eq!(t.screen.text(), "\n full");
        assert!(!t.screen.cursor_visible());
        assert_eq!(t.screen.title(), "my title");
        t.feed(b"\x1b[?1049l\x1b[?25h");
        assert!(!t.screen.app_cursor());
        t.feed(b"\x1b[?1h\x1b[?2004h");
        assert!(t.screen.app_cursor() && t.screen.bracketed_paste());
        t.feed(b"\x1b[?1l");
        assert!(!t.screen.app_cursor());
        assert!(!t.screen.alternate());
        assert_eq!(t.screen.text(), "shell$ ls");
        assert_eq!(t.screen.cursor(), (0, 9));
        t.feed(b"\x1b[6n\x07");
        assert_eq!(t.screen.take_responses(), b"\x1b[1;10R");
        assert_eq!(t.screen.bells(), 1);
        t.feed(b"\x1bc");
        assert_eq!(t.screen.text(), "");
    }

    #[test]
    fn mouse_modes_and_encodings() {
        let mut t = term(3, 10, "");
        assert_eq!(t.screen.mouse_mode(), MouseMode::Off);
        t.feed(b"\x1b[?1002h\x1b[?1006h");
        assert_eq!(t.screen.mouse_mode(), MouseMode::Drag);
        assert_eq!(t.screen.mouse_encoding(), MouseEncoding::Sgr);
        t.feed(b"\x1b[?1003h\x1b[?1005h");
        assert_eq!(t.screen.mouse_mode(), MouseMode::Motion);
        assert_eq!(t.screen.mouse_encoding(), MouseEncoding::Utf8);
        t.feed(b"\x1b[?1003l\x1b[?1005l\x1b[?9h");
        assert_eq!(t.screen.mouse_mode(), MouseMode::X10);
        assert_eq!(t.screen.mouse_encoding(), MouseEncoding::Default);
        t.feed(b"\x1bc");
        assert_eq!(
            t.screen.mouse_mode(),
            MouseMode::Off,
            "a reset turns reporting off"
        );
    }

    #[test]
    fn wide_and_combining_characters() {
        let t = term(2, 6, "日本x");
        assert_eq!(t.screen.text(), "日本x");
        assert_eq!(t.screen.cursor(), (0, 5));
        assert_eq!(t.screen.cell(0, 0).unwrap().width, 2);
        assert_eq!(t.screen.cell(0, 1).unwrap().width, 0);
        // A wide character that does not fit wraps and leaves the last column blank.
        let t = term(2, 5, "abcd語");
        assert_eq!(t.screen.text(), "abcd\n語");
        // Exactly filling the line defers the wrap.
        let t = term(2, 4, "語語");
        assert_eq!(
            (t.screen.text().as_str(), t.screen.cursor()),
            ("語語", (0, 3))
        );
        // Overwriting half of a wide character clears the other half.
        let t = term(1, 6, "語語\x1b[1;2HX");
        assert_eq!(t.screen.text(), " X語");
        let t = term(1, 6, "e\u{301}語\u{301}");
        assert_eq!(t.screen.text(), "e\u{301}語\u{301}");
        assert_eq!(t.screen.cursor(), (0, 3));
        let t = term(1, 1, "語");
        assert_eq!(t.screen.text(), "語");
    }

    #[test]
    fn resizing() {
        let mut t = term(3, 4, "a\r\nb\r\nc");
        t.screen.resize(2, 2);
        assert_eq!(t.screen.text(), "b\nc");
        assert_eq!(t.screen.scrollback(), ["a"]);
        t.screen.resize(4, 6);
        assert_eq!(t.screen.size(), (4, 6));
        assert_eq!(t.screen.text(), "b\nc");
    }
}
