//! SPEC-0005 v3 AK-17: properties of the splitter over generated scripts, with a fixed
//! pseudo-random walk (xorshift, no dependency) so every run checks the same scripts.

use ferris_idea_database::{Dialect, split_statements};

/// A small generator without a dependency, the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

/// Pieces of a statement that hold a `;` or another terminator only where the dialect protects it.
fn fragments(dialect: Dialect) -> Vec<&'static str> {
    let mut f = vec![
        "1",
        "x",
        "'a;b'",
        "'it''s'",
        "(3)",
        "/* ; */ 2",
        "é",
        "'日本;'",
    ];
    match dialect {
        Dialect::PostgreSql | Dialect::DuckDb => f.extend([
            "$$a;b$$",
            "$t$ ; $t$",
            r"E'x\';y'",
            "\"q;id\"",
            "$1",
            "/* a /* ; */ b */",
        ]),
        Dialect::MySql | Dialect::MariaDb => f.extend(["`b;t`", r"'c\';d'", "5--1", "\"s;t\""]),
        Dialect::SqlServer => f.extend([
            "[a;b]",
            "[x]]y;]",
            "N'x;y'",
            "\"q;id\"",
            "/* a /* ; */ b */",
        ]),
        Dialect::Sqlite => f.extend(["[a;b]", "\"q;id\""]),
        Dialect::Oracle => f.extend(["q'[a;b]'", "nq'!x;y!'", "\"q;id\""]),
    }
    f
}

/// Text between statements that holds no statement.
fn filler(dialect: Dialect) -> Vec<&'static str> {
    let mut f = vec!["", " ", "\n", "\n\n", "-- c; x\n", "/* ; */\n"];
    if dialect.mysql_like() {
        f.push("# c; x\n");
    }
    f
}

const DIALECTS: [Dialect; 7] = Dialect::ALL;

/// Statements joined with their terminators, comments and blank lines come back exactly.
#[test]
fn generated_scripts_split_back_into_their_statements() {
    for (n, dialect) in DIALECTS.into_iter().enumerate() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ (n as u64 + 1));
        let pieces = fragments(dialect);
        let fill = filler(dialect);
        for round in 0..400 {
            let count = 1 + rng.below(5);
            let mut script = String::new();
            let mut expected = Vec::new();
            // The `mysql` client's delimiter, switched now and then (behaviour 16).
            let mut delimiter = ";";
            for _ in 0..count {
                script.push_str(rng.pick(&fill));
                if dialect.mysql_like() && rng.below(4) == 0 {
                    delimiter = if delimiter == ";" { "//" } else { ";" };
                    script.push_str(&format!("\nDELIMITER {delimiter}\n"));
                }
                let words = 1 + rng.below(4);
                let mut statement = "select".to_owned();
                for _ in 0..words {
                    statement.push(' ');
                    statement.push_str(rng.pick(&pieces));
                }
                script.push_str(&statement);
                expected.push(statement);
                let end = match dialect {
                    Dialect::SqlServer if rng.below(3) == 0 => "\nGO\n",
                    Dialect::Oracle if rng.below(3) == 0 => "\n/\n",
                    _ => delimiter,
                };
                script.push_str(end);
                script.push_str(rng.pick(&fill));
            }
            let got: Vec<String> = split_statements(&script, dialect)
                .into_iter()
                .map(|s| s.text)
                .collect();
            assert_eq!(got, expected, "{dialect:?} round {round}: {script:?}");
        }
    }
}

/// Any text keeps behaviour 17: ordered, apart, on character boundaries, trimmed, never empty.
#[test]
fn any_text_splits_into_ordered_trimmed_slices() {
    const ALPHABET: &[&str] = &[
        "'",
        "\"",
        "`",
        "[",
        "]",
        "$",
        "$$",
        "$a$",
        ";",
        "-",
        "--",
        "#",
        "/",
        "*",
        "/*",
        "*/",
        "\n",
        " ",
        "GO",
        "go\n",
        "\nGO\n",
        "\nDELIMITER //\n",
        "\nDELIMITER ;\n",
        "//",
        "E'",
        "q'[",
        "\\",
        "é",
        "日本",
        "x",
        "begin",
        "end",
        "create trigger t ",
        "create procedure p ",
        "declare ",
        "case",
        "if",
        "(",
        ")",
        ",",
        "N'",
        "\n/\n",
    ];
    for (n, dialect) in DIALECTS.into_iter().enumerate() {
        let mut rng = Rng(0x2545_f491_4f6c_dd1d ^ (n as u64 + 7));
        for round in 0..400 {
            let len = rng.below(40);
            let script: String = (0..len).map(|_| rng.pick(ALPHABET)).collect();
            let statements = split_statements(&script, dialect);
            let mut last_end = 0;
            for s in &statements {
                assert!(
                    s.range.start >= last_end,
                    "{dialect:?} {round}: order {script:?}"
                );
                assert!(
                    s.range.start < s.range.end,
                    "{dialect:?} {round}: empty {script:?}"
                );
                assert!(
                    script.is_char_boundary(s.range.start) && script.is_char_boundary(s.range.end)
                );
                assert_eq!(&script[s.range.clone()], s.text);
                assert_eq!(s.text.trim(), s.text);
                last_end = s.range.end;
            }
        }
    }
}
