use crate::scanner::metadata::is_binary_file;
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    Source,
    Config,
    Documentation,
    Asset,
    Generated,
    Dependency,
    Binary,
    Unknown,
}

impl ResourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceKind::Source => "source",
            ResourceKind::Config => "config",
            ResourceKind::Documentation => "documentation",
            ResourceKind::Asset => "asset",
            ResourceKind::Generated => "generated",
            ResourceKind::Dependency => "dependency",
            ResourceKind::Binary => "binary",
            ResourceKind::Unknown => "unknown",
        }
    }
}

/// Directories holding third-party code. Matched case-sensitively, since these
/// names are conventions of the tools that create them.
const DEPENDENCY_DIRS: &[&str] = &[
    "node_modules",
    "bower_components",
    "vendor",
    ".venv",
    "venv",
];

/// Directories holding build output. `bin` is handled separately because it
/// commonly holds hand-written scripts.
const GENERATED_DIRS: &[&str] = &[
    "target",
    "dist",
    "build",
    "out",
    "obj",
    ".next",
    ".nuxt",
    ".output",
    "__pycache__",
    "coverage",
];

/// Configuration build outputs (`bin/Debug`, `bin/Release`) used by .NET and similar toolchains.
const BUILD_CONFIGURATION_DIRS: &[&str] = &["Debug", "Release", "x64", "x86"];

/// Classifies a resource. The path decides first; file content is only
/// sampled (for binary detection) when the path gives no answer.
pub fn classify_resource(rel_path: &Path, abs_path: &Path) -> (String, Option<String>) {
    let (kind, lang) = classify_path(rel_path);
    if kind != ResourceKind::Unknown {
        return (kind.as_str().to_string(), lang.map(str::to_string));
    }
    if is_binary_file(abs_path).unwrap_or(false) {
        (ResourceKind::Binary.as_str().to_string(), None)
    } else {
        (ResourceKind::Unknown.as_str().to_string(), None)
    }
}

/// Path-only classification, without reading the file.
pub fn classify_path(rel_path: &Path) -> (ResourceKind, Option<&'static str>) {
    let lang = language_for(rel_path);

    let dirs: Vec<&str> = rel_path
        .parent()
        .map(|p| {
            p.components()
                .filter_map(|c| match c {
                    Component::Normal(s) => s.to_str(),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    for (i, dir) in dirs.iter().enumerate() {
        if DEPENDENCY_DIRS.contains(dir) {
            return (ResourceKind::Dependency, lang);
        }
        if GENERATED_DIRS.contains(dir) {
            return (ResourceKind::Generated, lang);
        }
        if *dir == "bin"
            && dirs
                .get(i + 1)
                .is_some_and(|next| BUILD_CONFIGURATION_DIRS.contains(next))
        {
            return (ResourceKind::Generated, lang);
        }
    }

    let file_name = rel_path.file_name().and_then(|f| f.to_str()).unwrap_or("");
    if let Some(found) = classify_file_name(file_name) {
        return found;
    }

    let ext = extension_of(rel_path);
    if let Some(kind) = kind_for_extension(&ext) {
        return (kind, lang);
    }
    (ResourceKind::Unknown, None)
}

fn classify_file_name(name: &str) -> Option<(ResourceKind, Option<&'static str>)> {
    let lower = name.to_lowercase();
    let found = match lower.as_str() {
        "cargo.lock" | "cargo.toml" => (ResourceKind::Config, Some("toml")),
        "package.json" | "package-lock.json" | "tsconfig.json" | "composer.json" => {
            (ResourceKind::Config, Some("json"))
        }
        "makefile" | "gnumakefile" => (ResourceKind::Config, Some("make")),
        "dockerfile" | "containerfile" => (ResourceKind::Config, Some("dockerfile")),
        "cmakelists.txt" => (ResourceKind::Config, Some("cmake")),
        "gemfile" | "rakefile" | "podfile" => (ResourceKind::Config, Some("ruby")),
        "jenkinsfile" | "procfile" | "yarn.lock" | "pnpm-lock.yaml" | "go.mod" | "go.sum"
        | "requirements.txt" | "pipfile" | "pipfile.lock" | "poetry.lock" => {
            (ResourceKind::Config, None)
        }
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".editorconfig" | ".inspecteyeignore"
        | ".dockerignore" | ".npmrc" | ".nvmrc" | ".prettierrc" | ".eslintrc" | ".babelrc" => {
            (ResourceKind::Config, None)
        }
        "license" | "licence" | "copying" | "notice" | "authors" | "readme" | "changelog" => {
            (ResourceKind::Documentation, None)
        }
        _ if lower == ".env" || lower.starts_with(".env.") => {
            (ResourceKind::Config, Some("dotenv"))
        }
        _ => return None,
    };
    Some(found)
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

fn kind_for_extension(ext: &str) -> Option<ResourceKind> {
    let kind = match ext {
        "exe" | "dll" | "so" | "dylib" | "bin" | "o" | "obj" | "a" | "lib" | "wasm" | "class"
        | "pyc" | "pyo" | "pdb" | "jar" => ResourceKind::Binary,

        "json" | "jsonc" | "toml" | "yaml" | "yml" | "xml" | "ini" | "cfg" | "conf" | "env"
        | "properties" | "lock" | "csproj" | "fsproj" | "vbproj" | "sln" | "props" | "targets"
        | "gradle" | "editorconfig" => ResourceKind::Config,

        "md" | "markdown" | "txt" | "rst" | "adoc" => ResourceKind::Documentation,

        "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" | "webp" | "bmp" | "ttf" | "otf"
        | "woff" | "woff2" | "eot" | "mp3" | "wav" | "ogg" | "mp4" | "webm" | "pdf" => {
            ResourceKind::Asset
        }

        _ if language_for_extension(ext).is_some() => ResourceKind::Source,
        _ => return None,
    };
    Some(kind)
}

/// Language of a resource, derived from its name. Used for source and config files.
pub fn language_for(path: &Path) -> Option<&'static str> {
    let ext = extension_of(path);
    if let Some(lang) = language_for_extension(&ext) {
        return Some(lang);
    }
    match ext.as_str() {
        "json" | "jsonc" => Some("json"),
        "toml" => Some("toml"),
        "yaml" | "yml" => Some("yaml"),
        "xml" | "csproj" | "fsproj" | "vbproj" | "props" | "targets" => Some("xml"),
        "md" | "markdown" => Some("markdown"),
        _ => None,
    }
}

fn language_for_extension(ext: &str) -> Option<&'static str> {
    let lang = match ext {
        "rs" => "rust",
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "cs" => "csharp",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" | "cxx" | "hh" | "hxx" => "cpp",
        "py" | "pyi" => "python",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "scala" => "scala",
        "rb" => "ruby",
        "php" => "php",
        "lua" => "lua",
        "dart" => "dart",
        "vue" => "vue",
        "svelte" => "svelte",
        "html" | "htm" => "html",
        "css" | "scss" | "sass" | "less" => "css",
        "sql" => "sql",
        "sh" | "bash" | "zsh" => "shell",
        "ps1" | "psm1" => "powershell",
        "bat" | "cmd" => "batch",
        _ => return None,
    };
    Some(lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(path: &str) -> ResourceKind {
        classify_path(Path::new(path)).0
    }

    #[test]
    fn test_classify_resource_types() {
        assert_eq!(kind("src/main.rs"), ResourceKind::Source);
        assert_eq!(kind("package.json"), ResourceKind::Config);
        assert_eq!(kind("README.md"), ResourceKind::Documentation);
        assert_eq!(kind("assets/logo.svg"), ResourceKind::Asset);
        assert_eq!(
            kind("node_modules/lodash/index.js"),
            ResourceKind::Dependency
        );
        assert_eq!(
            kind("vendor/bundle/gems/rails.rb"),
            ResourceKind::Dependency
        );
        assert_eq!(kind("dist/app.bundle.js"), ResourceKind::Generated);
        assert_eq!(kind("target/debug/build.log"), ResourceKind::Generated);
        assert_eq!(kind("native/driver.dll"), ResourceKind::Binary);
    }

    #[test]
    fn test_first_party_directories_are_not_misclassified() {
        // Monorepo package folders are first-party code.
        assert_eq!(kind("packages/core/src/index.ts"), ResourceKind::Source);
        // Directory conventions are case-sensitive.
        assert_eq!(kind("Build/Tasks.cs"), ResourceKind::Source);
        // Scripts in bin/ are hand-written; bin/Debug is build output.
        assert_eq!(kind("bin/run.sh"), ResourceKind::Source);
        assert_eq!(kind("src/App/bin/Debug/App.dll"), ResourceKind::Generated);
        // A file named like a generated dir is not inside one.
        assert_eq!(kind("src/build.rs"), ResourceKind::Source);
    }

    #[test]
    fn test_well_known_file_names() {
        assert_eq!(kind("Makefile"), ResourceKind::Config);
        assert_eq!(kind("docker/Dockerfile"), ResourceKind::Config);
        assert_eq!(kind(".env"), ResourceKind::Config);
        assert_eq!(kind(".env.local"), ResourceKind::Config);
        assert_eq!(kind("CMakeLists.txt"), ResourceKind::Config);
        assert_eq!(kind("LICENSE"), ResourceKind::Documentation);
        assert_eq!(kind("App.csproj"), ResourceKind::Config);
    }

    #[test]
    fn test_language_detection() {
        assert_eq!(classify_path(Path::new("a.tsx")).1, Some("typescript"));
        assert_eq!(classify_path(Path::new("a.kt")).1, Some("kotlin"));
        assert_eq!(classify_path(Path::new("a.json")).1, Some("json"));
        assert_eq!(classify_path(Path::new("logo.png")).1, None);
    }
}
