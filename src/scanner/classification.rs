use crate::scanner::metadata::is_binary_file;
use std::path::Path;

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

pub fn classify_resource(rel_path: &Path, abs_path: &Path) -> (String, Option<String>) {
    // 1. Check directory components for dependency or generated directories
    for component in rel_path.components() {
        if let std::path::Component::Normal(c) = component {
            let seg = c.to_string_lossy().to_lowercase();
            match seg.as_str() {
                "node_modules" | "vendor" | "bower_components" | "packages" => {
                    let (_, lang) = detect_extension_language(rel_path);
                    return ("dependency".to_string(), lang);
                }
                "target" | "dist" | "build" | "out" | "bin" | "obj" | ".next" | ".nuxt"
                | ".output" => {
                    let (_, lang) = detect_extension_language(rel_path);
                    return ("generated".to_string(), lang);
                }
                _ => {}
            }
        }
    }

    // 2. Check for known binary executable / library extensions
    let ext = rel_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "exe" | "dll" | "so" | "dylib" | "bin" | "o" | "obj" | "a" | "lib" | "wasm" | "class"
        | "pyc" | "pyo" => return ("binary".to_string(), None),
        _ => {}
    }

    // 3. Check special filenames
    let file_name = rel_path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("")
        .to_lowercase();

    match file_name.as_str() {
        "cargo.toml" | "cargo.lock" => return ("config".to_string(), Some("toml".to_string())),
        "package.json" | "package-lock.json" | "tsconfig.json" => {
            return ("config".to_string(), Some("json".to_string()))
        }
        "readme.md" | "license" | "contributing.md" | "agents.md" | "changelog.md" => {
            return ("documentation".to_string(), Some("markdown".to_string()))
        }
        ".gitignore" | ".gitattributes" | ".editorconfig" | ".inspecteyeignore" => {
            return ("config".to_string(), None)
        }
        _ => {}
    }

    // 4. Extension-based classification
    let (kind, lang) = detect_extension_language(rel_path);
    if kind != "unknown" {
        return (kind, lang);
    }

    // 5. Inspect file content for binary detection
    if is_binary_file(abs_path).unwrap_or(false) {
        ("binary".to_string(), None)
    } else {
        ("unknown".to_string(), None)
    }
}

fn detect_extension_language(path: &Path) -> (String, Option<String>) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "rs" => ("source".to_string(), Some("rust".to_string())),
        "ts" | "tsx" => ("source".to_string(), Some("typescript".to_string())),
        "js" | "jsx" | "mjs" | "cjs" => ("source".to_string(), Some("javascript".to_string())),
        "cs" => ("source".to_string(), Some("csharp".to_string())),
        "c" | "h" => ("source".to_string(), Some("c".to_string())),
        "cpp" | "hpp" | "cc" | "cxx" => ("source".to_string(), Some("cpp".to_string())),
        "py" => ("source".to_string(), Some("python".to_string())),
        "go" => ("source".to_string(), Some("go".to_string())),
        "java" => ("source".to_string(), Some("java".to_string())),
        "rb" => ("source".to_string(), Some("ruby".to_string())),
        "php" => ("source".to_string(), Some("php".to_string())),
        "html" | "htm" => ("source".to_string(), Some("html".to_string())),
        "css" | "scss" | "sass" | "less" => ("source".to_string(), Some("css".to_string())),
        "sql" => ("source".to_string(), Some("sql".to_string())),
        "sh" | "bash" | "zsh" => ("source".to_string(), Some("shell".to_string())),
        "ps1" => ("source".to_string(), Some("powershell".to_string())),
        "bat" | "cmd" => ("source".to_string(), Some("batch".to_string())),

        "json" => ("config".to_string(), Some("json".to_string())),
        "toml" => ("config".to_string(), Some("toml".to_string())),
        "yaml" | "yml" => ("config".to_string(), Some("yaml".to_string())),
        "xml" => ("config".to_string(), Some("xml".to_string())),
        "ini" | "cfg" | "conf" | "env" | "properties" => ("config".to_string(), None),

        "md" | "markdown" | "txt" | "rst" | "adoc" => {
            ("documentation".to_string(), Some("markdown".to_string()))
        }

        "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" | "webp" | "bmp" => {
            ("asset".to_string(), None)
        }
        "ttf" | "woff" | "woff2" | "eot" => ("asset".to_string(), None),
        "mp3" | "wav" | "ogg" | "mp4" | "webm" => ("asset".to_string(), None),

        _ => ("unknown".to_string(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_resource_types() {
        let dummy = Path::new("dummy");

        assert_eq!(
            classify_resource(Path::new("src/main.rs"), dummy).0,
            "source"
        );
        assert_eq!(
            classify_resource(Path::new("package.json"), dummy).0,
            "config"
        );
        assert_eq!(
            classify_resource(Path::new("README.md"), dummy).0,
            "documentation"
        );
        assert_eq!(
            classify_resource(Path::new("assets/logo.svg"), dummy).0,
            "asset"
        );
        assert_eq!(
            classify_resource(Path::new("node_modules/lodash/index.js"), dummy).0,
            "dependency"
        );
        assert_eq!(
            classify_resource(Path::new("vendor/bundle/gems/rails.rb"), dummy).0,
            "dependency"
        );
        assert_eq!(
            classify_resource(Path::new("dist/app.bundle.js"), dummy).0,
            "generated"
        );
        assert_eq!(
            classify_resource(Path::new("target/debug/build.log"), dummy).0,
            "generated"
        );
        assert_eq!(
            classify_resource(Path::new("bin/tool.exe"), dummy).0,
            "generated" // matches bin/ component
        );
        assert_eq!(
            classify_resource(Path::new("native/driver.dll"), dummy).0,
            "binary"
        );
    }
}
