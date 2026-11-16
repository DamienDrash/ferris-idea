//! Which files a session opened for an agent may reach (`SPEC-0012` v3 behaviour 15).
//!
//! A file database runs inside IDECraft with the rights of the user. An agent that may run SQL on
//! one must not reach other files through it: for agents the rules of `read_file` hold. SQLite
//! statements are checked here before they run; DuckDB closes its own file access (see
//! `duckdb.rs`).

use std::path::Path;

use crate::Dialect;
use crate::script::code_words;

/// A piece of a statement, as far as the file rules need it.
#[derive(Debug, PartialEq, Eq)]
enum Token {
    /// A keyword or name, lower-cased.
    Word(String),
    /// A quoted text with its quotes removed and doubled quotes joined.
    Quoted(String),
    /// Anything else, one character.
    Other(char),
}

/// The first `limit` tokens of `sql`, comments left out.
fn tokens(sql: &str, limit: usize) -> Vec<Token> {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() && out.len() < limit {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(chars.len());
        } else if c == '\'' || c == '"' {
            let mut text = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => break,
                    Some(&q) if q == c => {
                        if chars.get(i + 1) == Some(&c) {
                            text.push(c);
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    }
                    Some(&other) => {
                        text.push(other);
                        i += 1;
                    }
                }
            }
            out.push(Token::Quoted(text));
        } else if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Token::Word(
                chars[start..i].iter().collect::<String>().to_lowercase(),
            ));
        } else {
            out.push(Token::Other(c));
            i += 1;
        }
    }
    out
}

/// Why an agent's session on a file database must not run `statement`, if it reaches a file
/// outside `root`: `None` when the statement opens no file or only one inside the workspace.
///
/// SQLite: `ATTACH` is allowed with a quoted name of an existing file inside `root` (or
/// `:memory:` or the empty name); a name that is an expression, a `file:` URI, a relative name or
/// a file outside is refused, and so is `VACUUM INTO`. Other dialects open no file through SQL
/// here: DuckDB refuses by its own setting, servers read files on their own machine.
pub fn refused_file_access(statement: &str, dialect: Dialect, root: &Path) -> Option<String> {
    if dialect != Dialect::Sqlite {
        return None;
    }
    let words = code_words(statement, dialect);
    let has = |w: &str| words.iter().any(|x| x == w);
    if has("vacuum") && has("into") {
        return Some(
            "VACUUM INTO writes a database file; agents cannot do that through a connection".into(),
        );
    }
    if !has("attach") {
        return None;
    }
    let tokens = tokens(statement, 4);
    let mut rest = tokens.iter();
    if rest.next() != Some(&Token::Word("attach".into())) {
        return Some(
            "ATTACH is allowed for agents only as a statement of its own, with a quoted file name"
                .into(),
        );
    }
    let mut next = rest.next();
    if next == Some(&Token::Word("database".into())) {
        next = rest.next();
    }
    let (Some(Token::Quoted(name)), Some(Token::Word(as_word))) = (next, rest.next()) else {
        return Some(
            "ATTACH is allowed for agents only with a quoted file name followed by AS; an expression cannot be checked"
                .into(),
        );
    };
    if as_word != "as" {
        return Some(
            "ATTACH is allowed for agents only with a quoted file name followed by AS; an expression cannot be checked"
                .into(),
        );
    }
    if name.is_empty() || name == ":memory:" {
        return None;
    }
    if name.to_ascii_lowercase().starts_with("file:") {
        return Some(format!(
            "ATTACH of the URI `{name}` is refused for agents; name the file by its path"
        ));
    }
    let file = Path::new(name);
    if !file.is_absolute() {
        return Some(format!(
            "ATTACH of `{name}` is refused for agents: give the full path of a file inside the workspace"
        ));
    }
    let outside = || {
        Some(format!(
            "`{name}` is not inside the workspace; an agent's session attaches only existing files inside {}",
            root.display()
        ))
    };
    let (Ok(file), Ok(root)) = (file.canonicalize(), root.canonicalize()) else {
        return outside();
    };
    if !file.is_file() || !file.starts_with(&root) {
        return outside();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("idecraft-agent-files-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ws/data")).unwrap();
        std::fs::write(dir.join("ws/data/inside.db"), "").unwrap();
        std::fs::write(dir.join("outside.db"), "").unwrap();
        dir.canonicalize().unwrap()
    }

    /// SPEC-0012 v3 AK-13: what an agent's SQLite session may attach.
    #[test]
    fn sqlite_attaches_only_files_inside_the_workspace() {
        let dir = scratch("attach");
        let root = dir.join("ws");
        let check = |sql: &str| refused_file_access(sql, Dialect::Sqlite, &root);
        let inside = root.join("data/inside.db");
        let outside = dir.join("outside.db");
        assert_eq!(check("select 1"), None);
        assert_eq!(check("select 'attach database'"), None);
        assert_eq!(
            check(&format!("attach database '{}' as a", inside.display())),
            None
        );
        assert_eq!(
            check(&format!("ATTACH \"{}\" AS a", inside.display())),
            None
        );
        assert_eq!(check("attach ':memory:' as m"), None);
        assert_eq!(check("attach '' as tmp"), None);
        for refused in [
            format!("attach database '{}' as a", outside.display()),
            format!(
                "/* c */ attach -- x\n database '{}' as a",
                outside.display()
            ),
            format!("attach '{}/../outside.db' as a", root.display()),
            format!("attach '{}/data/missing.db' as a", root.display()),
            "attach 'data/inside.db' as a".to_owned(),
            format!("attach 'file:{}?mode=ro' as a", inside.display()),
            format!("attach '{}' || '' as a", inside.display()),
            "attach other as a".to_owned(),
            format!("explain attach '{}' as a", inside.display()),
            format!("vacuum into '{}/data/copy.db'", root.display()),
            "vacuum main into 'x.db'".to_owned(),
        ] {
            assert!(check(&refused).is_some(), "not refused: {refused}");
        }
        // A link inside the workspace that leads outside is outside.
        #[cfg(unix)]
        {
            let link = root.join("data/link.db");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(check(&format!("attach '{}' as a", link.display())).is_some());
        }
        // Other dialects are not judged here.
        assert_eq!(
            refused_file_access("attach 'x' as a", Dialect::PostgreSql, &root),
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
