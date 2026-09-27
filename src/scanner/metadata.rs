use crate::error::Result;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    pub kind: String,
    pub language: Option<String>,
    pub size_bytes: i64,
    pub line_count: Option<i64>,
}

pub fn detect_kind_and_language(path: &Path) -> (String, Option<String>) {
    let file_name = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Check special file names first
    match file_name.as_str() {
        "cargo.toml" | "cargo.lock" => return ("config".to_string(), Some("toml".to_string())),
        "package.json" | "package-lock.json" | "tsconfig.json" => {
            return ("config".to_string(), Some("json".to_string()))
        }
        "readme.md" | "license" | "contributing.md" | "agents.md" => {
            return ("documentation".to_string(), Some("markdown".to_string()))
        }
        ".gitignore" | ".gitattributes" | ".editorconfig" => return ("config".to_string(), None),
        _ => {}
    }

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
        "ini" | "cfg" | "conf" | "env" => ("config".to_string(), None),

        "md" | "markdown" | "txt" | "rst" => {
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

pub fn is_binary_file(path: &Path) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut buffer = [0u8; 8192];
    let bytes_read = file.read(&mut buffer)?;

    // Check for null bytes in initial sample
    Ok(buffer[..bytes_read].contains(&0))
}

pub fn count_lines(path: &Path) -> Result<Option<i64>> {
    if is_binary_file(path)? {
        return Ok(None);
    }

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut lines = 0i64;
    let mut buffer = String::new();

    loop {
        buffer.clear();
        let bytes_read = match reader.read_line(&mut buffer) {
            Ok(b) => b,
            Err(_) => return Ok(None), // Not valid UTF-8, treat as binary
        };
        if bytes_read == 0 {
            break;
        }
        lines += 1;
    }

    Ok(Some(lines))
}

pub fn inspect_file_metadata(path: &Path) -> Result<FileMetadata> {
    let fs_meta = std::fs::metadata(path)?;
    let size_bytes = fs_meta.len() as i64;
    let (kind, language) = detect_kind_and_language(path);
    let line_count = count_lines(path).unwrap_or(None);

    Ok(FileMetadata {
        kind,
        language,
        size_bytes,
        line_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_detect_kind_and_language() {
        assert_eq!(
            detect_kind_and_language(Path::new("src/main.rs")),
            ("source".to_string(), Some("rust".to_string()))
        );
        assert_eq!(
            detect_kind_and_language(Path::new("src/index.tsx")),
            ("source".to_string(), Some("typescript".to_string()))
        );
        assert_eq!(
            detect_kind_and_language(Path::new("package.json")),
            ("config".to_string(), Some("json".to_string()))
        );
        assert_eq!(
            detect_kind_and_language(Path::new("logo.png")),
            ("asset".to_string(), None)
        );
        assert_eq!(
            detect_kind_and_language(Path::new("README.md")),
            ("documentation".to_string(), Some("markdown".to_string()))
        );
    }

    #[test]
    fn test_text_and_binary_line_count() {
        let mut text_file = NamedTempFile::new().unwrap();
        writeln!(text_file, "line 1").unwrap();
        writeln!(text_file, "line 2").unwrap();
        writeln!(text_file, "line 3").unwrap();
        assert_eq!(count_lines(text_file.path()).unwrap(), Some(3));

        let mut bin_file = NamedTempFile::new().unwrap();
        bin_file
            .write_all(&[0x89, b'P', b'N', b'G', 0x00, 0x01, 0x02])
            .unwrap();
        assert_eq!(count_lines(bin_file.path()).unwrap(), None);
    }
}
