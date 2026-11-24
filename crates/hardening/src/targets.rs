//! The fuzz targets: one function per family of untrusted input. Each splits its bytes into the
//! parts the reader takes, calls it, and checks the invariants the reader promises (ranges inside
//! the text, undo restores the text, the server still answers). A panic anywhere is a finding.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// One fuzz target.
pub struct Target {
    /// Name, as `cargo fuzz run <name>` and the driver take it.
    pub name: &'static str,
    /// What it feeds.
    pub about: &'static str,
    /// The function under test.
    pub run: fn(&[u8]),
    /// Valid inputs to start from.
    pub seeds: &'static [&'static [u8]],
    /// Tokens the driver splices in.
    pub dict: &'static [&'static [u8]],
}

/// Every target.
pub const TARGETS: &[Target] = &[
    Target {
        name: "lsp_frame",
        about: "LSP Content-Length framing and the replies Ferris IDEA reads (hover, completion, …)",
        run: lsp_frame,
        seeds: &[
            b"Content-Length: 2\r\n\r\n{}",
            b"Content-Length: 52\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"contents\":\"x\"}}",
        ],
        dict: &[
            b"Content-Length: ",
            b"\r\n\r\n",
            b"{\"result\":",
            b"\"range\":{\"start\":{\"line\":0,\"character\":1},\"end\":{\"line\":0,\"character\":2}}",
            b"\"uri\":\"file:///a.rs\"",
            b"\"items\":[{\"label\":\"x\"}]",
            b"\"signatures\":[{\"label\":\"f(a, b)\",\"parameters\":[{\"label\":[2,3]}]}]",
            b"99999999999",
        ],
    },
    Target {
        name: "dap_frame",
        about: "DAP Content-Length framing",
        run: dap_frame,
        seeds: &[b"Content-Length: 38\r\n\r\n{\"seq\":1,\"type\":\"event\",\"event\":\"x\"}"],
        dict: &[b"Content-Length: ", b"\r\n\r\n", b"content-length:", b"99999999999"],
    },
    Target {
        name: "mcp_jsonrpc",
        about: "MCP JSON-RPC lines and read-only tool calls against a live server",
        run: mcp_jsonrpc,
        seeds: &[
            b"\x00{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"tools/list\"}",
            b"\x01\x00{\"path\":\"src/app.py\",\"start_line\":2}",
            b"\x01\x05{\"query\":\"a\"}",
        ],
        dict: &[
            b"{\"jsonrpc\":\"2.0\",",
            b"\"id\":",
            b"\"method\":\"tools/call\"",
            b"\"params\":{\"name\":\"read_file\",\"arguments\":{}}",
            b"\"path\":\"",
            b"\"text\":\"",
            b"\"query\":\"",
            b"../",
            b"null",
            b"18446744073709551615",
        ],
    },
    Target {
        name: "sql_split",
        about: "SQL statement splitter of every dialect (SPEC-0005)",
        run: sql_split,
        seeds: &[
            b"select 1; select 'a;b'; -- c;\nselect \"x;\" from t;",
            b"DELIMITER //\ncreate procedure p() begin select 1; end//\nDELIMITER ;\n",
            b"create function f() returns int as $$ begin return 1; end $$ language plpgsql;",
            b"select 1\nGO\nselect 2\ngo 3\n",
            b"begin\n  null;\nend;\n/\nselect 1 from dual;",
        ],
        dict: &[
            b";", b"'", b"\"", b"`", b"[", b"]", b"$$", b"$tag$", b"--", b"#", b"/*", b"*/",
            b"\\", b"DELIMITER ", b"\nGO\n", b"\n/\n", b"BEGIN", b"END", b"CASE", b"CREATE ",
            b"PROCEDURE ", b"FUNCTION ", b"TRIGGER ", b"PACKAGE ", b"E'", b"N'", b"q'[",
        ],
    },
    Target {
        name: "terminal_vte",
        about: "terminal escape sequences into the screen model, search and links",
        run: terminal_vte,
        seeds: &[
            b"\x18\x50\x04\x00hello\r\nworld\x1b[2J\x1b[H\x1b[31mred\x1b[0m",
            b"\x05\x0a\x02\x01\x1b[?1049h\x1b[5;20r\x1b[10L\x1b[3M\x1b]0;title\x07\x1b]8;;http://x\x1b\\a\x1b]8;;\x1b\\",
            b"\x18\x50\x10\x00\x1b]133;A\x07$ \x1b]133;B\x07ls\x1b]133;C\x07\r\nout\r\n\x1b]133;D;0\x07",
        ],
        dict: &[
            b"\x1b[", b"\x1b]", b"\x1bP", b"\x07", b"\x1b\\", b"\x1b[?", b"h", b"l", b"m", b"H",
            b"J", b"K", b"L", b"M", b"P", b"@", b"r", b"S", b"T", b"X", b"b", b"d", b"G", b";",
            b"65535", b"9999", b"1049", b"1000", b"1006", b"2004", b"38;2;", b"48;5;", b"133;",
            b"8;;", b"7;file://", b"\r\n", b"\t", b"\x08", b"\xe2\x80\x8b", b"\xf0\x9f\x98\x80",
            b"\xe4\xb8\xad",
        ],
    },
    Target {
        name: "text_buffer",
        about: "text buffer edits, undo groups, redo, reload and positions against a model (SPEC-0003)",
        run: text_buffer,
        seeds: &[b"\x00\x00a\x01\x00\x02\x00\x03\x05\x02bc\x04\x05\x06"],
        dict: &[b"\r\n", b"\n", b"\xc3\xa9", b"\xf0\x9f\x98\x80", b"\r"],
    },
    Target {
        name: "diff_merge",
        about: "line diff, side-by-side rows and the three-way merge",
        run: diff_merge,
        seeds: &[b"a\nb\nc\n\x00a\nB\nc\n\x00a\nb\nC\n", b"x\n\x00y\n\x00z\n"],
        dict: &[b"\n", b"\r\n", b"<<<<<<< ", b"=======", b">>>>>>> ", b"|||||||", b"\x00"],
    },
    Target {
        name: "plugin_module",
        about: "arbitrary WebAssembly as a plugin module (SPEC-0104)",
        run: plugin_module,
        seeds: &[b"\x00asm\x01\x00\x00\x00"],
        dict: &[b"\x00asm\x01\x00\x00\x00", b"memory", b"ferris_idea_manifest", b"ferris_idea_alloc"],
    },
    Target {
        name: "plugin_answers",
        about: "what a plugin answers: manifests, edits, findings, completions, rows, file types",
        run: plugin_answers,
        seeds: &[
            b"\x01{\"edits\":[{\"start\":0,\"end\":2,\"text\":\"x\"}],\"select\":[1,1]}",
            b"\x02{\"findings\":[{\"start\":0,\"end\":1,\"message\":\"m\",\"fix\":{\"title\":\"t\",\"edits\":[]}}]}",
            b"\x03{\"items\":[{\"label\":\"x\"}]}",
            b"\x04{\"text\":\"a\\nb\"}",
            b"\x05{\"rows\":[{\"text\":\"r\",\"path\":\"a\"}]}",
            b"\x06{\"name\":\"T\",\"extensions\":[\"t\"],\"line_comment\":\"#\",\"block_comment\":[\"(*\",\"*)\"],\"keywords\":[\"if\"]}\x00if x # c\n(* b *) \"s\" 1.5",
            b"\x00{\"name\":\"p\",\"actions\":[{\"id\":\"a\",\"title\":\"A\"}]}",
        ],
        dict: &[
            b"\"start\":", b"\"end\":", b"\"text\":\"", b"\"edits\":[", b"\"findings\":[",
            b"\"items\":[", b"\"rows\":[", b"\"keywords\":[", b"\"case_sensitive\":false",
            b"\"file_types\":[", b"18446744073709551615", b"4294967295", b"\\u0000",
        ],
    },
    Target {
        name: "keymap",
        about: "keymap XML and keystroke text",
        run: keymap,
        seeds: &[
            b"<keymap version=\"1\" name=\"k\" parent=\"$default\"><action id=\"A\"><keyboard-shortcut first-keystroke=\"control shift F\" second-keystroke=\"alt X\"/><mouse-shortcut keystroke=\"button1 doubleClick\"/></action></keymap>",
        ],
        dict: &[
            b"<action id=\"", b"<keyboard-shortcut first-keystroke=\"", b"second-keystroke=\"",
            b"control ", b"shift ", b"alt ", b"meta ", b"\"/>", b"</action>", b"&amp;", b"&#",
        ],
    },
    Target {
        name: "theme",
        about: "colour-scheme XML and UI theme JSON",
        run: theme,
        seeds: &[
            b"\x00<scheme name=\"s\" parent_scheme=\"Darcula\"><colors><option name=\"CARET_ROW_COLOR\" value=\"323232\"/></colors><attributes><option name=\"KEYWORD\"><value><option name=\"FOREGROUND\" value=\"cc7832\"/><option name=\"FONT_TYPE\" value=\"1\"/></value></option></attributes></scheme>",
            b"\x01{\"name\":\"t\",\"dark\":true,\"parentTheme\":\"x\",\"ui\":{\"*\":{\"background\":\"#2b2b2b\"},\"Button\":{\"arc\":4,\"padding\":\"1,2,3,4\"}}}",
        ],
        dict: &[b"<option name=\"", b"value=\"", b"#", b"ffffffff", b"\"ui\":{", b"\"colors\":{"],
    },
    Target {
        name: "settings",
        about: "settings TOML, run configurations, layouts, breakpoints, .editorconfig, grants, connections",
        run: settings,
        seeds: &[
            b"\x00[editor]\nfont = \"x\"\nfont_size = 13\n",
            b"\x01[editor]\ntab_size = 4\n[terminal]\nscrollback = 5\n",
            b"\x02[[run]]\nname = \"a\"\ncommand = [\"echo\"]\n",
            b"\x03{\"windows\":[{\"id\":\"project\",\"side\":\"left\"}]}",
            b"\x04src/a.rs:1,2,3\n",
            b"\x05root = true\n[*.{rs,toml}]\nindent_style = tab\n[{a,b}/**]\nindent_size = 2\n",
            b"\x06[[workspace]]\npath = \"/\"\nallow = [\"run\"]\n",
            b"\x08[[connection]]\nname = \"x\"\ndialect = \"postgresql\"\n",
        ],
        dict: &[
            b"[", b"]", b" = ", b"\"", b"{", b"}", b"**", b"*", b"?", b"..", b"\n", b"[[", b"]]",
        ],
    },
    Target {
        name: "idea_project",
        about: "`.idea` XML: misc, modules, module files, libraries, VCS mappings",
        run: idea_project,
        seeds: &[
            b"p\x00<project><component name=\"ProjectRootManager\" project-jdk-name=\"17\" languageLevel=\"JDK_17\"><output url=\"file://$PROJECT_DIR$/out\"/></component></project>\x00<project><component name=\"ProjectModuleManager\"><modules><module filepath=\"$PROJECT_DIR$/m.iml\"/></modules></component></project>\x00<project><component name=\"VcsDirectoryMappings\"><mapping directory=\"\" vcs=\"Git\"/></component></project>\x00<component name=\"libraryTable\"><library name=\"l\"><CLASSES><root url=\"jar://x.jar!/\"/></CLASSES></library></component>\x00<module><component name=\"NewModuleRootManager\"><content url=\"file://$MODULE_DIR$\"><sourceFolder url=\"file://$MODULE_DIR$/src\" isTestSource=\"false\"/></content><orderEntry type=\"library\" name=\"l\" level=\"project\"/></component></module>",
        ],
        dict: &[
            b"$PROJECT_DIR$", b"$MODULE_DIR$", b"file://", b"jar://", b"../", b"<component name=\"",
            b"url=\"", b"/>", b"&lt;", b"<!--", b"<![CDATA[",
        ],
    },
    Target {
        name: "project_model",
        about: "build files: Cargo, npm, pyproject, requirements, Maven, Gradle and lock files",
        run: project_model,
        seeds: &[
            b"[package]\nname = \"a\"\n[dependencies]\nserde = \"1\"\n[workspace]\nmembers = [\"b\"]\n\x00{\"name\":\"n\",\"dependencies\":{\"x\":\"^1\"},\"workspaces\":[\"p\"]}\x00[project]\nname = \"p\"\ndependencies = [\"requests>=2\"]\n\x00flask==2\n-r other.txt\n\x00<project><artifactId>a</artifactId><modules><module>b</module></modules><dependencies><dependency><groupId>g</groupId><artifactId>x</artifactId><version>${v}</version></dependency></dependencies></project>\x00include ':app'\n\x00dependencies { implementation(\"g:a:1\") }\n",
        ],
        dict: &[b"\x00", b"[", b"]", b"\"", b"${", b"../", b"<module>", b"workspace", b"members"],
    },
    Target {
        name: "http_file",
        about: "`.http` request files, variables and URLs",
        run: http_file,
        seeds: &[
            b"### a\nGET http://{{host}}/x?q={{$uuid}}\nAccept: */*\n\n### b\nPOST https://h:8080/p\nContent-Type: application/json\n\n{\"a\": \"{{v}}\"}\n> {% client.test(); %}\n",
        ],
        dict: &[b"{{", b"}}", b"###", b"\n", b"GET ", b"POST ", b"http://", b"https://", b"@", b":", b"$"],
    },
    Target {
        name: "test_report",
        about: "test reports: JUnit XML, TAP, libtest output, Jest JSON",
        run: test_report,
        seeds: &[
            b"<testsuites><testsuite name=\"s\" tests=\"1\"><testcase name=\"t\" classname=\"c\" time=\"0.1\"><failure message=\"m\">x</failure></testcase></testsuite></testsuites>",
            b"TAP version 13\n1..2\nok 1 - a\nnot ok 2 - b # TODO\n  ---\n  message: x\n  ...\n",
            b"running 2 tests\ntest a ... ok\ntest b ... FAILED\n\nfailures:\n\n---- b stdout ----\npanicked\n\ntest result: FAILED. 1 passed; 1 failed;",
            b"{\"testResults\":[{\"name\":\"f\",\"assertionResults\":[{\"fullName\":\"x\",\"status\":\"passed\",\"duration\":3}]}]}",
        ],
        dict: &[b"<testcase ", b"time=\"", b"ok ", b"not ok ", b"test ", b" ... ", b"\"status\":"],
    },
    Target {
        name: "profiler_formats",
        about: "profiler outputs: JFR, sample, perf script, massif, malloc history, tracemalloc, cProfile, heap, ps",
        run: profiler_formats,
        seeds: &[
            b"\x00jdk.ExecutionSample {\n  stackTrace = [\n    a.B.c() line: 1\n  ]\n}\n",
            b"\x01Call graph:\n    10 Thread_1\n    + 5 main  (in a) + 1 [0x1]\n",
            b"\x02prog 1 1.0: cycles:\n\t  1 f (/a)\n\t  2 g (/a)\n\n",
            b"\x03snapshot=0\nmem_heap_B=10\nheap_tree=detailed\nn1: 10 (heap allocation functions)\n n0: 10 0x1: f (a.c:1)\n",
            b"\x05{\"stats\":[{\"size\":10,\"count\":1,\"traceback\":[[\"a.py\",1]]}]}",
            b"\x06{\"functions\":[{\"file\":\"a.py\",\"line\":1,\"name\":\"f\",\"calls\":1,\"total\":0.1,\"cumulative\":0.2,\"callers\":[]}]}",
            b"\x08  PID COMMAND\n  1 /sbin/launchd\n",
        ],
        dict: &[b"\n", b"  ", b"+ ", b"n1:", b"(in ", b"line:", b"[", b"]", b"{", b"}"],
    },
    Target {
        name: "misc_formats",
        about: "smaller readers: plans, dependency trees, specs, properties, dictionaries, ESLint, AI CLI lines",
        run: misc_formats,
        seeds: &[
            b"\x00[{\"Plan\":{\"Node Type\":\"Seq Scan\",\"Total Cost\":1.5,\"Plans\":[]}}]",
            b"\x01-> Table scan on t  (cost=0.35 rows=1)\n    -> Filter: x\n",
            b"\x02g:a:jar:1:compile\n+- g:b:jar:2:compile\n|  \\- g:c:jar:3:runtime\n",
            b"\x03+--- g:a:1\n|    \\--- g:b:1 -> 2 (*)\n",
            b"\x05a.b = c\\\n  d\nx:y\n! c\n",
            b"\x06<component name=\"ProjectDictionaryState\"><dictionary name=\"d\"><words><w>foo</w></words></dictionary></component>",
            b"\x07[{\"filePath\":\"a.js\",\"messages\":[{\"ruleId\":\"r\",\"severity\":2,\"message\":\"m\",\"line\":1,\"column\":1,\"endLine\":1,\"endColumn\":2}]}]",
            b"\x08f(a, b<c, d>, 'x')",
            b"\x09{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}",
        ],
        dict: &[b"->", b"+-", b"\\-", b"|", b"(", b")", b"<", b">", b"\"", b"\\u", b"\\"],
    },
];

/// The target called `name`.
pub fn target(name: &str) -> Option<&'static Target> {
    TARGETS.iter().find(|t| t.name == name)
}

fn text(data: &[u8]) -> String {
    String::from_utf8_lossy(data).into_owned()
}

/// Splits at `0x00` into exactly `n` parts (missing ones empty).
fn parts(data: &[u8], n: usize) -> Vec<String> {
    let mut out: Vec<String> = data.splitn(n, |b| *b == 0).map(text).collect();
    out.resize(n, String::new());
    out
}

/// A clean scratch folder for this thread.
fn scratch() -> PathBuf {
    thread_local!(static N: u64 = {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    });
    let dir = std::env::temp_dir().join(format!(
        "ferris-idea-fuzz-{}-{}",
        std::process::id(),
        N.with(|n| *n)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

fn write(root: &Path, rel: &str, text: &str) -> PathBuf {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("scratch subfolder");
    }
    std::fs::write(&path, text).expect("scratch file");
    path
}

struct Identity;

impl ferris_idea_languages::insight::Converter for Identity {
    fn character(&mut self, _uri: &str, _line: u64, units: u64) -> u64 {
        units
    }
}

/// Runs the readers of language-server replies on one decoded message.
fn lsp_replies(v: &Value) {
    use ferris_idea_languages::insight;
    for r in [v, &v["result"]] {
        let _ = insight::parse_hover(r, "file:///a.rs", &mut Identity);
        let _ = insight::parse_signature_help(r);
        let _ = insight::parse_locations(r, &mut Identity);
        let _ = insight::parse_completion(r, 50);
        let _ = insight::parse_symbols(r, "file:///a.rs", &mut Identity);
    }
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

/// LSP framing: messages until the end or the first error, each through the reply readers.
pub fn lsp_frame(data: &[u8]) {
    runtime().block_on(async {
        let mut reader = tokio::io::BufReader::new(data);
        for _ in 0..64 {
            match ferris_idea_languages::lsp::read_message(&mut reader).await {
                Ok(Some(v)) => lsp_replies(&v),
                Ok(None) | Err(_) => break,
            }
        }
    });
    // The reply readers on the bytes as plain JSON too.
    if let Ok(v) = serde_json::from_slice::<Value>(data) {
        lsp_replies(&v);
    }
}

/// DAP framing: messages until the end or the first error.
pub fn dap_frame(data: &[u8]) {
    let mut reader = std::io::BufReader::new(data);
    for _ in 0..64 {
        match ferris_idea_debugger::read_message(&mut reader) {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
}

/// Read-only tools the MCP target may call with fuzzed arguments; nothing that writes, runs or
/// reaches the network, so fuzzing stays inside its scratch workspace.
const MCP_TOOLS: &[&str] = &[
    "read_file",
    "list_files",
    "search_in_files",
    "detect_language",
    "highlight_file",
    "search_everywhere",
    "folding_ranges",
    "editorconfig",
    "idea_project",
    "project_model",
    "parse_test_report",
    "merge_three_way",
    "inspect_sql",
    "properties_file",
    "keymap_lookup",
    "http_requests",
    "terminal_render",
    "preview_multi_caret_edit",
    "preview_column_edit",
    "replace_in_files_preview",
    "local_history",
    "project_dictionary",
    "git_status",
];

/// Arguments for a fuzzed call: the bytes as a JSON object, else as every common string field.
fn mcp_arguments(tool: &str, data: &[u8]) -> Value {
    match serde_json::from_slice::<Value>(data) {
        Ok(v @ Value::Object(_)) => v,
        _ => {
            let s = text(data);
            let mut all = json!({
                "path": s, "text": s, "query": s, "sql": s, "output": s, "pattern": s,
                "base": s, "ours": s, "theirs": s, "report": s, "keystroke": s,
            });
            // Only the names the tool declares, so the call reaches the tool instead of the
            // refusal of unknown arguments (`SPEC-0006` v59 behaviour 21).
            if let (Some(declared), Some(map)) = (
                MCP_DECLARED.get(tool).and_then(Option::as_ref),
                all.as_object_mut(),
            ) {
                map.retain(|k, _| declared.contains(k));
            }
            all
        }
    }
}

/// The argument names each tool declares (`SPEC-0006` v59 behaviour 21), read once.
static MCP_DECLARED: std::sync::LazyLock<
    BTreeMap<String, Option<std::collections::BTreeSet<String>>>,
> = std::sync::LazyLock::new(|| ferris_idea_mcp::declared_arguments().into_iter().collect());

struct McpClient {
    write: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    lines: tokio::io::Lines<tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    next: u64,
}

impl McpClient {
    async fn connect() -> Self {
        use rmcp::ServiceExt;
        use tokio::io::AsyncBufReadExt;
        let root = scratch();
        write(&root, "src/app.py", "a = 1\nb = 2\n");
        write(&root, "schema.sql", "select 1; select 2;");
        write(&root, ".idea/misc.xml", "<project/>");
        let permissions = ferris_idea_core::AgentPermissions {
            allow: Default::default(),
            file: Some(root.join("agent-permissions.toml")),
            problem: None,
        };
        let workspace = ferris_idea_core::Workspace::open(&root).expect("scratch workspace");
        let server = ferris_idea_mcp::IdeServer::with_permissions(workspace, permissions)
            .with_plugins_dir(root.join(".plugins"))
            .with_theme_file(root.join(".theme"))
            .with_ide_settings_file(root.join(".settings.toml"))
            .with_keymap_file(root.join(".keymap.toml"))
            .with_window_layouts_file(root.join(".window-layouts.json"))
            .with_package_index("http://127.0.0.1:9".into(), root.join(".pypi.txt"));
        let (client_io, server_io) = tokio::io::duplex(1 << 20);
        tokio::spawn(async move {
            if let Ok(running) = server.serve(tokio::io::split(server_io)).await {
                let _ = running.waiting().await;
            }
        });
        let (read, write) = tokio::io::split(client_io);
        let mut client = Self {
            write,
            lines: tokio::io::BufReader::new(read).lines(),
            next: 1 << 40,
        };
        let init = client
            .request(
                "initialize",
                json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "fuzz", "version": "0"}}),
            )
            .await
            .expect("initialize");
        assert!(init.get("result").is_some(), "initialize failed: {init}");
        client
            .send_line(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .await;
        client
    }

    async fn send_line(&mut self, line: &[u8]) -> bool {
        use tokio::io::AsyncWriteExt;
        let mut buf = line.to_vec();
        buf.push(b'\n');
        self.write.write_all(&buf).await.is_ok() && self.write.flush().await.is_ok()
    }

    /// Sends a request and waits for its answer; `None` when the connection closed.
    async fn request(&mut self, method: &str, params: Value) -> Option<Value> {
        self.next += 1;
        let id = self.next;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if !self.send_line(line.to_string().as_bytes()).await {
            return None;
        }
        let wait = async {
            loop {
                let line = self.lines.next_line().await.ok()??;
                if let Ok(v) = serde_json::from_str::<Value>(&line)
                    && v["id"] == json!(id)
                {
                    return Some(v);
                }
            }
        };
        match tokio::time::timeout(std::time::Duration::from_secs(20), wait).await {
            Ok(answer) => answer,
            Err(_) => panic!("MCP server did not answer `{method}` within 20 s"),
        }
    }
}

/// MCP: raw lines (mode 0) or a read-only tool call (mode 1), then a ping that must be answered.
pub fn mcp_jsonrpc(data: &[u8]) {
    thread_local!(static CLIENT: RefCell<Option<McpClient>> = const { RefCell::new(None) });
    let (mode, rest) = data.split_first().unwrap_or((&0, &[]));
    let mut client = CLIENT
        .with(|c| c.borrow_mut().take())
        .unwrap_or_else(|| runtime().block_on(McpClient::connect()));
    let alive = runtime().block_on(async {
        if mode % 2 == 0 {
            // Raw bytes: one line per newline-separated piece, like a broken client.
            for line in rest.split(|b| *b == b'\n').take(16) {
                if !client.send_line(line).await {
                    return false;
                }
            }
        } else {
            let (pick, args) = rest.split_first().unwrap_or((&0, &[]));
            let tool = MCP_TOOLS[*pick as usize % MCP_TOOLS.len()];
            let params = json!({"name": tool, "arguments": mcp_arguments(tool, args)});
            if client.request("tools/call", params).await.is_none() {
                return false;
            }
        }
        client.request("ping", json!({})).await.is_some()
    });
    // A server that hangs up on garbage is allowed; the next input gets a fresh one.
    if alive {
        CLIENT.with(|c| *c.borrow_mut() = Some(client));
    }
}

/// SQL splitting in every dialect, with the promised ranges.
pub fn sql_split(data: &[u8]) {
    let script = text(data);
    for dialect in ferris_idea_database::Dialect::ALL {
        let statements = ferris_idea_database::split_statements(&script, dialect);
        let mut last = 0;
        for s in &statements {
            assert!(
                s.range.start <= s.range.end && s.range.end <= script.len(),
                "{dialect:?}: range {:?} outside the script",
                s.range
            );
            assert!(s.range.start >= last, "{dialect:?}: statements overlap");
            assert_eq!(
                script.get(s.range.clone()),
                Some(s.text.as_str()),
                "{dialect:?}: text is not the script slice"
            );
            last = s.range.end;
        }
    }
}

/// Terminal: size from the first bytes, output fed in chunks, a resize, then every reader.
pub fn terminal_vte(data: &[u8]) {
    let get = |i: usize| data.get(i).copied().unwrap_or(0) as usize;
    let rows = 1 + get(0) % 60;
    let cols = 1 + get(1) % 160;
    let chunk = 1 + get(2) % 64;
    let resize = get(3);
    let body = data.get(4..).unwrap_or_default();
    let mut terminal = ferris_idea_terminal::Terminal::new(rows, cols);
    let half = body.len() / 2;
    for piece in body[..half].chunks(chunk) {
        terminal.feed(piece);
    }
    if resize % 3 == 1 {
        terminal.screen.resize(1 + resize % 37, 1 + resize % 101);
    }
    for piece in body[half..].chunks(chunk) {
        terminal.feed(piece);
    }
    let screen = &mut terminal.screen;
    let (rows, cols) = screen.size();
    let (row, col) = screen.cursor();
    assert!(
        row < rows && col <= cols,
        "cursor {row},{col} outside {rows}x{cols}"
    );
    let text = screen.text();
    let _ = screen.scrollback();
    let _ = screen.history_lines(200);
    let _ = screen.prompts().len();
    let _ = screen.take_responses();
    let _ = ferris_idea_terminal::find_links(&text);
    for index in 0..screen.scrollback_len() + rows {
        if let Some(cells) = screen.line(index) {
            let _ = ferris_idea_terminal::line_links(screen, cells);
        }
    }
    let query: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .take(2)
        .collect();
    if !query.is_empty() {
        let _ = ferris_idea_terminal::search(screen, &query, false);
        let _ = ferris_idea_terminal::search(screen, &query, true);
    }
    // History restore drops control characters, whatever the lines hold.
    let lines = screen.history_lines(20);
    ferris_idea_terminal::Terminal::new(rows, cols).restore(&lines);
}

/// The undo history the model keeps: one step per entry, `(id, before, after)`.
#[derive(Default)]
struct Model {
    text: Vec<char>,
    undo: Vec<(u64, Vec<char>, Vec<char>)>,
    redo: Vec<(u64, Vec<char>, Vec<char>)>,
    /// An open group; the text before its first edit once it has one.
    group: Option<Option<Vec<char>>>,
    next_id: u64,
    saved: u64,
}

impl Model {
    fn current(&self) -> u64 {
        self.undo.last().map_or(0, |s| s.0)
    }

    fn end_group(&mut self) {
        if let Some(Some(before)) = self.group.take() {
            self.push(before);
        }
    }

    fn push(&mut self, before: Vec<char>) {
        self.next_id += 1;
        self.undo.push((self.next_id, before, self.text.clone()));
    }

    fn replace(&mut self, start: usize, end: usize, new: &[char]) {
        if start == end && new.is_empty() {
            return;
        }
        let before = self.text.clone();
        self.text.splice(start..end, new.iter().copied());
        self.redo.clear();
        match &mut self.group {
            Some(first) => {
                first.get_or_insert(before);
            }
            None => self.push(before),
        }
    }
}

/// Text buffer: an op script against a plain model of text and history.
pub fn text_buffer(data: &[u8]) {
    use ferris_idea_editor::TextBuffer;
    let mut ops = data;
    let mut take = |n: usize| -> &[u8] {
        let (a, b) = ops.split_at(n.min(ops.len()));
        ops = b;
        a
    };
    let seed = text(take(8));
    let mut buffer = TextBuffer::from_text(&seed);
    let mut model = Model {
        text: seed.chars().collect(),
        ..Model::default()
    };
    let byte = |b: &[u8]| b.first().copied().unwrap_or(0) as usize;
    for _ in 0..256 {
        let op = take(1);
        if op.is_empty() {
            break;
        }
        let len = model.text.len();
        match op[0] % 12 {
            0..=2 => {
                let start = byte(take(1)) % (len + 2);
                let width = byte(take(1)) % 4;
                let n = byte(take(1)) % 6;
                let new = text(take(n));
                let end = start + width;
                let result = buffer.replace(start..end, &new);
                if end <= len {
                    result.expect("replace inside the text");
                    let chars: Vec<char> = new.chars().collect();
                    model.replace(start, end, &chars);
                } else {
                    assert!(
                        result.is_err(),
                        "replace {start}..{end} past {len} accepted"
                    );
                }
            }
            3 => {
                buffer.begin_group();
                model.group.get_or_insert(None);
            }
            4 => {
                buffer.end_group();
                model.end_group();
            }
            5 => {
                let done = buffer.undo();
                model.end_group();
                let step = model.undo.pop();
                assert_eq!(done, step.is_some(), "undo availability");
                if let Some(step) = step {
                    model.text = step.1.clone();
                    model.redo.push(step);
                }
            }
            6 => {
                let done = buffer.redo();
                let step = model.redo.pop();
                assert_eq!(done, step.is_some(), "redo availability");
                if let Some(step) = step {
                    model.text = step.2.clone();
                    model.undo.push(step);
                }
            }
            7 => {
                buffer.mark_saved();
                model.end_group();
                model.saved = model.current();
            }
            8 => {
                let joined = buffer.join_last_steps();
                model.end_group();
                let n = model.undo.len();
                let expect = n >= 2 && model.undo[n - 2].0 != model.saved;
                assert_eq!(joined, expect, "join_last_steps");
                if expect {
                    let last = model.undo.pop().expect("two steps");
                    let earlier = model.undo.last_mut().expect("two steps");
                    earlier.0 = last.0;
                    earlier.2 = last.2;
                }
            }
            9 => {
                let n = byte(take(1)) % 8;
                let new = text(take(n));
                buffer.reload(&new);
                model.end_group();
                let chars: Vec<char> = new.chars().collect();
                // A reload replaces only changed lines: the same text is no undo step (D40).
                if chars != model.text {
                    let all = model.text.len();
                    model.replace(0, all, &chars);
                }
                model.saved = model.current();
            }
            10 => {
                let offset = byte(take(1)) % (len + 2);
                match buffer.offset_to_position(offset) {
                    Ok(pos) => {
                        assert!(offset <= len, "position for offset {offset} past {len}");
                        // An offset between `\r` and `\n` is inside the line break: it has a
                        // position, but no position names it (recorded in HARDENING.md).
                        let inside_crlf = offset > 0
                            && model.text.get(offset - 1) == Some(&'\r')
                            && model.text.get(offset) == Some(&'\n');
                        if !inside_crlf {
                            assert_eq!(buffer.position_to_offset(pos), Ok(offset), "round trip");
                        }
                    }
                    Err(_) => assert!(offset > len, "offset {offset} of {len} refused"),
                }
                let _ = buffer.char_to_utf16(offset);
                let _ = buffer.utf16_to_char(offset);
                let _ = buffer.line(byte(take(1)) % 8);
                let _ = buffer.line_ending();
            }
            _ => {
                let _ = buffer.undo_caret();
                model.end_group();
                if let Some(step) = model.undo.pop() {
                    model.text = step.1.clone();
                    model.redo.push(step);
                }
            }
        }
        let text: String = model.text.iter().collect();
        assert_eq!(
            buffer.text(),
            text,
            "buffer and model differ after op {}",
            op[0]
        );
        let modified =
            model.group.as_ref().is_some_and(Option::is_some) || model.current() != model.saved;
        assert_eq!(buffer.is_modified(), modified, "modified flag");
    }
    // Undoing everything gives the first text back.
    while buffer.undo() {}
    if let Some(first) = model.undo.first().map(|s| s.1.clone()) {
        assert_eq!(buffer.text(), first.iter().collect::<String>(), "undo all");
    }
}

/// Diffs and the three-way merge of three texts.
pub fn diff_merge(data: &[u8]) {
    use ferris_idea_vcs::{Labels, MarkerStyle, Merge3, Resolution};
    let p = parts(data, 3);
    let (base, ours, theirs) = (&p[0], &p[1], &p[2]);
    let _ = ferris_idea_vcs::line_changes(base, ours);
    let _ = ferris_idea_vcs::unified_hunks(base, theirs);
    let rows = ferris_idea_vcs::side_by_side(ours, theirs);
    let _ = ferris_idea_vcs::change_starts(&rows);
    let per_line: Vec<Option<usize>> = (0..base.lines().count()).map(Some).collect();
    let _ = ferris_idea_vcs::map_lines(base, ours, &per_line);
    for mut merge in [
        Merge3::new(base, ours, theirs),
        Merge3::untrimmed(base, ours, theirs),
    ] {
        if ours == base || theirs == base || ours == theirs {
            assert_eq!(merge.open_conflicts(), 0, "conflict with an unchanged side");
        }
        let labels = Labels {
            ours: "ours".into(),
            base: "base".into(),
            theirs: "theirs".into(),
        };
        let _ = merge.text(&labels, MarkerStyle::Merge);
        let _ = merge.text(&labels, MarkerStyle::Diff3);
        let _ = merge.simple_conflicts();
        let _ = merge.resolve_simple();
        let all = [
            Resolution::Ours,
            Resolution::Theirs,
            Resolution::OursThenTheirs,
            Resolution::TheirsThenOurs,
            Resolution::Base,
        ];
        for (i, c) in merge.conflicts().into_iter().enumerate() {
            merge.resolve(c, Some(all[i % all.len()].clone()));
        }
        assert_eq!(
            merge.open_conflicts(),
            0,
            "a conflict left after resolving all"
        );
        let _ = merge.resolve(usize::MAX, None);
    }
}

/// Arbitrary bytes as a plugin module.
pub fn plugin_module(data: &[u8]) {
    if let Ok(plugin) = ferris_idea_plugins::Plugin::from_bytes(Path::new("fuzz.wasm"), data) {
        for action in plugin.manifest.actions.clone() {
            let _ = plugin.run_with_fuel(&action.id, "text", 1_000_000);
        }
    }
}

/// A module whose manifest and every answer are the given bytes.
fn answering_module(manifest: &[u8], answer: &[u8]) -> Vec<u8> {
    let escape = |bytes: &[u8]| -> String { bytes.iter().map(|b| format!("\\{b:02x}")).collect() };
    let wat = format!(
        r#"(module
  (memory (export "memory") 2)
  (global $next (mut i32) (i32.const 131072))
  (data (i32.const 1024) "{m}")
  (data (i32.const 65536) "{a}")
  (func (export "ferris_idea_alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $len)))
    (if (i32.gt_u (global.get $next) (i32.mul (memory.size) (i32.const 65536)))
      (then (drop (memory.grow (i32.add (i32.div_u (local.get $len) (i32.const 65536)) (i32.const 1))))))
    (local.get $at))
  (func (export "ferris_idea_manifest") (result i64)
    (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const {ml})))
  (func (export "ferris_idea_run") (param i32 i32 i32 i32) (result i64)
    (i64.or (i64.shl (i64.const 65536) (i64.const 32)) (i64.const {al}))))"#,
        m = escape(manifest),
        a = escape(answer),
        ml = manifest.len(),
        al = answer.len(),
    );
    wat::parse_str(wat).expect("generated module")
}

const ANSWER_MANIFEST: &str = r#"{"name":"fuzz","actions":[{"id":"e","title":"E","kind":"edit"},{"id":"i","title":"I","kind":"inspect"},{"id":"c","title":"C","kind":"complete"},{"id":"s","title":"S","kind":"status"},{"id":"p","title":"P","kind":"panel"},{"id":"r","title":"R"}]}"#;

/// Plugin answers: a manifest (mode 0), one action's answer (modes 1–5), a file type (mode 6).
pub fn plugin_answers(data: &[u8]) {
    use ferris_idea_plugins::{
        Access, CompleteContext, EditorContext, FileType, InspectContext, PanelContext, Plugin,
    };
    let (mode, rest) = data.split_first().unwrap_or((&0, &[]));
    let rest = &rest[..rest.len().min(32 << 10)];
    let path = Path::new("fuzz.wasm");
    let source = "fn main() {\n    let x = 1;\n}\n";
    let editor = EditorContext {
        path: "a.rs".into(),
        language: Some("rust".into()),
        text: source.into(),
        selection_start: 3,
        selection_end: 7,
        line: 0,
        column: 3,
    };
    match mode % 7 {
        0 => {
            if let Ok(p) = Plugin::from_bytes(path, &answering_module(rest, b"{}")) {
                let _ = ferris_idea_plugins::contributions(&p.manifest);
                for t in &p.manifest.file_types {
                    let _ = ferris_idea_plugins::highlight(t, source);
                }
                let plugins = [p];
                let _ = ferris_idea_plugins::file_type_for(&plugins, Path::new("a.t"), false);
                let _ = ferris_idea_plugins::action_list(&plugins);
            }
        }
        6 => {
            let (json, text_part) = match rest.iter().position(|b| *b == 0) {
                Some(i) => (&rest[..i], &rest[i + 1..]),
                None => (rest, &b""[..]),
            };
            if let Ok(t) = serde_json::from_slice::<FileType>(json) {
                let source = text(text_part);
                let len = source.chars().count();
                let mut last = 0;
                for (range, _) in ferris_idea_plugins::highlight(&t, &source) {
                    assert!(
                        range.start >= last && range.start <= range.end && range.end <= len,
                        "highlight run {range:?} of {len} characters"
                    );
                    last = range.end;
                }
            }
        }
        m => {
            let plugin =
                Plugin::from_bytes(path, &answering_module(ANSWER_MANIFEST.as_bytes(), rest))
                    .expect("fixed manifest loads");
            let access = Access::default;
            match m {
                1 => {
                    if let Ok(r) = plugin.run_edit("e", &editor, access()) {
                        let len = source.chars().count();
                        for e in &r.edits {
                            assert!(e.start <= e.end && e.end <= len, "edit outside the text");
                        }
                    }
                }
                2 => {
                    let context = InspectContext {
                        path: "a.rs".into(),
                        language: None,
                        text: source.into(),
                    };
                    let _ = plugin.run_inspect("i", &context, access());
                }
                3 => {
                    let context = CompleteContext {
                        path: "a.rs".into(),
                        language: None,
                        text: source.into(),
                        offset: 5,
                        prefix: "ma".into(),
                    };
                    let _ = plugin.run_complete("c", &context, access());
                }
                4 => {
                    if let Ok(s) = plugin.run_status("s", &editor, access()) {
                        assert!(!s.text.contains('\n'), "status text on more than one line");
                    }
                }
                _ => {
                    let context = PanelContext {
                        project: "/p".into(),
                        active: None,
                    };
                    let _ = plugin.run_panel("p", &context, access());
                    let _ = plugin.run("r", "x");
                }
            }
        }
    }
}

/// Keymap XML, keystrokes and shortcuts.
pub fn keymap(data: &[u8]) {
    use ferris_idea_ui::keymap::{KeymapFile, Keystroke, Shortcut};
    let s = text(data);
    let _ = KeymapFile::parse(&s);
    for line in s.lines().take(8) {
        if let Ok(k) = Keystroke::parse(line) {
            let _ = k.display(true);
            let _ = k.display(false);
            let _ = k.to_string();
        }
        if let Ok(sc) = Shortcut::parse_keyboard(line) {
            let _ = sc.display(true);
            let _ = sc.swap_control_meta();
        }
    }
}

/// Colour schemes (mode even) and UI themes (mode odd).
pub fn theme(data: &[u8]) {
    use ferris_idea_ui::theme::{ColorScheme, UiTheme, parse_schemes};
    let (mode, rest) = data.split_first().unwrap_or((&0, &[]));
    let s = text(rest);
    if mode % 2 == 0 {
        if let Ok(schemes) = parse_schemes(&s) {
            let bases = parse_schemes(&s).unwrap_or_default();
            for scheme in schemes {
                if let Ok(cs) = ColorScheme::resolve(scheme, &bases) {
                    let _ = cs.chain();
                    let _ = cs.color("CARET_ROW_COLOR");
                    let _ = cs.attributes("KEYWORD");
                    let _ = cs.capture_style("keyword");
                }
            }
        }
    } else if let Ok(t) = UiTheme::parse(&s) {
        let _ = t.color("*.background");
        let _ = t.number("Button.arc");
        let _ = t.insets("Button.padding");
        let _ = UiTheme::with_parent(&s, &t);
    }
}

/// Settings readers, picked by the first byte.
pub fn settings(data: &[u8]) {
    let (mode, rest) = data.split_first().unwrap_or((&0, &[]));
    let s = text(rest);
    match mode % 10 {
        0 => {
            let mut config = ferris_idea_core::LayeredConfig::new();
            if config
                .set_layer_toml(ferris_idea_core::Layer::User, &s)
                .is_ok()
            {
                let _ = config.effective();
                let _ = config.get::<String>("editor.font");
                let _ = config.get::<i64>("editor.font_size");
                let _ = config.origin("editor");
            }
        }
        1 => {
            let root = scratch();
            let file = write(&root, "settings.toml", &s);
            let settings = ferris_idea_core::load_ide_settings(&file).clamped();
            let _ = settings.line_height();
        }
        2 => {
            let _ = ferris_idea_core::parse_run_configs(&s);
        }
        3 => {
            let _ = ferris_idea_ui::tool_windows::Layout::parse(&s);
        }
        4 => {
            let _ = ferris_idea_debugger::saved::parse_breakpoints(Path::new("/p"), &s);
        }
        5 => {
            let root = scratch();
            write(&root, ".editorconfig", &s);
            let config = ferris_idea_core::editorconfig::editorconfig_for(&root.join("a/b.rs"));
            let _ = config.indent_unit();
            let _ = config.line_break();
            for line in s.lines().take(4) {
                let _ = ferris_idea_core::editorconfig::section_matches(line, "a/b.rs");
            }
        }
        6 => {
            let root = scratch();
            let file = write(&root, "grants.toml", &s);
            let _ = ferris_idea_core::load_agent_permissions(&file, Path::new("/"));
        }
        7 => {
            let root = scratch();
            let file = write(&root, "recent.txt", &s);
            let _ = ferris_idea_core::load_recent_projects(&file);
        }
        8 => {
            let root = scratch();
            write(&root, ferris_idea_database::PROFILE_FILE, &s);
            let _ = ferris_idea_database::load_profiles(&root);
        }
        _ => {
            let root = scratch();
            let p = parts(rest, 2);
            write(&root, ferris_idea_http::ENV_FILE, &p[0]);
            write(&root, ferris_idea_http::PRIVATE_ENV_FILE, &p[1]);
            let _ = ferris_idea_http::load_environments(&root, &root);
        }
    }
}

/// `.idea` folder: `.name`, misc, modules, VCS, one library and one module file.
pub fn idea_project(data: &[u8]) {
    let p = parts(data, 6);
    let root = scratch();
    let files = [
        ".idea/.name",
        ".idea/misc.xml",
        ".idea/modules.xml",
        ".idea/vcs.xml",
        ".idea/libraries/l.xml",
        "m.iml",
    ];
    for (rel, content) in files.iter().zip(&p) {
        write(&root, rel, content);
    }
    let _ = ferris_idea_core::read_idea_project(&root);
}

/// Build files of every kind the project model reads.
pub fn project_model(data: &[u8]) {
    let names = [
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "requirements.txt",
        "pom.xml",
        "settings.gradle",
        "build.gradle.kts",
        "Cargo.lock",
        "b/Cargo.toml",
    ];
    let p = parts(data, names.len());
    let root = scratch();
    let files: Vec<PathBuf> = names
        .iter()
        .zip(&p)
        .map(|(rel, content)| {
            write(&root, rel, content);
            PathBuf::from(rel)
        })
        .collect();
    let _ = ferris_idea_core::read_project(&root, &files);
}

/// `.http` files resolved against an environment.
pub fn http_file(data: &[u8]) {
    let s = text(data);
    let file = ferris_idea_http::parse(&s);
    let env: BTreeMap<String, String> = [("host", "localhost"), ("v", "{{v}}")]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    for request in ferris_idea_http::resolve(&file, &env) {
        let _ = ferris_idea_http::parse_url(&request.url);
    }
    let _ = ferris_idea_http::substitute(&s, &env, &env);
    for line in s.lines().take(4) {
        let _ = ferris_idea_http::parse_url(line);
    }
}

/// Test reports in every format and with detection.
pub fn test_report(data: &[u8]) {
    use ferris_idea_core::ReportFormat;
    let s = text(data);
    let _ = ferris_idea_core::parse_report(&s, None);
    for f in [
        ReportFormat::Junit,
        ReportFormat::Tap,
        ReportFormat::Libtest,
        ReportFormat::Jest,
    ] {
        let _ = ferris_idea_core::parse_report(&s, Some(f));
    }
}

/// Profiler outputs, picked by the first byte.
pub fn profiler_formats(data: &[u8]) {
    use ferris_idea_profiler as p;
    let (mode, rest) = data.split_first().unwrap_or((&0, &[]));
    let s = text(rest);
    let profile = match mode % 9 {
        0 => p::parse_jfr(&s),
        1 => p::parse_sample(&s),
        2 => p::parse_perf_script(&s),
        3 => p::parse_massif(&s),
        4 => p::parse_malloc_history(&s),
        5 => match p::parse_tracemalloc(&s) {
            Ok(profile) => profile,
            Err(_) => return,
        },
        6 => match p::parse_cprofile(&s) {
            Ok(profile) => profile,
            Err(_) => return,
        },
        7 => p::parse_heap(&s),
        _ => {
            let _ = p::parse_ps(&s, 1);
            return;
        }
    };
    let _ = p::hot_spots(&profile);
}

/// Smaller readers, picked by the first byte.
pub fn misc_formats(data: &[u8]) {
    let (mode, rest) = data.split_first().unwrap_or((&0, &[]));
    let s = text(rest);
    match mode % 10 {
        0 => {
            let _ = ferris_idea_database::parse_postgres(&s);
        }
        1 => {
            let _ = ferris_idea_database::parse_mysql(&s);
            let rows: Vec<Vec<Option<String>>> = s
                .lines()
                .take(64)
                .map(|l| l.split('\t').map(|c| Some(c.to_owned())).collect())
                .collect();
            let _ = ferris_idea_database::parse_sqlite(&rows);
        }
        2 => {
            let _ = ferris_idea_core::jvmdeps::parse_maven_tree(&s);
        }
        3 => {
            let _ = ferris_idea_core::jvmdeps::parse_gradle_tree(&s);
        }
        4 => {
            let _ = ferris_idea_core::parse_specs(&s);
            for line in s.lines().take(8) {
                let _ = ferris_idea_core::split_pep508(line);
            }
        }
        5 => {
            let _ = ferris_idea_languages::properties::parse(&s);
        }
        6 => {
            let _ = ferris_idea_languages::project_dictionary::parse(&s);
        }
        7 => {
            let _ = ferris_idea_languages::js_tools::parse_eslint("let a = 1;\nlet b;\n", &s);
        }
        8 => {
            for open in [0, 1, s.len() / 2, s.len()] {
                for types in [false, true] {
                    if let Some((outer, items)) =
                        ferris_idea_languages::signature::split_list(&s, open, types)
                    {
                        assert!(outer.end <= s.len(), "list span past the text");
                        for item in items {
                            assert!(
                                item.start <= item.end && item.end <= s.len(),
                                "item span past the text"
                            );
                        }
                    }
                }
            }
        }
        _ => {
            use ferris_idea_ai::subscription::{Cli, parse_line};
            for cli in [Cli::Claude, Cli::Codex, Cli::Gemini] {
                for line in s.lines().take(8) {
                    let _ = parse_line(cli, line);
                }
            }
        }
    }
}
