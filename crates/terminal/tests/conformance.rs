//! Conformance run of the terminal model without a window (`SPEC-0038` v3, `SPEC-0062` v3): byte
//! streams go into the screen model and the grid, cursor, attributes, scrollback and answers are
//! compared with what ECMA-48, the xterm control sequences and the terminfo entry of our `TERM`
//! value (`xterm-256color`) say. The recordings in `fixtures/` were made once with programs that
//! are installed anyway (`fixtures/record.sh`).

use idecraft_terminal::{Cell, Color, Terminal, key_bytes, paste_bytes};

fn term(rows: usize, cols: usize, input: &[u8]) -> Terminal {
    let mut t = Terminal::new(rows, cols);
    t.feed(input);
    t
}

fn lines(t: &Terminal) -> Vec<String> {
    t.screen.text().split('\n').map(str::to_owned).collect()
}

fn cell(t: &Terminal, row: usize, col: usize) -> &Cell {
    t.screen.cell(row, col).expect("cell on the screen")
}

/// The grid has exactly the size the screen says, and the cursor is inside it.
fn assert_sound(t: &Terminal) {
    let (rows, cols) = t.screen.size();
    let (row, col) = t.screen.cursor();
    assert!(
        row < rows && col < cols,
        "cursor {row},{col} in {rows}x{cols}"
    );
    for r in 0..rows {
        assert!(t.screen.cell(r, cols - 1).is_some(), "row {r} is short");
        assert!(t.screen.cell(r, cols).is_none(), "row {r} is long");
    }
    assert!(t.screen.cell(rows, 0).is_none(), "too many rows");
}

// ---- Cursor ---------------------------------------------------------------------------------

#[test]
fn c01_cursor_position_and_relative_moves() {
    let mut t = term(5, 10, b"\x1b[3;4H");
    assert_eq!(t.screen.cursor(), (2, 3));
    for (seq, at) in [
        (&b"\x1b[2A"[..], (0, 3)),
        (b"\x1b[9A", (0, 3)),
        (b"\x1b[2B", (2, 3)),
        (b"\x1b[99B", (4, 3)),
        (b"\x1b[3C", (4, 6)),
        (b"\x1b[99C", (4, 9)),
        (b"\x1b[4D", (4, 5)),
        (b"\x1b[99D", (4, 0)),
        (b"\x1b[H", (0, 0)),
        (b"\x1b[99;99H", (4, 9)),
        (b"\x1b[0;0H", (0, 0)),
        (b"\x1b[2;3f", (1, 2)),
        (b"\x1b[2E", (3, 0)),
        (b"\x1b[1F", (2, 0)),
        (b"\x1b[7G", (2, 6)),
        (b"\x1b[5d", (4, 6)),
        (b"\x1b[;2H", (0, 1)),
        (b"\x1b[A\x1b[B\x1b[C\x1b[D", (1, 1)),
    ] {
        t.feed(seq);
        assert_eq!(t.screen.cursor(), at, "{:?}", String::from_utf8_lossy(seq));
    }
}

#[test]
fn c02_column_and_row_moves_of_ecma_48() {
    // HPA `, HPR a, VPR e (ECMA-48 8.3.57, 8.3.59, 8.3.160).
    let mut t = term(5, 10, b"\x1b[4`");
    assert_eq!(t.screen.cursor(), (0, 3));
    t.feed(b"\x1b[2a");
    assert_eq!(t.screen.cursor(), (0, 5));
    t.feed(b"\x1b[3e");
    assert_eq!(t.screen.cursor(), (3, 5));
    t.feed(b"\x1b[99a\x1b[99e");
    assert_eq!(t.screen.cursor(), (4, 9));
}

#[test]
fn c03_save_and_restore_the_cursor_position() {
    let mut t = term(5, 10, b"\x1b[2;3H\x1b7\x1b[5;9H\x1b8");
    assert_eq!(t.screen.cursor(), (1, 2));
    t.feed(b"\x1b[4;4H\x1b[s\x1b[H\x1b[u");
    assert_eq!(t.screen.cursor(), (3, 3));
}

#[test]
fn c04_save_and_restore_keep_the_attributes() {
    // DECSC saves the rendition with the position (xterm: "Save Cursor").
    let t = term(2, 10, b"\x1b[31;1m\x1b7\x1b[0m\x1b[2;1Hn\x1b8X");
    let x = cell(&t, 0, 0);
    assert_eq!(
        (x.ch, x.style.fg, x.style.bold),
        ('X', Color::Indexed(1), true)
    );
    let n = cell(&t, 1, 0);
    assert_eq!(
        (n.ch, n.style.fg, n.style.bold),
        ('n', Color::Default, false)
    );
}

// ---- Erase, insert, delete ------------------------------------------------------------------

#[test]
fn c05_erase_in_line() {
    let mut t = term(1, 10, b"0123456789\x1b[1;5H\x1b[K");
    assert_eq!(t.screen.text(), "0123");
    t.feed(b"\r0123456789\x1b[1;5H\x1b[1K");
    assert_eq!(t.screen.text(), "     56789");
    t.feed(b"\x1b[44m\x1b[2K");
    assert_eq!(t.screen.text(), "");
    assert_eq!(
        t.screen.cursor(),
        (0, 4),
        "erasing does not move the cursor"
    );
    // Erased cells take the current background (terminfo `bce`).
    assert_eq!(cell(&t, 0, 9).style.bg, Color::Indexed(4));
}

#[test]
fn c06_erase_in_display() {
    let fill = b"aaaa\r\nbbbb\r\ncccc";
    let mut t = term(3, 4, fill);
    t.feed(b"\x1b[2;3H\x1b[J");
    assert_eq!(lines(&t), ["aaaa", "bb"]);
    let mut t = term(3, 4, fill);
    t.feed(b"\x1b[2;3H\x1b[1J");
    assert_eq!(lines(&t), ["", "   b", "cccc"]);
    let mut t = term(3, 4, fill);
    t.feed(b"\x1b[2;3H\x1b[2J");
    assert_eq!(t.screen.text(), "");
    assert_eq!(t.screen.cursor(), (1, 2), "ED 2 does not home the cursor");
    // ED 3 clears the scrollback and leaves the screen.
    let mut t = term(2, 4, b"1\r\n2\r\n3\r\n4");
    assert_eq!(t.screen.scrollback(), ["1", "2"]);
    t.feed(b"\x1b[3J");
    assert_eq!(t.screen.scrollback_len(), 0);
    assert_eq!(lines(&t), ["3", "4"]);
}

#[test]
fn c07_erase_characters() {
    let mut t = term(1, 10, b"0123456789\x1b[1;3H\x1b[3X");
    assert_eq!(t.screen.text(), "01   56789");
    assert_eq!(t.screen.cursor(), (0, 2));
    t.feed(b"\x1b[999X");
    assert_eq!(t.screen.text(), "01");
}

#[test]
fn c08_insert_and_delete_lines() {
    let mut t = term(5, 4, b"a\r\nb\r\nc\r\nd\r\ne\x1b[2;3H\x1b[2L");
    assert_eq!(lines(&t), ["a", "", "", "b", "c"]);
    assert_eq!(
        t.screen.cursor(),
        (1, 0),
        "IL puts the cursor in the first column"
    );
    t.feed(b"\x1b[M");
    assert_eq!(lines(&t), ["a", "", "b", "c"]);
    // Inside a scroll region the lines below the region stay.
    let mut t = term(5, 4, b"a\r\nb\r\nc\r\nd\r\ne\x1b[2;4r\x1b[2;1H\x1b[L");
    assert_eq!(lines(&t), ["a", "", "b", "c", "e"]);
    t.feed(b"\x1b[99M");
    assert_eq!(lines(&t), ["a", "", "", "", "e"]);
    // Outside the region IL and DL do nothing.
    t.feed(b"\x1b[5;1H\x1b[L\x1b[M");
    assert_eq!(lines(&t), ["a", "", "", "", "e"]);
    assert_eq!(t.screen.scrollback_len(), 0);
}

#[test]
fn c09_insert_and_delete_characters() {
    let mut t = term(1, 8, b"abcdefgh\x1b[1;3H\x1b[2@");
    assert_eq!(t.screen.text(), "ab  cdef");
    t.feed(b"\x1b[3P");
    assert_eq!(t.screen.text(), "abdef");
    t.feed(b"\x1b[999@");
    assert_eq!(t.screen.text(), "ab");
    t.feed(b"\x1b[1;1H\x1b[999P");
    assert_eq!(t.screen.text(), "");
    assert_sound(&t);
}

#[test]
fn c10_insert_mode() {
    // IRM (ECMA-48 mode 4; terminfo smir/rmir of xterm-256color).
    let mut t = term(1, 12, b"world\r\x1b[4hhello \x1b[4l");
    assert_eq!(t.screen.text(), "hello world");
    t.feed(b"\rJ");
    assert_eq!(t.screen.text(), "Jello world");
    // Inserted text pushes the rest off the right edge; nothing wraps.
    let t = term(2, 4, b"abcd\r\x1b[4hXY");
    assert_eq!(lines(&t), ["XYab"]);
}

// ---- Scrolling ------------------------------------------------------------------------------

#[test]
fn c11_scroll_region() {
    let mut t = term(5, 4, b"a\r\nb\r\nc\r\nd\r\ne\x1b[2;4r");
    assert_eq!(t.screen.cursor(), (0, 0), "DECSTBM homes the cursor");
    t.feed(b"\x1b[4;1H\n");
    assert_eq!(lines(&t), ["a", "c", "d", "", "e"]);
    assert_eq!(
        t.screen.scrollback_len(),
        0,
        "a region below the top keeps no history"
    );
    t.feed(b"\x1b[2;1H\x1bM");
    assert_eq!(lines(&t), ["a", "", "c", "d", "e"]);
    t.feed(b"\x1b[2S");
    assert_eq!(lines(&t), ["a", "d", "", "", "e"]);
    t.feed(b"\x1b[1T");
    assert_eq!(lines(&t), ["a", "", "d", "", "e"]);
    // Cursor up and down stop at the region when they start inside it.
    t.feed(b"\x1b[3;1H\x1b[9A");
    assert_eq!(t.screen.cursor(), (1, 0));
    t.feed(b"\x1b[9B");
    assert_eq!(t.screen.cursor(), (3, 0));
    // An invalid region is ignored; `CSI r` alone resets it.
    t.feed(b"\x1b[4;2r\x1b[r\x1b[5;1H\n");
    assert_eq!(lines(&t), ["", "d", "", "e"]);
    assert_eq!(t.screen.scrollback(), ["a"]);
}

#[test]
fn c12_index_reverse_index_and_next_line() {
    let mut t = term(3, 4, b"a\r\nb\r\nc\x1bD");
    assert_eq!(lines(&t), ["b", "c"]);
    assert_eq!(t.screen.cursor(), (2, 1), "IND keeps the column");
    t.feed(b"\x1bE");
    assert_eq!(t.screen.cursor(), (2, 0));
    assert_eq!(lines(&t), ["c"]);
    t.feed(b"\x1b[H\x1bM");
    assert_eq!(lines(&t), ["", "c"]);
}

#[test]
fn c13_origin_mode() {
    // DECOM: positions count from the scroll region and stay inside it.
    let mut t = term(6, 10, b"\x1b[3;5r\x1b[?6h");
    assert_eq!(t.screen.cursor(), (2, 0));
    t.feed(b"\x1b[2;4H");
    assert_eq!(t.screen.cursor(), (3, 3));
    t.feed(b"\x1b[99;1H");
    assert_eq!(t.screen.cursor(), (4, 0));
    t.feed(b"\x1b[1d");
    assert_eq!(t.screen.cursor(), (2, 0));
    t.feed(b"\x1b[2;3H\x1b[6n");
    assert_eq!(
        t.screen.take_responses(),
        b"\x1b[2;3R",
        "CPR is relative too"
    );
    t.feed(b"\x1b[?6l");
    assert_eq!(t.screen.cursor(), (0, 0));
    t.feed(b"\x1b[6;1H");
    assert_eq!(t.screen.cursor(), (5, 0));
}

// ---- Wrapping and tabs ----------------------------------------------------------------------

#[test]
fn c14_autowrap_and_the_pending_wrap_at_the_last_column() {
    let mut t = term(3, 5, b"abcde");
    assert_eq!(
        t.screen.cursor(),
        (0, 4),
        "the cursor stays on the last column"
    );
    assert_eq!(lines(&t), ["abcde"]);
    t.feed(b"f");
    assert_eq!(lines(&t), ["abcde", "f"]);
    assert_eq!(t.screen.cursor(), (1, 1));
    // CR, BS and cursor movement cancel the pending wrap.
    let t = term(3, 5, b"abcde\rX");
    assert_eq!(lines(&t), ["Xbcde"]);
    let t = term(3, 5, b"abcde\x08X");
    assert_eq!(lines(&t), ["abcXe"]);
    let t = term(3, 5, b"abcde\x1b[DX");
    assert_eq!(lines(&t), ["abcXe"]);
    let t = term(3, 5, b"abcde\x1b[1;5HX");
    assert_eq!(lines(&t), ["abcdX"]);
    // A line feed in the pending state moves down in the same column and prints there.
    let t = term(3, 5, b"abcde\nX");
    assert_eq!(lines(&t), ["abcde", "    X"]);
    // The wrap at the bottom scrolls.
    let t = term(2, 3, b"abcdefg");
    assert_eq!(lines(&t), ["def", "g"]);
    assert_eq!(t.screen.scrollback(), ["abc"]);
}

#[test]
fn c15_autowrap_off() {
    // DECAWM off: the last column is overwritten, nothing wraps.
    let mut t = term(2, 5, b"\x1b[?7labcdefg");
    assert_eq!(lines(&t), ["abcdg"]);
    assert_eq!(t.screen.cursor(), (0, 4));
    t.feed(b"\x1b[?7h\rabcdefg");
    assert_eq!(lines(&t), ["abcde", "fg"]);
}

#[test]
fn c16_default_tab_stops() {
    let mut t = term(2, 20, b"a\tb\tc");
    assert_eq!(t.screen.text(), "a       b       c");
    t.feed(b"\t\t\t");
    assert_eq!(t.screen.cursor(), (0, 19), "a tab stops at the last column");
    t.feed(b"Z");
    assert_eq!(t.screen.cursor(), (0, 19));
    assert_eq!(cell(&t, 0, 19).ch, 'Z');
}

#[test]
fn c17_setting_and_clearing_tab_stops() {
    // HTS at columns 3 and 5; TBC 0 clears one, TBC 3 all (terminfo hts, tbc).
    let mut t = term(1, 30, b"\x1b[1;4H\x1bH\x1b[1;6H\x1bH\r\t");
    assert_eq!(t.screen.cursor(), (0, 3));
    t.feed(b"\t");
    assert_eq!(t.screen.cursor(), (0, 5));
    t.feed(b"\x1b[g\r\t\t");
    assert_eq!(t.screen.cursor(), (0, 8));
    t.feed(b"\x1b[3g\r\t");
    assert_eq!(t.screen.cursor(), (0, 29), "no stops: to the last column");
    // CHT and CBT (terminfo cbt) with the default stops.
    let mut t = term(1, 30, b"\x1b[2I");
    assert_eq!(t.screen.cursor(), (0, 16));
    t.feed(b"ab\x1b[Z");
    assert_eq!(t.screen.cursor(), (0, 16));
    t.feed(b"\x1b[2Z");
    assert_eq!(t.screen.cursor(), (0, 0));
    t.feed(b"\x1b[9Z\x1b[99I");
    assert_eq!(t.screen.cursor(), (0, 29));
}

// ---- SGR ------------------------------------------------------------------------------------

#[test]
fn c18_sgr_attributes_and_their_resets() {
    let t = term(
        1,
        20,
        b"\x1b[1;3;4;7mA\x1b[22mB\x1b[23mC\x1b[24mD\x1b[27mE\x1b[1;4m\x1b[mF",
    );
    let s = |c: usize| {
        let s = cell(&t, 0, c).style;
        (s.bold, s.italic, s.underline, s.inverse)
    };
    assert_eq!(s(0), (true, true, true, true));
    assert_eq!(s(1), (false, true, true, true));
    assert_eq!(s(2), (false, false, true, true));
    assert_eq!(s(3), (false, false, false, true));
    assert_eq!(s(4), (false, false, false, false));
    assert_eq!(s(5), (false, false, false, false));
    // Double underline (21) is drawn as underline.
    let t = term(1, 4, b"\x1b[21mA");
    assert!(cell(&t, 0, 0).style.underline);
}

#[test]
fn c19_sgr_16_256_and_true_colours_and_their_reset() {
    let t = term(
        1,
        20,
        b"\x1b[31;42mA\x1b[91;102mB\x1b[38;5;196;48;5;21mC\x1b[38;2;1;2;3;48;2;4;5;6mD\x1b[39mE\x1b[49mF\x1b[35;45m\x1b[0mG",
    );
    let c = |i: usize| (cell(&t, 0, i).style.fg, cell(&t, 0, i).style.bg);
    assert_eq!(c(0), (Color::Indexed(1), Color::Indexed(2)));
    assert_eq!(c(1), (Color::Indexed(9), Color::Indexed(10)));
    assert_eq!(c(2), (Color::Indexed(196), Color::Indexed(21)));
    assert_eq!(c(3), (Color::Rgb(1, 2, 3), Color::Rgb(4, 5, 6)));
    assert_eq!(c(4), (Color::Default, Color::Rgb(4, 5, 6)));
    assert_eq!(c(5), (Color::Default, Color::Default));
    assert_eq!(c(6), (Color::Default, Color::Default));
    // The colon forms of ITU T.416, with and without the colour space field.
    let t = term(
        1,
        20,
        b"\x1b[38:5:196mA\x1b[38:2:1:2:3mB\x1b[38:2::7:8:9mC\x1b[48:2:0:7:8:9mD",
    );
    assert_eq!(cell(&t, 0, 0).style.fg, Color::Indexed(196));
    assert_eq!(cell(&t, 0, 1).style.fg, Color::Rgb(1, 2, 3));
    assert_eq!(cell(&t, 0, 2).style.fg, Color::Rgb(7, 8, 9));
    assert_eq!(cell(&t, 0, 3).style.bg, Color::Rgb(7, 8, 9));
}

#[test]
fn c20_sgr_parameters_are_not_misread() {
    let plain = |input: &[u8]| {
        let t = term(1, 4, input);
        let s = cell(&t, 0, 0).style;
        (s.fg, s.bg, s.bold, s.italic, s.underline, s.inverse)
    };
    let none = (Color::Default, Color::Default, false, false, false, false);
    // Curly underline is an underline, not underline plus italic.
    assert_eq!(
        plain(b"\x1b[4:3mX"),
        (Color::Default, Color::Default, false, false, true, false)
    );
    // `4:0` ends the underline and nothing else.
    assert_eq!(
        plain(b"\x1b[1;4m\x1b[4:0mX"),
        (Color::Default, Color::Default, true, false, false, false)
    );
    // The underline colour (58) takes its parameters with it.
    assert_eq!(plain(b"\x1b[58;5;1mX"), none);
    assert_eq!(plain(b"\x1b[58;2;1;4;7mX"), none);
    assert_eq!(plain(b"\x1b[58:2::1:4:7mX"), none);
    // Values outside 0–255 select no colour.
    assert_eq!(plain(b"\x1b[38;5;300mX"), none);
    assert_eq!(plain(b"\x1b[38;2;256;0;0mX"), none);
    // A truncated colour changes nothing and does not eat the next sequence.
    assert_eq!(plain(b"\x1b[38;5mX"), none);
    assert_eq!(plain(b"\x1b[38;2;1mX"), none);
    // Sequences with a private marker are not SGR: `CSI > 4 ; 2 m` asks for modifyOtherKeys.
    assert_eq!(plain(b"\x1b[>4;2mX"), none);
    assert_eq!(plain(b"\x1b[?4mX"), none);
    // Attributes the model does not keep (dim, blink, conceal, strike) are ignored.
    assert_eq!(plain(b"\x1b[2;5;8;9mX"), none);
}

// ---- Alternate screen -----------------------------------------------------------------------

#[test]
fn c21_alternate_screen_1049_saves_and_restores_the_cursor() {
    let mut t = term(3, 10, b"one\r\ntwo\x1b[?1049h");
    assert!(t.screen.alternate());
    assert_eq!(t.screen.text(), "", "the alternate screen starts blank");
    t.feed(b"\x1b[H1\r\n2\r\n3\r\n4\r\n5");
    assert_eq!(
        t.screen.scrollback_len(),
        0,
        "the alternate screen has no history"
    );
    t.feed(b"\x1b[?1049l");
    assert!(!t.screen.alternate());
    assert_eq!(lines(&t), ["one", "two"]);
    assert_eq!(t.screen.cursor(), (1, 3));
    // Entering again shows a blank screen.
    t.feed(b"\x1b[?1049h");
    assert_eq!(t.screen.text(), "");
}

#[test]
fn c22_alternate_screen_1047_without_and_1048_with_the_saved_cursor() {
    let mut t = term(4, 10, b"one\r\ntwo\x1b[?1048h\x1b[?1047h");
    assert!(t.screen.alternate());
    t.feed(b"\x1b[4;6Halt\x1b[?1047l");
    assert!(!t.screen.alternate());
    assert_eq!(lines(&t), ["one", "two"]);
    assert_eq!(
        t.screen.cursor(),
        (3, 8),
        "1047 alone does not restore the cursor"
    );
    t.feed(b"\x1b[?1048l");
    assert_eq!(t.screen.cursor(), (1, 3));
    // Mode 47 is the same switch.
    t.feed(b"\x1b[?47h");
    assert!(t.screen.alternate());
    assert_eq!(t.screen.text(), "");
    t.feed(b"\x1b[?47l");
    assert_eq!(lines(&t), ["one", "two"]);
}

#[test]
fn c23_each_screen_has_its_own_saved_cursor() {
    // A program that saves the cursor on the alternate screen must not move the shell's cursor.
    let t = term(4, 10, b"\x1b[3;4H\x1b[?1049h\x1b[1;6H\x1b7\x1b[?1049l");
    assert_eq!(t.screen.cursor(), (2, 3));
}

// ---- Modes ----------------------------------------------------------------------------------

#[test]
fn c24_bracketed_paste() {
    let mut t = term(2, 10, b"\x1b[?2004h");
    assert!(t.screen.bracketed_paste());
    assert_eq!(paste_bytes("a\nb\r\nc", true), b"\x1b[200~a\rb\rc\x1b[201~");
    t.feed(b"\x1b[?2004l");
    assert!(!t.screen.bracketed_paste());
    assert_eq!(paste_bytes("a\nb", false), b"a\rb");
}

#[test]
fn c25_pasted_text_cannot_end_the_bracketed_paste() {
    // Text from the clipboard that contains the end mark would run what follows it as typed.
    let sent = paste_bytes("x\x1b[201~rm -rf ~\n", true);
    let body = &sent[6..sent.len() - 6];
    assert!(sent.starts_with(b"\x1b[200~") && sent.ends_with(b"\x1b[201~"));
    assert!(!body.windows(6).any(|w| w == b"\x1b[201~"), "{body:?}");
    assert!(!body.contains(&0x1b), "no escape character inside a paste");
    // Without the mode the escape character goes too: a paste is text, not key presses.
    assert!(!paste_bytes("a\x1b[Ab", false).contains(&0x1b));
    // Tabs and line ends stay.
    assert_eq!(paste_bytes("a\tb\n", false), b"a\tb\r");
}

#[test]
fn c26_application_cursor_and_keypad_modes() {
    let mut t = term(2, 10, b"\x1b[?1h\x1b=");
    assert!(t.screen.app_cursor());
    assert!(t.screen.app_keypad());
    t.feed(b"\x1b[?1l\x1b>");
    assert!(!t.screen.app_cursor());
    assert!(!t.screen.app_keypad());
    assert_eq!(t.screen.text(), "", "the mode sequences print nothing");
    // terminfo smkx/rmkx are exactly these.
    let keys = include_str!("fixtures/xterm-256color.keys");
    assert!(keys.contains("smkx=\\E[?1h\\E=,") && keys.contains("rmkx=\\E[?1l\\E>,"));
}

#[test]
fn c27_soft_reset() {
    // DECSTR, part of terminfo is2/rs2: modes and rendition back to their defaults, text kept.
    let mut t = term(
        4,
        10,
        b"keep\x1b[2;3r\x1b[?6h\x1b[?7l\x1b[4h\x1b[?1h\x1b=\x1b[?25l\x1b[1;31m\x1b(0",
    );
    t.feed(b"\x1b[!p");
    assert_eq!(t.screen.text(), "keep");
    assert!(t.screen.cursor_visible() && !t.screen.app_cursor() && !t.screen.app_keypad());
    t.feed(b"\x1b[4;1Hq\n");
    assert_eq!(
        lines(&t),
        ["", "", "q"],
        "the region is the whole screen again"
    );
    assert_eq!(t.screen.scrollback(), ["keep"]);
    let q = cell(&t, 2, 0);
    assert_eq!(
        (q.ch, q.style.fg, q.style.bold),
        ('q', Color::Default, false)
    );
}

#[test]
fn c28_full_reset() {
    let mut t = term(
        2,
        10,
        b"1\r\n2\r\n3\x1b]0;t\x07\x1b[?1049h\x1b[?1000h\x1b[?2004h\x1b[31m\x1bc",
    );
    assert!(!t.screen.alternate() && !t.screen.bracketed_paste());
    assert_eq!(t.screen.mouse_mode(), idecraft_terminal::MouseMode::Off);
    assert_eq!(t.screen.text(), "");
    assert_eq!(t.screen.cursor(), (0, 0));
    assert_eq!(t.screen.scrollback(), ["1"], "the history stays");
    t.feed(b"x");
    assert_eq!(cell(&t, 0, 0).style.fg, Color::Default);
}

// ---- OSC ------------------------------------------------------------------------------------

#[test]
fn c29_window_title() {
    let mut t = term(2, 10, b"\x1b]0;first\x07");
    assert_eq!(t.screen.title(), "first");
    t.feed(b"\x1b]2;second\x1b\\");
    assert_eq!(t.screen.title(), "second");
    // A title may contain semicolons and non-ASCII text.
    t.feed("\x1b]2;a;b; ü — c\x07".as_bytes());
    assert_eq!(t.screen.title(), "a;b; ü — c");
    // The icon name (OSC 1) is not the title.
    t.feed(b"\x1b]1;icon\x07");
    assert_eq!(t.screen.title(), "a;b; ü — c");
    t.feed(b"\x1b]2;\x07");
    assert_eq!(t.screen.title(), "");
    assert_eq!(t.screen.text(), "");
}

#[test]
fn c30_hyperlinks() {
    let t = term(
        1,
        30,
        b"a\x1b]8;;https://example.org/x;y?q=1\x1b\\li\x1b[31m\x1b[0mnk\x1b]8;;\x1b\\b\x1b]8;id=7;file:///tmp/f\x07c\x1b]8;;\x07",
    );
    assert_eq!(t.screen.text(), "alinkbc");
    let link = |c: usize| t.screen.link_uri(cell(&t, 0, c).style.link);
    assert_eq!(link(0), None);
    for c in 1..=4 {
        assert_eq!(link(c), Some("https://example.org/x;y?q=1"), "column {c}");
    }
    assert_eq!(link(5), None);
    assert_eq!(link(6), Some("file:///tmp/f"));
}

#[test]
fn c31_working_directory() {
    let mut t = term(1, 10, b"\x1b]7;file://host/tmp/a%20b/%C3%BC\x07");
    assert_eq!(t.screen.reported_dir(), Some("/tmp/a b/ü"));
    t.feed(b"\x1b]7;file:///srv;x\x1b\\");
    assert_eq!(t.screen.reported_dir(), Some("/srv;x"));
    // Anything but a file address is ignored.
    t.feed(b"\x1b]7;https://example.org/x\x07\x1b]7;/etc\x07");
    assert_eq!(t.screen.reported_dir(), Some("/srv;x"));
}

// ---- Characters -----------------------------------------------------------------------------

#[test]
fn c32_wide_characters_and_combining_marks() {
    let t = term(2, 5, "日本e\u{301}".as_bytes());
    assert_eq!((cell(&t, 0, 0).ch, cell(&t, 0, 0).width), ('日', 2));
    assert_eq!(cell(&t, 0, 1).width, 0);
    assert_eq!((cell(&t, 0, 2).ch, cell(&t, 0, 2).width), ('本', 2));
    assert_eq!(
        (cell(&t, 0, 4).ch, cell(&t, 0, 4).combining.as_str()),
        ('e', "\u{301}")
    );
    assert_eq!(t.screen.cursor(), (0, 4));
    // One column left: the wide character goes to the next line.
    let t = term(2, 5, "abcd日".as_bytes());
    assert_eq!(lines(&t), ["abcd", "日"]);
    // Overwriting half of a wide character clears the other half.
    let t = term(1, 6, "日本\x1b[1;2Hx".as_bytes());
    assert_eq!(t.screen.text(), " x本");
    // A mark after a wide character joins it, also in the pending-wrap state.
    let t = term(1, 4, "ab日\u{3099}".as_bytes());
    assert_eq!(cell(&t, 0, 2).combining, "\u{3099}");
}

#[test]
fn c33_emoji_with_variation_selectors_and_joiners() {
    // Width per code point (UAX #11): a variation selector or a joiner adds no column.
    let t = term(1, 12, "☺\u{fe0f}x".as_bytes());
    assert_eq!((cell(&t, 0, 0).ch, cell(&t, 0, 0).width), ('☺', 1));
    assert_eq!(cell(&t, 0, 0).combining, "\u{fe0f}");
    assert_eq!(cell(&t, 0, 1).ch, 'x');
    let t = term(1, 12, "👍🏽x".as_bytes());
    assert_eq!(
        (
            cell(&t, 0, 0).width,
            cell(&t, 0, 2).width,
            cell(&t, 0, 4).ch
        ),
        (2, 2, 'x')
    );
    let t = term(1, 12, "👨\u{200d}👩x".as_bytes());
    assert_eq!(cell(&t, 0, 0).combining, "\u{200d}");
    assert_eq!((cell(&t, 0, 2).ch, cell(&t, 0, 4).ch), ('👩', 'x'));
    assert_eq!(t.screen.text(), "👨\u{200d}👩x", "the text is unchanged");
}

#[test]
fn c34_line_drawing_character_set() {
    // DEC special graphics in G0 (terminfo smacs/rmacs) and in G1 with SO/SI.
    let t = term(2, 10, b"\x1b(0lqqk\x1b(Bq\r\n\x1b)0\x0ex\x0fx");
    assert_eq!(lines(&t), ["┌──┐q", "│x"]);
    let t = term(1, 40, b"\x1b(0`afgjklmnopqrstuvwxyz{|}~_A\x1b(B");
    assert_eq!(t.screen.text(), "◆▒°±┘┐┌└┼⎺⎻─⎼⎽├┤┴┬│≤≥π≠£· A");
}

#[test]
fn c35_repeat_character() {
    // REP (ECMA-48 8.3.103).
    let mut t = term(2, 6, b"ab\x1b[3b");
    assert_eq!(t.screen.text(), "abbbb");
    t.feed(b"\x1b[3b");
    assert_eq!(lines(&t), ["abbbbb", "bb"]);
    // Nothing to repeat after a control function.
    let t = term(2, 6, b"a\r\x1b[3b");
    assert_eq!(t.screen.text(), "a");
    // The largest count a parameter can hold.
    let t = term(2, 3, b"z\x1b[65535b");
    assert_eq!(lines(&t), ["zzz", "z"]);
    assert!(t.screen.scrollback_len() <= 10_000);
}

// ---- Resizing -------------------------------------------------------------------------------

#[test]
fn c36_resizing_keeps_the_lines_as_they_are() {
    // No reflow (SPEC-0038 behaviour 8): lines are cut or padded, rows go to the history.
    let mut t = term(3, 10, b"0123456789abc");
    assert_eq!(lines(&t), ["0123456789", "abc"]);
    t.screen.resize(3, 5);
    assert_eq!(lines(&t), ["01234", "abc"]);
    assert_eq!(t.screen.cursor(), (1, 3));
    t.screen.resize(3, 10);
    assert_eq!(lines(&t), ["01234", "abc"]);
    let mut t = term(3, 4, b"a\r\nb\r\nc");
    t.screen.resize(2, 4);
    assert_eq!(lines(&t), ["b", "c"]);
    assert_eq!(t.screen.scrollback(), ["a"]);
    assert_eq!(t.screen.cursor(), (1, 1));
    // A wide character cut in half by the new edge goes.
    let mut t = term(1, 6, "日本日".as_bytes());
    t.screen.resize(1, 5);
    assert_eq!(t.screen.text(), "日本");
    assert_sound(&t);
    // The scroll region is the whole new screen; custom tab stops stay.
    let mut t = term(4, 20, b"\x1b[2;3r\x1b[1;4H\x1bH");
    t.screen.resize(6, 30);
    t.feed(b"\x1b[6;1Hx\n\r\t");
    assert_eq!(t.screen.scrollback_len(), 1);
    assert_eq!(t.screen.cursor(), (5, 3));
}

#[test]
fn c37_resizing_on_the_alternate_screen() {
    let mut t = term(5, 10, b"1\r\n2\r\n3\r\n4\r\n5\x1b[?1049h\x1b[5;10Hx");
    t.screen.resize(2, 4);
    assert_sound(&t);
    t.feed(b"\x1b[?1049l");
    assert_sound(&t);
    assert_eq!(lines(&t), ["4", "5"], "the shell's last lines are shown");
    assert_eq!(t.screen.scrollback(), ["1", "2", "3"]);
    assert_eq!(t.screen.cursor(), (1, 1));
    t.feed(b"\x1b[2;4Hy\x1b[J\x1b[2L");
    assert_sound(&t);
    // Growing while the alternate screen is shown.
    let mut t = term(2, 4, b"a\x1b[?1049h");
    t.screen.resize(6, 12);
    t.feed(b"\x1b[?1049l\x1b[6;12Hz");
    assert_sound(&t);
    assert_eq!(cell(&t, 5, 11).ch, 'z');
}

// ---- Answers --------------------------------------------------------------------------------

#[test]
fn c38_status_and_device_attribute_answers() {
    let mut t = term(5, 10, b"\x1b[3;4H\x1b[6n");
    assert_eq!(t.screen.take_responses(), b"\x1b[3;4R");
    t.feed(b"\x1b[5n");
    assert_eq!(t.screen.take_responses(), b"\x1b[0n");
    // Primary device attributes: the answer terminfo names as u8 for the question u9.
    let keys = include_str!("fixtures/xterm-256color.keys");
    assert!(keys.contains("u9=\\E[c,") && keys.contains("u8=\\E[?1;2c,"));
    t.feed(b"\x1b[c");
    assert_eq!(t.screen.take_responses(), b"\x1b[?1;2c");
    t.feed(b"\x1b[0c");
    assert_eq!(t.screen.take_responses(), b"\x1b[?1;2c");
    assert_eq!(t.screen.text(), "");
}

#[test]
fn c39_queries_that_would_echo_program_controlled_text_get_no_answer() {
    // Whatever a terminal answers arrives at the shell as if typed. Title, colour, clipboard,
    // setting and capability queries are not answered; neither are the secondary and tertiary
    // device attributes and the answerback (SPEC-0038 behaviour 9).
    for query in [
        &b"\x1b]0;$(touch pwned)\x07\x1b[21t"[..],
        b"\x1b[20t",
        b"\x1b[14t\x1b[18t\x1b[19t",
        b"\x1b]10;?\x07",
        b"\x1b]11;?\x1b\\",
        b"\x1b]4;1;?\x07",
        b"\x1b]52;c;?\x07",
        b"\x1bP$qm\x1b\\",
        b"\x1bP+q544e\x1b\\",
        b"\x1b[>c",
        b"\x1b[>0c",
        b"\x1b[=c",
        b"\x1b[>q",
        b"\x1b[?u",
        b"\x1b[?996n",
        b"\x1b[?6n",
        b"\x1b[?1$p",
        b"\x1b[2$p",
        b"\x05",
        b"\x1b[x",
    ] {
        let mut t = term(3, 20, query);
        assert_eq!(
            String::from_utf8_lossy(&t.screen.take_responses()),
            "",
            "{:?}",
            String::from_utf8_lossy(query)
        );
        assert_eq!(t.screen.text(), "", "{:?}", String::from_utf8_lossy(query));
    }
}

#[test]
fn c40_output_cannot_write_the_clipboard() {
    // OSC 52 is not carried out: no state changes, nothing is answered.
    let mut t = term(
        2,
        20,
        b"\x1b]2;t\x07\x1b]52;c;cm0gLXJmIH4K\x07\x1b]52;p;!\x1b\\ok",
    );
    assert_eq!(t.screen.text(), "ok");
    assert_eq!(t.screen.title(), "t");
    assert!(t.screen.take_responses().is_empty());
}

// ---- Robustness -----------------------------------------------------------------------------

#[test]
fn c41_malformed_sequences_never_panic() {
    let cases: &[&[u8]] = &[
        b"\x1b[",
        b"\x1b[;;;;;;;;H",
        b"\x1b[65535;65535H\x1b[65535A\x1b[65535B\x1b[65535C\x1b[65535D",
        b"\x1b[99999999999999999999;99999999999999999999H",
        b"\x1b[65535L\x1b[65535M\x1b[65535P\x1b[65535@\x1b[65535X\x1b[65535S\x1b[65535T",
        b"\x1b[65535;1r\x1b[1;65535r\x1b[0;0r\x1b[5;5r\x1b[5;4r",
        b"\x1b[1;2;3;4;5;6;7;8;9;10;11;12;13;14;15;16;17;18;19;20;21;22;23;24;25;26;27;28;29;30;31;32;33;34;35;36;37;38;39;40m",
        b"\x1b[38;2m\x1b[38;5m\x1b[38m\x1b[48;2;1;2m\x1b[38:2m\x1b[38:5m\x1b[:::m\x1b[4:m",
        b"\x1b[?65535h\x1b[?h\x1b[?;l\x1b[?1049l\x1b[?1049l\x1b[?1048l\x1b[?1047l",
        b"\x1b[1;1\x1b[2;2H",
        b"\x1b[12\x18H\x1b[1\x1aH",
        b"\x1b[ q\x1b[!p\x1b[\"q\x1b[$~\x1b[1;2;3;4$x",
        b"\x1b]\x07\x1b];\x07\x1b]0\x07\x1b]8\x07\x1b]8;\x07\x1b]7\x07\x1b]7;\x07\x1b]133\x07\x1b]133;\x07\x1b]133;D;x\x07",
        b"\x1b]0;unterminated",
        b"\x1bP\x1b\\\x1bPq#0;2;0;0;0\x1b\\\x1b_apc\x1b\\\x1b^pm\x1b\\\x1bXsos\x1b\\",
        b"\xff\xfe\xc0\x80\xed\xa0\x80\xf4\x90\x80\x80\xe2\x82",
        b"\x1b\x1b\x1b[\x1b]\x1b\x1bM\x1bM\x1bM\x1bM\x1bM\x1bD\x1bD\x1bD\x1bD\x1bD\x1bE",
        b"\x1b(\x1b)\x1b(0\x1b)0\x0e\x0f\x1b#8\x1b%G\x1b n",
        b"\x1b[3g\t\t\t\x1b[Z\x1b[65535I\x1b[65535Z\x1b[65535b",
        b"\x1b[?6h\x1b[65535;65535H\x1b[4h\xe6\x97\xa5\x1b[?7l\xe6\x97\xa5\xe6\x97\xa5\xe6\x97\xa5",
        b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1c\x1d\x1e\x1f\x7f",
        "\u{301}\u{301}\u{fe0f}\u{200d}".as_bytes(),
    ];
    for &(rows, cols) in &[(1, 1), (1, 2), (2, 1), (3, 7), (24, 80)] {
        for case in cases {
            let mut t = term(rows, cols, case);
            assert_sound(&t);
            t.feed(b"text \xe6\x97\xa5\xe6\x9c\xac\r\n");
            t.screen.resize(cols, rows);
            for other in cases {
                t.feed(other);
            }
            assert_sound(&t);
            let _ = (
                t.screen.text(),
                t.screen.scrollback(),
                t.screen.take_responses(),
            );
        }
    }
}

#[test]
fn c42_arbitrary_bytes_in_any_chunking_keep_the_model_sound() {
    // A fixed pseudo-random stream biased towards escape sequences, fed in uneven pieces with
    // resizes in between.
    let alphabet: &[&[u8]] = &[
        b"\x1b",
        b"[",
        b"]",
        b";",
        b":",
        b"?",
        b">",
        b"!",
        b"$",
        b"0",
        b"1",
        b"9",
        b"65535",
        b"H",
        b"J",
        b"K",
        b"L",
        b"M",
        b"P",
        b"@",
        b"X",
        b"S",
        b"T",
        b"r",
        b"m",
        b"h",
        b"l",
        b"b",
        b"g",
        b"I",
        b"Z",
        b"n",
        b"c",
        b"s",
        b"u",
        b"d",
        b"G",
        b"A",
        b"B",
        b"C",
        b"D",
        b"7",
        b"8",
        b"(",
        b")",
        b"\x07",
        b"\x08",
        b"\t",
        b"\n",
        b"\r",
        b"\x0e",
        b"\x0f",
        b"\x18",
        b"\\",
        b"1049",
        b"1047",
        b"1048",
        b"2004",
        b"6",
        b"4",
        b"38;2;",
        b"48:5:",
        b"133;A",
        b"8;;u",
        b"abc",
        "日".as_bytes(),
        "\u{301}".as_bytes(),
        "👍".as_bytes(),
        b"\xff",
        b"\xe2\x82",
        b" ",
    ];
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut t = Terminal::new(7, 13);
    let mut pending = Vec::new();
    for round in 0..60_000u32 {
        pending.extend_from_slice(alphabet[(next() % alphabet.len() as u64) as usize]);
        if pending.len() > (next() % 23) as usize {
            t.feed(&pending);
            pending.clear();
        }
        if round % 997 == 0 {
            t.screen
                .resize(1 + (next() % 9) as usize, 1 + (next() % 17) as usize);
            assert_sound(&t);
        }
        if round % 4999 == 0 {
            assert_sound(&t);
            let _ = (t.screen.text(), t.screen.take_responses());
        }
    }
    assert_sound(&t);
    assert!(t.screen.scrollback_len() <= 10_000);
}

#[test]
fn c43_over_long_sequences_do_not_grow_the_model() {
    // An operating system command without an end is dropped at 64 KiB; what follows is read
    // normally.
    let mut t = term(3, 20, b"\x1b]2;kept\x07");
    t.feed(b"\x1b]2;");
    for _ in 0..40 {
        t.feed(&[b'x'; 4096]);
    }
    t.feed(b"\x07after");
    assert_eq!(t.screen.title(), "kept", "the over-long title is not taken");
    assert_eq!(t.screen.text(), "after");
    t.feed(b"\x1b]2;");
    for _ in 0..40 {
        t.feed(&[b';'; 4096]);
    }
    t.feed(b"\x1b\\\x1b]2;next\x1b\\");
    assert_eq!(t.screen.title(), "next");
    assert_eq!(t.screen.text(), "after");
    // Bytes between ESC and `]` that the parser skips do not hide the command.
    for skipped in [&b"\x00"[..], b"\x7f", b"\xff", b"\n"] {
        let mut t = term(3, 20, b"\x1b]2;kept\x07\x1b");
        t.feed(skipped);
        t.feed(b"]2;");
        for _ in 0..40 {
            t.feed(&[b'y'; 4096]);
        }
        t.feed(b"\x07");
        assert_eq!(t.screen.title(), "kept", "{skipped:?}");
    }
    // A title is cut at 1 024 characters.
    let mut long = b"\x1b]0;".to_vec();
    long.extend(std::iter::repeat_n(b'T', 5000));
    long.push(7);
    t.feed(&long);
    assert_eq!(t.screen.title().chars().count(), 1024);
    // Combining marks on one cell are limited.
    let mut t = term(1, 4, b"a");
    for _ in 0..10_000 {
        t.feed("\u{301}".as_bytes());
    }
    assert!(cell(&t, 0, 0).combining.chars().count() <= 16);
    // The history is limited.
    let mut t = Terminal::new(2, 4);
    for _ in 0..12_000 {
        t.feed(b"x\r\n");
    }
    assert_eq!(t.screen.scrollback_len(), 10_000);
    // Answers nobody collects are limited.
    let mut t = Terminal::new(2, 4);
    for _ in 0..10_000 {
        t.feed(b"\x1b[6n\x1b[c");
    }
    assert!(t.screen.take_responses().len() <= 4096);
}

#[test]
fn c44_marks_and_links_from_output_are_limited() {
    // Prompt marks repeated on one line keep one prompt.
    let mut t = Terminal::new(3, 20);
    for _ in 0..5000 {
        t.feed(b"\x1b]133;A\x07");
    }
    assert_eq!(t.screen.prompts().len(), 1);
    // A hyperlink address of more than 2 083 bytes is no link.
    let mut t = Terminal::new(3, 20);
    let mut long = b"\x1b]8;;https://example.org/".to_vec();
    long.extend(std::iter::repeat_n(b'a', 3000));
    long.extend_from_slice(b"\x07x\x1b]8;;\x07");
    t.feed(&long);
    assert_eq!(cell(&t, 0, 0).ch, 'x');
    assert_eq!(cell(&t, 0, 0).style.link, 0);
    // Addresses are kept up to 1 MiB in all; later links are plain text.
    let mut t = Terminal::new(3, 20);
    let mut last = 0;
    for n in 0..1200 {
        let uri = format!("https://example.org/{n:04}/{}", "p".repeat(1000));
        t.feed(format!("\r\x1b]8;;{uri}\x07y\x1b]8;;\x07").as_bytes());
        if n == 0 {
            assert_ne!(cell(&t, 0, 0).style.link, 0);
        }
        last = cell(&t, 0, 0).style.link;
    }
    assert_eq!(last, 0);
    // A reported folder of more than 4 096 bytes is not taken.
    let mut t = term(1, 4, b"\x1b]7;file:///ok\x07");
    t.feed(format!("\x1b]7;file:///{}\x07", "d".repeat(5000)).as_bytes());
    assert_eq!(t.screen.reported_dir(), Some("/ok"));
}

// ---- Recorded programs ----------------------------------------------------------------------

#[test]
fn c45_recorded_tput_session() {
    // `tput` with TERM=xterm-256color: keypad mode, alternate screen, positioning, colours, the
    // line-drawing set, erase with a background, delete/insert character, tab stops, save and
    // restore, insert mode, a scroll region and the cursor position query (fixtures/record.sh).
    let mut t = term(24, 80, include_bytes!("fixtures/tput.raw"));
    assert!(t.screen.alternate() && t.screen.app_cursor() && t.screen.app_keypad());
    assert_eq!(
        lines(&t),
        [
            "┌──┐",
            "│  │",
            "",
            "bar",
            "",
            "abXef",
            "   T",
            "back",
            "hello world",
            "         far",
            "",
            "end",
        ]
    );
    assert_eq!(cell(&t, 3, 0).style.bg, Color::Indexed(4));
    assert_eq!(
        cell(&t, 3, 79).style.bg,
        Color::Indexed(4),
        "erased to the end in blue"
    );
    assert_eq!(cell(&t, 5, 0).style.bg, Color::Default);
    assert_eq!(t.screen.cursor(), (11, 3));
    assert_eq!(t.screen.take_responses(), b"\x1b[12;4R");
}

#[test]
fn c46_recorded_alternate_screen_round_trip() {
    let mut t = term(5, 40, b"$ prog\r\n");
    t.feed(include_bytes!("fixtures/alt.raw"));
    assert!(!t.screen.alternate() && !t.screen.app_cursor() && !t.screen.app_keypad());
    assert_eq!(lines(&t), ["$ prog", "after"]);
    assert_eq!(t.screen.scrollback_len(), 0);
}

#[test]
fn c47_recorded_coloured_listing_and_log() {
    let t = term(6, 40, include_bytes!("fixtures/ls.raw"));
    assert_eq!(lines(&t), ["folder", "link", "plain.txt", "run.sh"]);
    let fg = |r: usize| cell(&t, r, 0).style.fg;
    assert_eq!(
        (fg(0), fg(1), fg(2), fg(3)),
        (
            Color::Indexed(4),
            Color::Indexed(5),
            Color::Default,
            Color::Indexed(1)
        )
    );
    let t = term(4, 200, include_bytes!("fixtures/log.raw"));
    let text = lines(&t);
    assert_eq!(text.len(), 3);
    for (r, line) in text.iter().enumerate() {
        assert_eq!(
            cell(&t, r, 0).style.fg,
            Color::Indexed(3),
            "the hash is yellow"
        );
        assert_eq!(cell(&t, r, 7).style.fg, Color::Indexed(3));
        assert_eq!(
            cell(&t, r, 9).style.fg,
            Color::Default,
            "the subject is not"
        );
        assert!(line.ends_with(')'), "{line}");
    }
    // At 40 columns the same log wraps without losing a character.
    let narrow = term(30, 40, include_bytes!("fixtures/log.raw"));
    assert_eq!(
        narrow.screen.text().replace(['\n', ' '], ""),
        text.concat().replace(' ', "")
    );
}

// ---- Keys -----------------------------------------------------------------------------------

/// The capabilities of the terminfo entry `xterm-256color` as recorded with `infocmp -1`.
fn terminfo(name: &str) -> Vec<u8> {
    let keys = include_str!("fixtures/xterm-256color.keys");
    let value = keys
        .lines()
        .find_map(|l| l.trim().strip_prefix(&format!("{name}=")))
        .unwrap_or_else(|| panic!("no capability {name}"))
        .trim_end_matches(',');
    value.replace("\\E", "\x1b").into_bytes()
}

#[test]
fn k01_keys_send_what_terminfo_says() {
    // (capability, key, ctrl, alt, shift). terminfo describes the keys after `smkx`, which turns
    // the application cursor mode on.
    const F: [&str; 12] = [
        "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
    ];
    let mut cases: Vec<(String, &str, bool, bool, bool)> = [
        ("kcuu1", "up", false, false, false),
        ("kcud1", "down", false, false, false),
        ("kcuf1", "right", false, false, false),
        ("kcub1", "left", false, false, false),
        ("khome", "home", false, false, false),
        ("kend", "end", false, false, false),
        ("kich1", "insert", false, false, false),
        ("kdch1", "delete", false, false, false),
        ("knp", "pagedown", false, false, false),
        ("kpp", "pageup", false, false, false),
        ("kcbt", "tab", false, false, true),
        ("kDC", "delete", false, false, true),
        ("kIC", "insert", false, false, true),
        ("kEND", "end", false, false, true),
        ("kHOM", "home", false, false, true),
        ("kLFT", "left", false, false, true),
        ("kRIT", "right", false, false, true),
        ("kNXT", "pagedown", false, false, true),
        ("kPRV", "pageup", false, false, true),
        ("kri", "up", false, false, true),
        ("kind", "down", false, false, true),
    ]
    .into_iter()
    .map(|(cap, key, c, a, s)| (cap.to_owned(), key, c, a, s))
    .collect();
    // kf1–kf12 plain, kf13–24 Shift, kf25–36 Control, kf37–48 Control+Shift, kf49–60 Alt,
    // kf61–63 Alt+Shift.
    for (block, (c, a, s)) in [
        (false, false, false),
        (false, false, true),
        (true, false, false),
        (true, false, true),
        (false, true, false),
        (false, true, true),
    ]
    .into_iter()
    .enumerate()
    {
        for (i, key) in F.iter().enumerate() {
            let n = block * 12 + i + 1;
            if n <= 63 {
                cases.push((format!("kf{n}"), *key, c, a, s));
            }
        }
    }
    assert_eq!(cases.len(), 21 + 63);
    for (cap, key, ctrl, alt, shift) in cases {
        assert_eq!(
            key_bytes(key, ctrl, alt, shift, true),
            Some(terminfo(&cap)),
            "{cap} = {key} ctrl={ctrl} alt={alt} shift={shift}"
        );
    }
}

#[test]
fn k02_arrows_home_and_end_in_both_cursor_modes() {
    let k = |key, c, a, s, app| {
        key_bytes(key, c, a, s, app).map(|b| String::from_utf8_lossy(&b).into_owned())
    };
    for (key, letter) in [
        ("up", 'A'),
        ("down", 'B'),
        ("right", 'C'),
        ("left", 'D'),
        ("home", 'H'),
        ("end", 'F'),
    ] {
        assert_eq!(
            k(key, false, false, false, false),
            Some(format!("\x1b[{letter}"))
        );
        assert_eq!(
            k(key, false, false, false, true),
            Some(format!("\x1bO{letter}"))
        );
        // With a modifier the form is the same in both modes: 2 Shift, 3 Alt, 5 Control, sums.
        for app in [false, true] {
            assert_eq!(
                k(key, false, false, true, app),
                Some(format!("\x1b[1;2{letter}"))
            );
            assert_eq!(
                k(key, false, true, false, app),
                Some(format!("\x1b[1;3{letter}"))
            );
            assert_eq!(
                k(key, false, true, true, app),
                Some(format!("\x1b[1;4{letter}"))
            );
            assert_eq!(
                k(key, true, false, false, app),
                Some(format!("\x1b[1;5{letter}"))
            );
            assert_eq!(
                k(key, true, false, true, app),
                Some(format!("\x1b[1;6{letter}"))
            );
            assert_eq!(
                k(key, true, true, false, app),
                Some(format!("\x1b[1;7{letter}"))
            );
            assert_eq!(
                k(key, true, true, true, app),
                Some(format!("\x1b[1;8{letter}"))
            );
        }
    }
}

#[test]
fn k03_editing_keys_with_modifiers() {
    let k = |key, c, a, s| {
        key_bytes(key, c, a, s, false).map(|b| String::from_utf8_lossy(&b).into_owned())
    };
    assert_eq!(
        k("delete", true, false, false).as_deref(),
        Some("\x1b[3;5~")
    );
    assert_eq!(
        k("delete", false, true, false).as_deref(),
        Some("\x1b[3;3~")
    );
    assert_eq!(k("pageup", true, false, true).as_deref(), Some("\x1b[5;6~"));
    assert_eq!(k("insert", false, false, false).as_deref(), Some("\x1b[2~"));
    assert_eq!(k("f1", false, false, false).as_deref(), Some("\x1bOP"));
    assert_eq!(k("f4", true, false, false).as_deref(), Some("\x1b[1;5S"));
    assert_eq!(k("f12", false, true, false).as_deref(), Some("\x1b[24;3~"));
}

#[test]
fn k04_enter_backspace_tab_and_escape() {
    let k = |key, c, a, s| key_bytes(key, c, a, s, false);
    assert_eq!(k("enter", false, false, false), Some(b"\r".to_vec()));
    // Backspace sends DEL, the erase character of the PTY (`stty erase ^?`); the terminfo entry
    // of this system names ^H as kbs, which Control+Backspace sends.
    assert_eq!(k("backspace", false, false, false), Some(vec![0x7f]));
    assert_eq!(k("backspace", true, false, false), Some(vec![0x08]));
    assert_eq!(terminfo("kbs"), b"^H");
    assert_eq!(k("tab", false, false, false), Some(b"\t".to_vec()));
    assert_eq!(k("escape", false, false, false), Some(b"\x1b".to_vec()));
    // Alt is Meta: an ESC in front.
    assert_eq!(k("enter", false, true, false), Some(b"\x1b\r".to_vec()));
    assert_eq!(
        k("backspace", false, true, false),
        Some(b"\x1b\x7f".to_vec())
    );
    // Without modifyOtherKeys (not in SPEC-0062) Control and Shift do not change these keys.
    assert_eq!(k("enter", true, false, false), Some(b"\r".to_vec()));
    assert_eq!(k("enter", false, false, true), Some(b"\r".to_vec()));
    assert_eq!(k("tab", true, false, false), Some(b"\t".to_vec()));
}

#[test]
fn k05_control_combinations() {
    let k = |key: &str, shift| key_bytes(key, true, false, shift, false);
    for (i, letter) in ('a'..='z').enumerate() {
        let byte = Some(vec![i as u8 + 1]);
        assert_eq!(k(&letter.to_string(), false), byte);
        assert_eq!(
            k(&letter.to_ascii_uppercase().to_string(), true),
            byte,
            "Shift changes nothing"
        );
    }
    for (key, byte) in [
        (" ", 0x00),
        ("@", 0x00),
        ("2", 0x00),
        ("[", 0x1b),
        ("3", 0x1b),
        ("\\", 0x1c),
        ("4", 0x1c),
        ("]", 0x1d),
        ("5", 0x1d),
        ("^", 0x1e),
        ("6", 0x1e),
        ("_", 0x1f),
        ("-", 0x1f),
        ("7", 0x1f),
        ("/", 0x1f),
        ("8", 0x7f),
        ("?", 0x7f),
    ] {
        assert_eq!(k(key, false), Some(vec![byte]), "Control+{key}");
    }
    assert_eq!(k("1", false), None);
    assert_eq!(k("ä", false), None);
}

#[test]
fn k06_alt_is_meta() {
    let k = |key, c, s| key_bytes(key, c, true, s, false);
    assert_eq!(k("b", false, false), Some(b"\x1bb".to_vec()));
    assert_eq!(k("b", false, true), Some(b"\x1bB".to_vec()));
    assert_eq!(k(".", false, false), Some(b"\x1b.".to_vec()));
    assert_eq!(k("c", true, false), Some(b"\x1b\x03".to_vec()));
    assert_eq!(k("ü", false, false), Some("\x1bü".as_bytes().to_vec()));
    // Plain text keys are typed through the input method.
    assert_eq!(key_bytes("b", false, false, false, false), None);
    assert_eq!(key_bytes("b", false, false, true, false), None);
    assert_eq!(key_bytes("unknown-key", true, true, true, false), None);
}
