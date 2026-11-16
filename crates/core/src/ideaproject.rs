//! Project files of the reference IDE in `.idea` (`SPEC-0049`), read only.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::Serialize;

use crate::buildtools::{FolderInfo, FolderRole};

/// Folder of the project files, relative to the project root.
pub const IDEA_DIR: &str = ".idea";

/// Files in `.idea` that are not read: personal state and database tool settings (behaviour 7);
/// of `workspace.xml` only the `RunManager` component is used (v3 behaviour 13).
const NOT_READ: &[&str] = &[
    "workspace.xml",
    "dataSources.xml",
    "dataSources.local.xml",
    "dataSources",
    "db-forest-config.xml",
    "sqldialects.xml",
    "shelf",
    "usage.statistics.xml",
];

/// Kind of a source folder (behaviour 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// Production sources.
    Source,
    /// Test sources.
    Test,
    /// Production resources.
    Resource,
    /// Test resources.
    TestResource,
}

/// A source folder inside a content root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceFolder {
    /// Folder, project-relative.
    pub dir: String,
    /// Kind.
    pub kind: SourceKind,
    /// Package prefix, if given.
    pub package_prefix: Option<String>,
}

/// A content root of a module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContentRoot {
    /// Folder, project-relative.
    pub dir: String,
    /// Source folders.
    pub sources: Vec<SourceFolder>,
    /// Excluded folders.
    pub excluded: Vec<String>,
}

/// SDK of a module (behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase", tag = "kind", content = "name")]
pub enum ModuleSdk {
    /// The project SDK.
    Inherited,
    /// A named SDK.
    Named(String),
    /// None given.
    None,
}

/// A dependency of a module (behaviour 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdeaDependency {
    /// `module`, `library` or `module-library`.
    pub kind: String,
    /// Module or library name (module libraries without a name use their first root).
    pub name: String,
    /// Library level (`project`, `application`) for `library`.
    pub level: Option<String>,
    /// Scope: compile (default), test, runtime, provided.
    pub scope: String,
    /// Whether it is exported to dependent modules.
    pub exported: bool,
    /// Class roots of a module library, project-relative.
    pub roots: Vec<String>,
}

/// One module from `modules.xml` and its module file (behaviour 2–4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdeaModule {
    /// Name (module file name without `.iml`).
    pub name: String,
    /// Module file, project-relative.
    pub file: String,
    /// Group, if given.
    pub group: Option<String>,
    /// Module type, e.g. `JAVA_MODULE`.
    pub module_type: Option<String>,
    /// Content roots.
    pub content_roots: Vec<ContentRoot>,
    /// SDK.
    pub sdk: ModuleSdk,
    /// Language level of the module, if it overrides the project's.
    pub language_level: Option<String>,
    /// Whether the compiler output is inherited from the project.
    pub inherit_output: bool,
    /// Own compiler output folders (production, test), project-relative.
    pub output: Option<String>,
    /// Own test output folder.
    pub test_output: Option<String>,
    /// Dependencies in order.
    pub dependencies: Vec<IdeaDependency>,
    /// Facets of the module, hints of the frameworks it uses (`SPEC-0049` v3 behaviour 14).
    pub facets: Vec<IdeaFacet>,
}

/// A facet from the `FacetManager` of a module file (`SPEC-0049` v3 behaviour 14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdeaFacet {
    /// Facet type, e.g. `Spring`, `web`, `jpa`, `Python`.
    #[serde(rename = "type")]
    pub facet_type: String,
    /// Its name.
    pub name: String,
}

/// A project library from `.idea/libraries` (behaviour 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdeaLibrary {
    /// Name.
    pub name: String,
    /// Class roots.
    pub classes: Vec<String>,
    /// Source roots.
    pub sources: Vec<String>,
}

/// A version control mapping from `vcs.xml` (behaviour 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VcsMapping {
    /// Folder, project-relative (`.` for the project).
    pub dir: String,
    /// Version control system, e.g. `Git` (empty means none).
    pub vcs: String,
}

/// What the `.idea` folder describes (behaviour 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdeaProject {
    /// Project name from `.idea/.name`, else the folder name.
    pub name: String,
    /// Project SDK name.
    pub sdk: Option<String>,
    /// Project SDK type, e.g. `JavaSDK`.
    pub sdk_type: Option<String>,
    /// Project language level.
    pub language_level: Option<String>,
    /// Project compiler output, project-relative.
    pub output: Option<String>,
    /// Modules in `modules.xml` order.
    pub modules: Vec<IdeaModule>,
    /// Project libraries.
    pub libraries: Vec<IdeaLibrary>,
    /// Version control mappings.
    pub vcs: Vec<VcsMapping>,
    /// Problems: malformed files and broken references (IMP-0024).
    pub problems: Vec<String>,
    /// Files in `.idea` that were deliberately not opened.
    pub not_read: Vec<String>,
    /// The folders the modules give a role in the Project tree (`SPEC-0049` v2 behaviours 10, 11).
    pub folders: Vec<IdeaFolder>,
    /// The project file of the file-based format, when the project is one (`SPEC-0049` v3
    /// behaviour 12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipr: Option<String>,
    /// Run configurations, translated or with the reason why not (`SPEC-0049` v3 behaviour 13).
    pub run_configurations: Vec<crate::idearun::IdeaRun>,
}

impl IdeaProject {
    /// The translated run configurations (`SPEC-0049` v3 behaviour 13), environment included.
    pub fn run_configs(&self) -> Vec<crate::run::RunConfig> {
        self.run_configurations
            .iter()
            .filter_map(|r| r.config.clone())
            .collect()
    }
}

/// The run configurations translated from the reference IDE's project files of `root`, none
/// without them (`SPEC-0049` v3 behaviour 13). Nothing runs and nothing is written.
pub fn idea_run_configs(root: &Path) -> Vec<crate::run::RunConfig> {
    read_idea_project(root)
        .map(|p| p.run_configs())
        .unwrap_or_default()
}

/// A folder the modules give a role (`SPEC-0049` v2 behaviour 11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdeaFolder {
    /// Folder, project-relative (`.` for the project).
    pub dir: String,
    /// Role in the Project tree.
    pub role: FolderRole,
    /// Module name shown in brackets, when it differs from the folder's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_name: Option<String>,
}

/// `rel` below `root` when it names an existing folder inside the project: no `..`, no macro left.
fn inside(root: &Path, rel: &str) -> Option<PathBuf> {
    if rel.contains('$') {
        return None;
    }
    let mut out = root.to_path_buf();
    for c in Path::new(rel).components() {
        match c {
            Component::CurDir => {}
            Component::Normal(s) => out.push(s),
            _ => return None,
        }
    }
    out.is_dir().then_some(out)
}

/// The roles the modules of `project` give the folders of `root` (`SPEC-0049` v2 behaviour 10):
/// content roots are module folders, source folders by their kind, excluded folders and the
/// compiler output excluded. Only existing folders below the project; absolute paths.
pub fn folder_roles(root: &Path, project: &IdeaProject) -> BTreeMap<PathBuf, FolderInfo> {
    let mut out = BTreeMap::new();
    let plain = |role| FolderInfo {
        role,
        module_name: None,
    };
    let mut outputs: Vec<&String> = project.output.iter().collect();
    for m in &project.modules {
        for c in &m.content_roots {
            for s in &c.sources {
                let role = match s.kind {
                    SourceKind::Source => FolderRole::Sources,
                    SourceKind::Test => FolderRole::Tests,
                    SourceKind::Resource => FolderRole::Resources,
                    SourceKind::TestResource => FolderRole::TestResources,
                };
                if let Some(dir) = inside(root, &s.dir) {
                    out.entry(dir).or_insert(plain(role));
                }
            }
            for e in &c.excluded {
                if let Some(dir) = inside(root, e) {
                    out.entry(dir).or_insert(plain(FolderRole::Excluded));
                }
            }
        }
        if !m.inherit_output {
            outputs.extend(m.output.iter().chain(m.test_output.iter()));
        }
    }
    for o in outputs {
        if let Some(dir) = inside(root, o).filter(|d| d != root) {
            out.entry(dir).or_insert(plain(FolderRole::Excluded));
        }
    }
    for m in &project.modules {
        for c in &m.content_roots {
            let Some(dir) = inside(root, &c.dir) else {
                continue;
            };
            let folder_name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.entry(dir).or_insert(FolderInfo {
                role: FolderRole::Module,
                module_name: (m.name != folder_name).then(|| m.name.clone()),
            });
        }
    }
    out
}

/// Minimal element tree.
#[derive(Debug, Default)]
pub(crate) struct Element {
    pub(crate) name: String,
    attrs: Vec<(String, String)>,
    pub(crate) children: Vec<Element>,
}

impl Element {
    pub(crate) fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub(crate) fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    pub(crate) fn first(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    /// The `component` child with the given `name` attribute.
    pub(crate) fn component(&self, name: &str) -> Option<&Element> {
        self.all("component").find(|c| c.attr("name") == Some(name))
    }
}

fn element(start: &BytesStart) -> Result<Element, String> {
    let mut e = Element {
        name: String::from_utf8_lossy(start.name().as_ref()).into_owned(),
        ..Element::default()
    };
    for a in start.attributes() {
        let a = a.map_err(|e| e.to_string())?;
        let value = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| e.to_string())?;
        e.attrs.push((
            String::from_utf8_lossy(a.key.as_ref()).into_owned(),
            value.into_owned(),
        ));
    }
    Ok(e)
}

fn parse_xml(text: &str) -> Result<Element, String> {
    let mut reader = Reader::from_str(text);
    let mut stack = vec![Element::default()];
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(s) => stack.push(element(&s)?),
            Event::Empty(s) => {
                let e = element(&s)?;
                stack.last_mut().expect("root").children.push(e);
            }
            Event::End(_) => {
                let done = stack.pop().expect("open element");
                match stack.last_mut() {
                    Some(parent) => parent.children.push(done),
                    None => return Err("unbalanced end tag".to_owned()),
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 {
        return Err("unexpected end of file".to_owned());
    }
    let doc = stack.pop().expect("document");
    doc.children
        .into_iter()
        .next()
        .ok_or_else(|| "empty document".to_owned())
}

/// Lexically joins `rel` to the project-relative folder `base` and normalises it, keeping `..`.
fn join_rel(base: &str, rel: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for c in Path::new(base).join(rel).components() {
        match c {
            Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
            Component::ParentDir => {
                if parts.last().is_some_and(|p| p != "..") {
                    parts.pop();
                } else {
                    parts.push("..".to_owned());
                }
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

/// Resolves a `file://`/`jar://` URL or plain path with `$PROJECT_DIR$`/`$MODULE_DIR$` to a
/// project-relative path; other macros (e.g. `$USER_HOME$`) are kept as written.
pub(crate) fn resolve(url: &str, module_dir: &str) -> String {
    let path = url
        .strip_prefix("file://")
        .or_else(|| url.strip_prefix("jar://"))
        .unwrap_or(url);
    let path = path.strip_suffix("!/").unwrap_or(path);
    for (macro_name, base) in [("$PROJECT_DIR$", "."), ("$MODULE_DIR$", module_dir)] {
        if let Some(rest) = path.strip_prefix(macro_name) {
            return join_rel(base, rest.trim_start_matches('/'));
        }
    }
    path.to_owned()
}

/// `$MODULE_DIR$` of a module file: its folder, or the project folder when the file lies directly
/// in `.idea` (behaviour 2).
fn module_dir(rel: &str) -> String {
    let parent = Path::new(rel).parent().unwrap_or(Path::new(""));
    let parent = if parent.file_name().is_some_and(|n| n == IDEA_DIR) {
        parent.parent().unwrap_or(Path::new(""))
    } else {
        parent
    };
    if parent.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        parent.to_string_lossy().into_owned()
    }
}

fn read_xml(root: &Path, rel: &str, problems: &mut Vec<String>) -> Option<Element> {
    let text = match std::fs::read_to_string(root.join(rel)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            problems.push(format!("{rel}: {e}"));
            return None;
        }
    };
    match parse_xml(&text) {
        Ok(e) => Some(e),
        Err(e) => {
            problems.push(format!("{rel}: {e}"));
            None
        }
    }
}

fn roots(e: Option<&Element>, module_dir: &str) -> Vec<String> {
    e.map(|e| {
        e.all("root")
            .filter_map(|r| r.attr("url"))
            .map(|u| resolve(u, module_dir))
            .collect()
    })
    .unwrap_or_default()
}

fn read_module(
    root: &Path,
    file: &str,
    group: Option<String>,
    problems: &mut Vec<String>,
) -> IdeaModule {
    let name = Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.to_owned());
    let mut module = IdeaModule {
        name,
        file: file.to_owned(),
        group,
        module_type: None,
        content_roots: Vec::new(),
        sdk: ModuleSdk::None,
        language_level: None,
        inherit_output: false,
        output: None,
        test_output: None,
        dependencies: Vec::new(),
        facets: Vec::new(),
    };
    if !root.join(file).is_file() {
        problems.push(format!(
            "{file}: module file listed in modules.xml is missing"
        ));
        return module;
    }
    let Some(doc) = read_xml(root, file, problems) else {
        return module;
    };
    let dir = module_dir(file);
    module.module_type = doc.attr("type").map(str::to_owned);
    if let Some(facets) = doc.component("FacetManager") {
        module.facets = facets
            .all("facet")
            .filter_map(|f| {
                Some(IdeaFacet {
                    facet_type: f.attr("type")?.to_owned(),
                    name: f.attr("name").unwrap_or_default().to_owned(),
                })
            })
            .collect();
    }
    let Some(manager) = doc.component("NewModuleRootManager") else {
        return module;
    };
    module.language_level = manager.attr("LANGUAGE_LEVEL").map(str::to_owned);
    module.inherit_output = manager.attr("inherit-compiler-output") == Some("true");
    module.output = manager
        .first("output")
        .and_then(|o| o.attr("url"))
        .map(|u| resolve(u, &dir));
    module.test_output = manager
        .first("output-test")
        .and_then(|o| o.attr("url"))
        .map(|u| resolve(u, &dir));
    for content in manager.all("content") {
        let Some(url) = content.attr("url") else {
            continue;
        };
        let sources = content
            .all("sourceFolder")
            .filter_map(|s| {
                let kind = match (s.attr("type"), s.attr("isTestSource")) {
                    (Some("java-resource"), _) => SourceKind::Resource,
                    (Some("java-test-resource"), _) => SourceKind::TestResource,
                    (_, Some("true")) => SourceKind::Test,
                    _ => SourceKind::Source,
                };
                Some(SourceFolder {
                    dir: resolve(s.attr("url")?, &dir),
                    kind,
                    package_prefix: s.attr("packagePrefix").map(str::to_owned),
                })
            })
            .collect();
        let excluded = content
            .all("excludeFolder")
            .filter_map(|e| e.attr("url"))
            .map(|u| resolve(u, &dir))
            .collect();
        module.content_roots.push(ContentRoot {
            dir: resolve(url, &dir),
            sources,
            excluded,
        });
    }
    for entry in manager.all("orderEntry") {
        let scope = entry.attr("scope").unwrap_or("COMPILE").to_lowercase();
        let exported = entry.attr("exported").is_some();
        match entry.attr("type") {
            Some("inheritedJdk") => module.sdk = ModuleSdk::Inherited,
            Some("jdk") => {
                module.sdk = ModuleSdk::Named(entry.attr("jdkName").unwrap_or_default().to_owned())
            }
            Some("module") => module.dependencies.push(IdeaDependency {
                kind: "module".to_owned(),
                name: entry.attr("module-name").unwrap_or_default().to_owned(),
                level: None,
                scope,
                exported,
                roots: Vec::new(),
            }),
            Some("library") => module.dependencies.push(IdeaDependency {
                kind: "library".to_owned(),
                name: entry.attr("name").unwrap_or_default().to_owned(),
                level: entry.attr("level").map(str::to_owned),
                scope,
                exported,
                roots: Vec::new(),
            }),
            Some("module-library") => {
                let library = entry.first("library");
                let classes = roots(library.and_then(|l| l.first("CLASSES")), &dir);
                let name = library
                    .and_then(|l| l.attr("name"))
                    .map(str::to_owned)
                    .or_else(|| classes.first().cloned())
                    .unwrap_or_default();
                module.dependencies.push(IdeaDependency {
                    kind: "module-library".to_owned(),
                    name,
                    level: None,
                    scope,
                    exported,
                    roots: classes,
                });
            }
            _ => {}
        }
    }
    module
}

/// Reads the `.idea` folder of `root`, or without one its `.ipr` (v3 behaviour 12); `None` when
/// there is neither (behaviour 1).
pub fn read_idea_project(root: &Path) -> Option<IdeaProject> {
    let idea: PathBuf = root.join(IDEA_DIR);
    let mut problems = Vec::new();
    // The file-based format (v3 behaviour 12): `<name>.ipr`, only without `.idea`.
    let ipr: Option<String> = if idea.is_dir() {
        None
    } else {
        let mut iprs: Vec<String> = std::fs::read_dir(root)
            .ok()?
            .flatten()
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".ipr"))
            .collect();
        iprs.sort();
        let first = iprs.first()?.clone();
        for other in &iprs[1..] {
            problems.push(format!("{other}: another project file; {first} is read"));
        }
        Some(first)
    };
    let ipr_doc = ipr
        .as_deref()
        .and_then(|f| read_xml(root, f, &mut problems));
    let name = ipr
        .as_deref()
        .and_then(|f| f.strip_suffix(".ipr"))
        .map(str::to_owned)
        .or_else(|| {
            std::fs::read_to_string(idea.join(".name"))
                .ok()
                .map(|s| s.trim().to_owned())
        })
        .filter(|s| !s.is_empty())
        .or_else(|| {
            root.canonicalize()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        })
        .unwrap_or_default();
    let mut project = IdeaProject {
        name,
        sdk: None,
        sdk_type: None,
        language_level: None,
        output: None,
        modules: Vec::new(),
        libraries: Vec::new(),
        vcs: Vec::new(),
        problems: Vec::new(),
        not_read: Vec::new(),
        folders: Vec::new(),
        ipr: ipr.clone(),
        run_configurations: Vec::new(),
    };
    // A component of the project: from the `.ipr`, or from its own file in `.idea`.
    let own = |rel: &str, problems: &mut Vec<String>| {
        if ipr.is_some() {
            None
        } else {
            read_xml(root, rel, problems)
        }
    };
    let misc_doc = own(".idea/misc.xml", &mut problems);
    if let Some(prm) = ipr_doc
        .as_ref()
        .or(misc_doc.as_ref())
        .and_then(|d| d.component("ProjectRootManager"))
    {
        project.sdk = prm.attr("project-jdk-name").map(str::to_owned);
        project.sdk_type = prm.attr("project-jdk-type").map(str::to_owned);
        project.language_level = prm.attr("languageLevel").map(str::to_owned);
        project.output = prm
            .first("output")
            .and_then(|o| o.attr("url"))
            .map(|u| resolve(u, "."));
    }

    let modules_doc = own(".idea/modules.xml", &mut problems);
    if let Some(modules) = ipr_doc
        .as_ref()
        .or(modules_doc.as_ref())
        .and_then(|d| d.component("ProjectModuleManager"))
        .and_then(|c| c.first("modules"))
    {
        for m in modules.all("module") {
            let Some(path) = m.attr("filepath").or_else(|| m.attr("fileurl")) else {
                continue;
            };
            let file = resolve(path, ".");
            let group = m.attr("group").map(str::to_owned);
            project
                .modules
                .push(read_module(root, &file, group, &mut problems));
        }
    }

    let mut library_docs: Vec<Element> = Vec::new();
    if ipr.is_none()
        && let Ok(entries) = std::fs::read_dir(idea.join("libraries"))
    {
        let mut files: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".xml"))
            .collect();
        files.sort();
        for f in files {
            if let Some(doc) = read_xml(root, &format!(".idea/libraries/{f}"), &mut problems) {
                library_docs.push(doc);
            }
        }
    }
    let libraries = library_docs
        .iter()
        .chain(ipr_doc.as_ref().and_then(|d| d.component("libraryTable")));
    for doc in libraries {
        for lib in doc.all("library") {
            project.libraries.push(IdeaLibrary {
                name: lib.attr("name").unwrap_or_default().to_owned(),
                classes: roots(lib.first("CLASSES"), "."),
                sources: roots(lib.first("SOURCES"), "."),
            });
        }
    }

    let vcs_doc = own(".idea/vcs.xml", &mut problems);
    if let Some(c) = ipr_doc
        .as_ref()
        .or(vcs_doc.as_ref())
        .and_then(|d| d.component("VcsDirectoryMappings"))
    {
        for m in c.all("mapping") {
            let dir = m.attr("directory").unwrap_or_default();
            project.vcs.push(VcsMapping {
                dir: if dir.is_empty() {
                    ".".to_owned()
                } else {
                    resolve(dir, ".")
                },
                vcs: m.attr("vcs").unwrap_or_default().to_owned(),
            });
        }
    }

    // Broken references (IMP-0024).
    let module_names: Vec<&str> = project.modules.iter().map(|m| m.name.as_str()).collect();
    for m in &project.modules {
        for d in &m.dependencies {
            if d.kind == "module" && !module_names.contains(&d.name.as_str()) {
                problems.push(format!("{}: depends on unknown module {}", m.file, d.name));
            }
            if d.kind == "library"
                && d.level.as_deref() == Some("project")
                && !project.libraries.iter().any(|l| l.name == d.name)
            {
                problems.push(format!(
                    "{}: depends on unknown project library {}",
                    m.file, d.name
                ));
            }
        }
    }

    // Run configurations (v3 behaviour 13): shared ones first, then the personal `RunManager`
    // of `workspace.xml` or the `.iws`; nothing else of the personal state is used.
    let mut run_sources: Vec<(String, Element)> = Vec::new();
    match &ipr {
        Some(file) => {
            if let Some(doc) = ipr_doc {
                run_sources.push((file.clone(), doc));
            }
            let iws = format!("{}.iws", file.trim_end_matches(".ipr"));
            if let Some(doc) = read_xml(root, &iws, &mut problems) {
                run_sources.push((iws, doc));
            }
        }
        None => {
            if let Ok(entries) = std::fs::read_dir(idea.join("runConfigurations")) {
                let mut files: Vec<String> = entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.ends_with(".xml"))
                    .collect();
                files.sort();
                for f in files {
                    let rel = format!(".idea/runConfigurations/{f}");
                    if let Some(doc) = read_xml(root, &rel, &mut problems) {
                        run_sources.push((rel, doc));
                    }
                }
            }
            let rel = ".idea/workspace.xml".to_owned();
            if let Some(doc) = read_xml(root, &rel, &mut problems) {
                run_sources.push((rel, doc));
            }
        }
    }
    // Personal state counts only with its `RunManager` (behaviour 7).
    for (file, doc) in &mut run_sources {
        if file.ends_with("workspace.xml") || file.ends_with(".iws") {
            let manager = std::mem::take(&mut doc.children)
                .into_iter()
                .filter(|c| c.name == "component" && c.attr("name") == Some("RunManager"))
                .collect();
            doc.children = manager;
        }
    }
    project.run_configurations =
        crate::idearun::read_runs(root, &project, &run_sources, &mut problems);
    project.problems = problems;

    if ipr.is_none()
        && let Ok(entries) = std::fs::read_dir(&idea)
    {
        let mut skipped: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| NOT_READ.contains(&n.as_str()))
            .collect();
        skipped.sort();
        project.not_read = skipped;
    }
    project.folders = folder_roles(root, &project)
        .into_iter()
        .map(|(dir, info)| IdeaFolder {
            dir: dir
                .strip_prefix(root)
                .ok()
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| ".".to_owned()),
            role: info.role,
            module_name: info.module_name,
        })
        .collect();
    Some(project)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_resolve_to_project_relative_paths() {
        assert_eq!(resolve("file://$MODULE_DIR$", ".idea"), ".idea");
        assert_eq!(resolve("file://$MODULE_DIR$/../src", ".idea"), "src");
        assert_eq!(resolve("file://$PROJECT_DIR$/out", ".idea"), "out");
        assert_eq!(resolve("$PROJECT_DIR$/../../..", "."), "../../..");
        assert_eq!(resolve("$PROJECT_DIR$", "."), ".");
        assert_eq!(module_dir(".idea/a.iml"), ".");
        assert_eq!(module_dir("core/core.iml"), "core");
        assert_eq!(module_dir("a.iml"), ".");
        assert_eq!(resolve("jar://$PROJECT_DIR$/lib/a.jar!/", "."), "lib/a.jar");
        assert_eq!(
            resolve("jar://$MAVEN_REPOSITORY$/x.jar!/", "."),
            "$MAVEN_REPOSITORY$/x.jar"
        );
    }
}
