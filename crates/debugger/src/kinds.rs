//! Which debugger runs a program (`SPEC-0091` v2 behaviour 2).

/// Which debugger runs the program (`SPEC-0091` v2 behaviour 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A native program through lldb-dap.
    Native,
    /// A Python script through debugpy.
    Python,
    /// A Java main class through java-debug in jdtls (`SPEC-0097` behaviour 6); the launch's
    /// `program` holds the class name.
    Java,
    /// A Node.js script through js-debug (`SPEC-0091` v4 behaviour 10).
    Node,
    /// A `package.json` script through its package manager and js-debug (`SPEC-0168` behaviour 1);
    /// the launch's `program` is the package manager, `args` start with `run`.
    PackageScript,
    /// A page in Chrome through js-debug (`SPEC-0133` behaviour 2); the launch's `program` holds
    /// the URL, `cwd` the folder its sources map to.
    Browser,
    /// A Kotlin main class of a Gradle or Maven project through kotlin-debug-adapter (`SPEC-0132`
    /// behaviour 2); the launch's `program` holds the class name, `prepare` the build command.
    Kotlin,
    /// A Go package, file or test through Delve (`SPEC-0039` v7 behaviour 11); the launch's
    /// `program` is the package folder or file, `args` the words after `go` (`run …`, `test …`).
    Go,
}

impl Kind {
    /// Name of the adapter in `crate::known_adapters`.
    pub fn adapter_name(self) -> &'static str {
        match self {
            Kind::Native => "lldb-dap",
            Kind::Python => "debugpy",
            Kind::Java => "java-debug",
            Kind::Node | Kind::PackageScript | Kind::Browser => "js-debug",
            Kind::Kotlin => "kotlin-debug-adapter",
            Kind::Go => "dlv",
        }
    }
}

impl Kind {
    /// The runtime of an attachable kind (`SPEC-0147`); Kotlin attaches as Java (JDWP).
    pub fn runtime(self) -> Option<crate::Runtime> {
        use crate::Runtime;
        match self {
            Kind::Native => Some(Runtime::Native),
            Kind::Python => Some(Runtime::Python),
            Kind::Node | Kind::PackageScript => Some(Runtime::Node),
            Kind::Java | Kind::Kotlin => Some(Runtime::Java),
            Kind::Go => Some(Runtime::Go),
            Kind::Browser => None,
        }
    }

    /// The kind that debugs `runtime`.
    pub fn of_runtime(runtime: crate::Runtime) -> Self {
        use crate::Runtime;
        match runtime {
            Runtime::Native => Kind::Native,
            Runtime::Python => Kind::Python,
            Runtime::Node => Kind::Node,
            Runtime::Java => Kind::Java,
            Runtime::Go => Kind::Go,
        }
    }
}

impl Kind {
    /// The kind's name in the session protocol (`SPEC-0322` behaviour 6).
    pub fn wire_name(self) -> &'static str {
        match self {
            Kind::Native => "native",
            Kind::Python => "python",
            Kind::Java => "java",
            Kind::Node => "node",
            Kind::PackageScript => "package_script",
            Kind::Browser => "browser",
            Kind::Kotlin => "kotlin",
            Kind::Go => "go",
        }
    }

    /// The kind called `name` in the session protocol.
    pub fn from_wire(name: &str) -> Option<Self> {
        [
            Kind::Native,
            Kind::Python,
            Kind::Java,
            Kind::Node,
            Kind::PackageScript,
            Kind::Browser,
            Kind::Kotlin,
            Kind::Go,
        ]
        .into_iter()
        .find(|k| k.wire_name() == name)
    }
}
