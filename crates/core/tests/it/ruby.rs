//! Ruby runtimes, requested version and Bundler (`SPEC-0185` AK-1, AK-2, AK-4) with fake folders.

use std::path::{Path, PathBuf};

use ferris_idea_core::{RubySearch, read_bundle, ruby_environment};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ferris-idea-ruby-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn ruby_at(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("ruby");
    std::fs::write(&exe, "").unwrap();
    exe
}

struct Fake {
    base: PathBuf,
    root: PathBuf,
    path_dir: PathBuf,
}

impl Fake {
    fn new(name: &str) -> Self {
        let base = temp(name);
        let home = base.join("home");
        ruby_at(&home.join(".rbenv/versions/3.2.2"));
        ruby_at(&home.join(".rbenv/versions/3.2.10"));
        ruby_at(&home.join(".rbenv/versions/3.3.6"));
        ruby_at(&home.join(".rvm/rubies/ruby-3.1.4"));
        ruby_at(&home.join(".rubies/ruby-3.4.1"));
        ruby_at(&home.join(".asdf/installs/ruby/3.0.7"));
        ruby_at(&home.join(".local/share/mise/installs/ruby/3.4.1"));
        // A PATH ruby of its own, and one that is rbenv's 3.3.6: dropped as duplicate.
        let path_dir = base.join("usr/bin");
        std::fs::create_dir_all(&path_dir).unwrap();
        std::fs::write(path_dir.join("ruby"), "").unwrap();
        let shim_dir = base.join("shims");
        std::fs::create_dir_all(&shim_dir).unwrap();
        std::os::unix::fs::symlink(
            home.join(".rbenv/versions/3.3.6/bin/ruby"),
            shim_dir.join("ruby"),
        )
        .unwrap();
        let root = base.join("project");
        std::fs::create_dir_all(&root).unwrap();
        Self {
            base,
            root,
            path_dir,
        }
    }

    fn search(&self) -> RubySearch {
        let home = self.base.join("home");
        RubySearch {
            root: self.root.clone(),
            path_var: Some(
                std::env::join_paths([self.path_dir.clone(), self.base.join("shims")]).unwrap(),
            ),
            rbenv_root: Some(home.join(".rbenv")),
            rvm_path: Some(home.join(".rvm")),
            asdf_dir: Some(home.join(".asdf")),
            mise_dir: Some(home.join(".local/share/mise")),
            rubies_dirs: vec![self.base.join("opt/rubies")],
            gem_home: None,
            home: Some(home),
            configured: None,
        }
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

#[test]
fn runtimes_from_every_manager() {
    let f = Fake::new("managers");
    let env = ruby_environment(&f.search());
    let found: Vec<(&str, Option<&str>)> = env
        .runtimes
        .iter()
        .map(|r| (r.found_by.as_str(), r.version.as_deref()))
        .collect();
    assert_eq!(
        found,
        [
            ("rbenv", Some("3.3.6")),
            ("rbenv", Some("3.2.10")),
            ("rbenv", Some("3.2.2")),
            ("rvm", Some("3.1.4")),
            ("chruby", Some("3.4.1")),
            ("asdf", Some("3.0.7")),
            ("mise", Some("3.4.1")),
            ("PATH", None),
        ]
    );
    // No version file: the first ruby on PATH.
    assert_eq!(env.selected, Some(7));
    assert_eq!(env.reason.as_deref(), Some("first ruby on PATH"));
}

#[test]
fn selection_by_version_file() {
    let f = Fake::new("select");
    std::fs::write(f.root.join(".ruby-version"), "ruby-3.2\n").unwrap();
    let env = ruby_environment(&f.search());
    let r = env.selected_runtime().unwrap();
    assert_eq!(r.version.as_deref(), Some("3.2.10"));
    assert_eq!(env.reason.as_deref(), Some(".ruby-version asks for 3.2"));
    assert!(env.problems.is_empty(), "{:?}", env.problems);
    assert!(env.server_path[0].ends_with(".rbenv/versions/3.2.10/bin"));
    assert!(
        env.server_path
            .last()
            .unwrap()
            .ends_with(".gem/ruby/3.2.0/bin")
    );

    // A request nothing matches: problem, and the PATH ruby is used.
    std::fs::write(f.root.join(".ruby-version"), "2.7.8\n").unwrap();
    std::fs::write(f.root.join(".tool-versions"), "nodejs 20\nruby 3.3.6\n").unwrap();
    let env = ruby_environment(&f.search());
    assert_eq!(env.reason.as_deref(), Some("first ruby on PATH"));
    assert_eq!(
        env.problems,
        [
            ".ruby-version asks for 2.7.8 but .tool-versions for 3.3.6",
            ".ruby-version asks for Ruby 2.7.8 but no installed Ruby matches",
        ]
    );
    assert_eq!(env.requested.len(), 2);

    // A configured Ruby wins over everything.
    let mut search = f.search();
    search.configured = Some(f.base.join("home/.rvm/rubies/ruby-3.1.4"));
    let env = ruby_environment(&search);
    assert_eq!(env.reason.as_deref(), Some("chosen in the settings"));
    assert_eq!(
        env.selected_runtime().unwrap().version.as_deref(),
        Some("3.1.4")
    );
}

#[test]
fn mise_and_gemfile_requests() {
    let f = Fake::new("gemfile");
    std::fs::write(
        f.root.join("mise.toml"),
        "[env]\nruby = \"x\"\n[tools]\nruby = \"3.4\"\n",
    )
    .unwrap();
    std::fs::write(
        f.root.join("Gemfile"),
        "source \"https://rubygems.org\"\nruby \">= 3.1\", \"< 3.4\"\ngem \"rake\"\n",
    )
    .unwrap();
    let env = ruby_environment(&f.search());
    let req: Vec<(&str, &str)> = env
        .requested
        .iter()
        .map(|r| (r.file.as_str(), r.version.as_str()))
        .collect();
    assert_eq!(req, [("mise.toml", "3.4"), ("Gemfile", ">= 3.1, < 3.4")]);
    assert_eq!(
        env.problems,
        ["mise.toml asks for 3.4 but Gemfile for >= 3.1, < 3.4"]
    );
    // Newest 3.4.x: chruby and mise have the same version, the first found wins.
    assert_eq!(env.selected_runtime().unwrap().found_by, "chruby");
    assert_eq!(
        env.notes,
        ["Gemfile without Gemfile.lock: run bundle install"]
    );
}

#[test]
fn lockfile() {
    let root = temp("lock");
    std::fs::write(root.join("Gemfile"), "gem 'solargraph'\n").unwrap();
    std::fs::write(
        root.join("Gemfile.lock"),
        "GIT\n  remote: https://example.invalid/tool.git\n  revision: abc\n  specs:\n    tool (0.1.0)\n      rake\n\nPATH\n  remote: .\n  specs:\n    mygem (1.0.0)\n\nGEM\n  remote: https://rubygems.org/\n  specs:\n    nokogiri (1.16.0-arm64-darwin)\n      racc (~> 1.4)\n    rake (13.2.1)\n    solargraph (0.49.0)\n      thor (~> 1.0)\n\nPLATFORMS\n  arm64-darwin\n\nDEPENDENCIES\n  solargraph\n\nRUBY VERSION\n   ruby 3.3.6p108\n\nBUNDLED WITH\n   2.5.22\n",
    )
    .unwrap();
    let b = read_bundle(&root);
    assert!(b.gemfile && b.lockfile);
    let gems: Vec<(&str, &str)> = b
        .gems
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(
        gems,
        [
            ("mygem", "1.0.0"),
            ("nokogiri", "1.16.0-arm64-darwin"),
            ("rake", "13.2.1"),
            ("solargraph", "0.49.0"),
            ("tool", "0.1.0"),
        ]
    );
    assert_eq!(b.ruby.as_deref(), Some("3.3.6p108"));
    assert_eq!(b.bundler.as_deref(), Some("2.5.22"));
    let _ = std::fs::remove_dir_all(&root);
}
