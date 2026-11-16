//! SPEC-0329 AK-4: the real `ruff` and `black`. `IDECRAFT_TEST_PYTOOLS` names a folder that
//! holds both (the `bin` folder of a virtual environment with `pip install ruff black`); without
//! it they are looked up on `PATH`. Skipped when they are not installed.

use std::path::{Path, PathBuf};

use idecraft_languages::insight::Range;
use idecraft_languages::python_tools::{
    PythonTool, ToolKind, ToolSearch, find, format, plan, ruff_imports,
};

const MESSY: &str = "import os, sys\nfrom app.prices import Item\ndef messy( a,b = 2,*args,**kw ):\n  x=a+b\n  y = [ 1,2 , 3 ]\n  d = {'k':x , \"v\" : y}\n  if x>3 : return x\n  return d\nclass Shape :\n  sides=4\n  def area( self ):\n    return 0\n  def name(self) :\n    return \"shape\"\nlam = lambda q:q*2\n";

const FORMATTED: &str = "import os, sys\nfrom app.prices import Item\n\n\ndef messy(a, b=2, *args, **kw):\n    x = a + b\n    y = [1, 2, 3]\n    d = {\"k\": x, \"v\": y}\n    if x > 3:\n        return x\n    return d\n\n\nclass Shape:\n    sides = 4\n\n    def area(self):\n        return 0\n\n    def name(self):\n        return \"shape\"\n\n\nlam = lambda q: q * 2\n";

/// A scratch project and the tool of `kind`, or `None` (with a note) when it is not installed.
fn project(name: &str, kind: ToolKind) -> Option<(PathBuf, PythonTool)> {
    let dir =
        std::env::temp_dir().join(format!("idecraft-pytools-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("app")).unwrap();
    let dir = dir.canonicalize().unwrap();
    let search = ToolSearch {
        root: dir.clone(),
        environment: std::env::var_os("IDECRAFT_TEST_PYTOOLS").map(PathBuf::from),
        path_var: std::env::var_os("PATH"),
        trusted: true,
    };
    let Some(tool) = find(&search, &[kind]) else {
        eprintln!(
            "skipped: no {} (set IDECRAFT_TEST_PYTOOLS to a folder with ruff and black)",
            kind.program()
        );
        let _ = std::fs::remove_dir_all(&dir);
        return None;
    };
    Some((dir, tool))
}

fn no_files_written(dir: &Path) {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names.retain(|n| n != "pyproject.toml");
    assert_eq!(names, ["app"], "the tool wrote into the project");
}

#[test]
fn ruff_formats_unsaved_text_with_the_project_configuration() {
    let Some((dir, ruff)) = project("ruff", ToolKind::Ruff) else {
        return;
    };
    // The file does not exist: only the text counts.
    let file = dir.join("fmt.py");
    let formatted = format(&ruff, &file, MESSY, None).unwrap();
    assert_eq!(formatted, FORMATTED);
    assert_eq!(format(&ruff, &file, &formatted, None).unwrap(), formatted);
    let p = plan(&file, MESSY, &formatted);
    assert!(!p.whitespace_only, "quotes change too");
    assert_eq!(p.edits.edits.len(), 1);
    no_files_written(&dir);

    // Only the function (0-based lines 2 to 8): the class stays as written.
    let range = Range {
        start_line: 2,
        start_character: 0,
        end_line: 8,
        end_character: 0,
    };
    let part = format(&ruff, &file, MESSY, Some(&range)).unwrap();
    assert!(part.starts_with("import os, sys\nfrom app.prices import Item\ndef messy(a, b=2, *args, **kw):\n    x = a + b\n"), "{part}");
    assert!(part.ends_with("class Shape :\n  sides=4\n  def area( self ):\n    return 0\n  def name(self) :\n    return \"shape\"\nlam = lambda q:q*2\n"), "{part}");

    // The project's configuration applies: single quotes and a short line.
    std::fs::write(
        dir.join("pyproject.toml"),
        "[tool.ruff]\nline-length = 30\n\n[tool.ruff.format]\nquote-style = \"single\"\n",
    )
    .unwrap();
    let configured = format(
        &ruff,
        &file,
        "d = {\"k\": 1}\nresult = compute(first_argument, second_argument)\n",
        None,
    )
    .unwrap();
    assert!(
        configured.starts_with("d = {'k': 1}\nresult = compute(\n    first_argument,"),
        "{configured}"
    );

    // A syntax error is a failure that names the tool; nothing is returned.
    let broken = format(&ruff, &file, "def f(:\n", None).unwrap_err();
    assert!(broken.starts_with("ruff: "), "{broken}");
    no_files_written(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ruff_removes_unused_imports_and_sorts() {
    let Some((dir, ruff)) = project("imports", ToolKind::Ruff) else {
        return;
    };
    let file = dir.join("fmt.py");
    let done = ruff_imports(&ruff, &file, MESSY).unwrap();
    assert_eq!(done.removed, ["os", "sys", "Item"]);
    assert!(
        done.text.starts_with("def messy( a,b = 2,*args,**kw ):\n"),
        "{}",
        done.text
    );
    assert_eq!((done.by, done.sorted), ("ruff", true));

    let unsorted = "import sys\nimport json\nimport os\n\nprint(sys.argv, os.sep)\n";
    let done = ruff_imports(&ruff, &file, unsorted).unwrap();
    assert_eq!(
        done.text,
        "import os\nimport sys\n\nprint(sys.argv, os.sep)\n"
    );
    assert_eq!(done.removed, ["json"]);
    // Nothing to do: the text comes back as it is.
    let again = ruff_imports(&ruff, &file, &done.text).unwrap();
    assert_eq!(again.text, done.text);
    assert!(again.removed.is_empty());
    // ruff leaves the unused import of a package's `__init__.py`.
    let init = "from .prices import Item\n";
    let kept = ruff_imports(&ruff, &dir.join("app/__init__.py"), init).unwrap();
    assert_eq!(kept.text, init);
    no_files_written(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn black_formats_when_it_is_the_formatter() {
    let Some((dir, black)) = project("black", ToolKind::Black) else {
        return;
    };
    let file = dir.join("fmt.py");
    assert_eq!(format(&black, &file, MESSY, None).unwrap(), FORMATTED);
    let range = Range {
        start_line: 2,
        start_character: 0,
        end_line: 8,
        end_character: 0,
    };
    let part = format(&black, &file, MESSY, Some(&range)).unwrap();
    assert!(
        part.contains("def messy(a, b=2, *args, **kw):\n    x = a + b\n"),
        "{part}"
    );
    assert!(
        part.ends_with("  def name(self) :\n    return \"shape\"\nlam = lambda q:q*2\n"),
        "{part}"
    );
    let broken = format(&black, &file, "def f(:\n", None).unwrap_err();
    assert!(broken.starts_with("black: "), "{broken}");
    no_files_written(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}
