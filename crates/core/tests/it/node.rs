//! SPEC-0051 AK-1 – AK-4.

use std::path::PathBuf;
use std::process::Command;

use ferris_idea_core::{
    NodeSearch, RunConfigs, RunKind, find_node_runtimes, node_environment, run_config,
};

/// Skips a test whose tool is missing; with `FERRIS_IDEA_REQUIRE_TOOLS` set (CI) it fails instead.
fn skip(reason: &str) {
    assert!(
        ferris_idea_core::product_var_os("FERRIS_IDEA_REQUIRE_TOOLS").is_none(),
        "required tool missing: {reason}"
    );
    eprintln!("skipped: {reason}");
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("ferris-idea-node-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("proj")).unwrap();
        Self(p.canonicalize().unwrap())
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn search(&self) -> NodeSearch {
        NodeSearch {
            root: self.0.join("proj"),
            ..NodeSearch::default()
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const HEADER: &str = "#ifndef SRC_NODE_VERSION_H_\n#define NODE_MAJOR_VERSION 21\n#define NODE_MINOR_VERSION 7\n#define NODE_PATCH_VERSION 3\n";

#[test]
fn runtimes_and_selection() {
    let s = Scratch::new("runtimes");
    s.write("nvm/versions/node/v18.20.0/bin/node", "");
    s.write("nvm/versions/node/v20.11.1/bin/node", "");
    s.write("nvm/versions/node/not-a-version/bin/node", "");
    s.write("fnm/node-versions/v22.1.0/installation/bin/node", "");
    s.write("usr/bin/node", "");
    s.write("usr/include/node/node_version.h", HEADER);
    let mut search = s.search();
    search.nvm_dir = Some(s.0.join("nvm"));
    search.fnm_dir = Some(s.0.join("fnm"));
    search.path_var = Some(s.0.join("usr/bin").into_os_string());

    let found = find_node_runtimes(&search);
    let summary: Vec<(&str, Option<&str>)> = found
        .iter()
        .map(|r| (r.found_by.as_str(), r.version.as_deref()))
        .collect();
    assert_eq!(
        summary,
        [
            ("nvm", Some("20.11.1")),
            ("nvm", Some("18.20.0")),
            ("fnm", Some("22.1.0")),
            ("PATH", Some("21.7.3")),
        ]
    );

    let env = node_environment(&search);
    assert_eq!(env.reason.as_deref(), Some("first node on PATH"));

    s.write("proj/.nvmrc", "v18\n");
    let env = node_environment(&search);
    assert_eq!(
        env.runtimes[env.selected.unwrap()].version.as_deref(),
        Some("18.20.0")
    );
    assert_eq!(env.reason.as_deref(), Some("version file asks for v18"));

    std::fs::remove_file(s.0.join("proj/.nvmrc")).unwrap();
    s.write("proj/.node-version", "16.20\n");
    let env = node_environment(&search);
    assert_eq!(env.reason.as_deref(), Some("first node on PATH"));
    assert!(
        env.notes
            .iter()
            .any(|n| n.contains("no installed node matches the version file (16.20)")),
        "{:?}",
        env.notes
    );

    search.configured = Some(s.0.join("fnm/node-versions/v22.1.0/installation"));
    let env = node_environment(&search);
    assert_eq!(env.reason.as_deref(), Some("chosen in the settings"));
    assert_eq!(
        env.runtimes[env.selected.unwrap()].version.as_deref(),
        Some("22.1.0")
    );
}

#[test]
fn package_manager_packages_and_tests() {
    let s = Scratch::new("project");
    s.write("usr/bin/node", "");
    s.write("usr/include/node/node_version.h", HEADER);
    s.write(
        "proj/package.json",
        r#"{
  "name": "shop",
  "engines": {"node": ">=22"},
  "packageManager": "pnpm@9.1.0+sha512.abc",
  "dependencies": {"react": "^19.0.0", "@scope/ui": "~2.1.0", "local": "workspace:*", "tag": "latest"},
  "devDependencies": {"vite": "^7.0.0", "typescript": "^5.6.0", "vitest": "^2"},
  "optionalDependencies": {"fsevents": "^2.3.0"},
  "peerDependencies": {"react-dom": "^19"}
}"#,
    );
    s.write("proj/package-lock.json", "{}");
    s.write(
        "proj/node_modules/react/package.json",
        r#"{"name":"react","version":"19.1.0"}"#,
    );
    s.write(
        "proj/node_modules/@scope/ui/package.json",
        r#"{"version":"2.1.4"}"#,
    );
    s.write(
        "proj/node_modules/vite/package.json",
        r#"{"version":"6.3.0"}"#,
    );
    s.write(
        "proj/node_modules/vitest/package.json",
        r#"{"version":"2.1.8"}"#,
    );
    let mut search = s.search();
    search.path_var = Some(s.0.join("usr/bin").into_os_string());

    let env = node_environment(&search);
    assert_eq!(env.engines_node.as_deref(), Some(">=22"));
    assert_eq!(env.compatible, Some(false));
    assert!(
        env.notes[0].contains("(21.7.3) does not satisfy engines.node >=22"),
        "{:?}",
        env.notes
    );
    let pm = env.package_manager.as_ref().unwrap();
    assert_eq!(
        (pm.name.as_str(), pm.version.as_deref()),
        ("pnpm", Some("9.1.0"))
    );
    assert_eq!(
        env.problems,
        ["packageManager is pnpm but the lockfile is package-lock.json"]
    );
    assert!(env.installed);
    let bad: Vec<(&str, Option<&str>)> = env
        .unsatisfied
        .iter()
        .map(|u| (u.name.as_str(), u.installed.as_deref()))
        .collect();
    assert_eq!(bad, [("typescript", None), ("vite", Some("6.3.0"))]);
    assert_eq!(env.not_checked, ["local workspace:*", "tag latest"]);
    assert_eq!(env.test_framework.as_deref(), Some("vitest"));
    let config = env.test_config.unwrap();
    assert_eq!(config.kind, RunKind::Test);
    assert_eq!(
        config.report_file.as_deref(),
        Some(".ferris-idea/vitest.json")
    );

    // Two package managers' lockfiles; no packageManager field.
    s.write(
        "proj/package.json",
        r#"{"name": "shop", "devDependencies": {"jest": "^29"}}"#,
    );
    s.write("proj/yarn.lock", "");
    std::fs::remove_dir_all(s.0.join("proj/node_modules")).unwrap();
    let env = node_environment(&search);
    assert_eq!(
        env.problems,
        ["lockfiles of several package managers: package-lock.json, yarn.lock"]
    );
    assert_eq!(env.package_manager.unwrap().source, "package-lock.json");
    assert!(!env.installed);
    assert!(
        env.notes
            .contains(&"dependencies are not installed (no node_modules)".to_owned())
    );
    assert_eq!(env.test_framework.as_deref(), Some("jest"));

    s.write("proj/package.json", "{ broken");
    let env = node_environment(&search);
    assert!(env.problems.iter().any(|p| p.starts_with("package.json:")));
}

/// The installed node: the version read from its files equals its own answer, and the suggested
/// `node --test` configuration runs real tests with a TAP report.
#[test]
fn real_node_runtime() {
    let s = Scratch::new("real");
    let Ok(out) = Command::new("node")
        .args(["-p", "process.versions.node"])
        .output()
    else {
        skip("node not installed");
        return;
    };
    let real = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    s.write(
        "proj/package.json",
        r#"{"name": "demo", "scripts": {"test": "node --test"}}"#,
    );
    s.write(
        "proj/math.test.mjs",
        "import test from 'node:test';\nimport assert from 'node:assert';\ntest('adds', () => assert.equal(1 + 1, 2));\ntest('skips', { skip: 'later' }, () => {});\n",
    );
    let search = NodeSearch::from_env(&s.0.join("proj"));
    let env = node_environment(&search);
    let chosen = &env.runtimes[env.selected.unwrap()];
    if chosen.version.is_none() {
        skip("node installation without include/node/node_version.h");
        return;
    }
    assert_eq!(chosen.version.as_deref(), Some(real.as_str()));
    assert_eq!(env.test_framework.as_deref(), Some("node:test"));
    let configs = RunConfigs {
        configs: vec![env.test_config.unwrap()],
        problems: Vec::new(),
    };
    let out = run_config(&s.0.join("proj"), &configs, "node --test").unwrap();
    assert!(out[0].success(), "{:?}", out[0]);
    let report = out[0]
        .report
        .as_ref()
        .unwrap_or_else(|| panic!("{:?}", out[0]));
    assert_eq!(report.totals.passed, 1, "{report:?}");
    assert_eq!(report.totals.skipped, 1, "{report:?}");
}

/// SPEC-0133 behaviour 4: web frameworks with their development server and page.
#[test]
fn web_frameworks_and_their_pages() {
    let s = Scratch::new("frameworks");
    s.write(
        "proj/package.json",
        r#"{"scripts": {"dev": "vite"}, "dependencies": {"react": "^19.0.0"}, "devDependencies": {"vite": "^6.0.0"}}"#,
    );
    let env = node_environment(&s.search());
    assert_eq!(env.frameworks, ["React"]);
    assert_eq!(
        env.dev_config.as_ref().unwrap().command,
        ["npm", "run", "dev"]
    );
    let browser = env.browser_config.unwrap();
    assert_eq!(browser.url.as_deref(), Some("http://localhost:5173"));
    assert!(browser.command.is_empty());

    s.write(
        "proj/package.json",
        r#"{"scripts": {"start": "ng serve"}, "dependencies": {"@angular/core": "^19.0.0"}}"#,
    );
    let env = node_environment(&s.search());
    assert_eq!(env.frameworks, ["Angular"]);
    assert_eq!(
        env.browser_config.unwrap().url.as_deref(),
        Some("http://localhost:4200")
    );
    s.write(
        "proj/package.json",
        r#"{"dependencies": {"next": "15.0.0", "react": "19.0.0"}}"#,
    );
    let env = node_environment(&s.search());
    assert_eq!(env.frameworks, ["Next.js", "React"]);
    assert!(env.dev_config.is_none(), "no dev script");
    s.write(
        "proj/package.json",
        r#"{"dependencies": {"lodash": "^4.0.0"}}"#,
    );
    assert!(node_environment(&s.search()).browser_config.is_none());
}
