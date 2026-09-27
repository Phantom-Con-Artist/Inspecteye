//! External filesystem references: absolute paths written in project files.
//!
//! Detection is textual and works on one fragment at a time (a string
//! literal, or a line of a configuration file). Evaluation looks only at the
//! referenced path itself (and its parent directory), never at anything
//! else on the machine (AGENTS.md §16).

use crate::issues::{Certainty, Severity, SourceRange};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// How the path is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathStyle {
    /// `C:\dir\file` or `C:/dir/file`
    WindowsDrive,
    /// `\\server\share\...`
    Unc,
    /// `/usr/...`, `/home/...` and other well-known Unix roots
    Unix,
    /// `~/...`
    Home,
}

impl PathStyle {
    pub fn as_str(&self) -> &'static str {
        match self {
            PathStyle::WindowsDrive => "windows_drive",
            PathStyle::Unc => "unc",
            PathStyle::Unix => "unix",
            PathStyle::Home => "home",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "windows_drive" => Some(PathStyle::WindowsDrive),
            "unc" => Some(PathStyle::Unc),
            "unix" => Some(PathStyle::Unix),
            "home" => Some(PathStyle::Home),
            _ => None,
        }
    }
}

/// An absolute path found in a project file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalReference {
    /// The path as written in the file (escapes included).
    pub raw: String,
    /// The path after removing string escaping (`\\` → `\`) and `file://`.
    pub path: String,
    pub style: PathStyle,
    /// `string_literal` (from a parsed code file) or `text` (line scan).
    pub context: String,
    pub range: SourceRange,
    /// Innermost enclosing symbol, when known.
    pub symbol: Option<String>,
    /// Found in test code, where fixture paths are often deliberately fake.
    pub in_test_code: bool,
}

/// What exists at a referenced path on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Existence {
    Exists {
        is_directory: bool,
    },
    Missing {
        parent_exists: bool,
    },
    /// Not probed; the reason says why.
    Unchecked {
        reason: String,
    },
}

/// Evaluation of one reference on this machine at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalCheck {
    pub existence: Existence,
    /// The path points inside the project root.
    pub inside_repository: bool,
    /// Why the path is specific to one user or machine, when it is.
    pub machine_specific: Option<String>,
    pub rule: String,
    pub severity: Severity,
    pub certainty: Certainty,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Unix roots that identify an absolute filesystem path. URL routes such as
/// `/api/users` do not start with one of these.
const UNIX_ROOTS: &[&str] = &[
    "home", "Users", "usr", "etc", "var", "opt", "tmp", "mnt", "srv", "Volumes", "root", "Library",
    "private", "media",
];

/// Roots that also appear as URL routes (`/home/feed`); they need a deeper
/// path before they are taken as filesystem paths.
const AMBIGUOUS_UNIX_ROOTS: &[&str] = &["home", "Users"];

/// Options for scanning one fragment.
#[derive(Debug, Clone, Copy)]
pub struct ScanOptions {
    /// The fragment uses backslash escapes (`"C:\\dir"` means `C:\dir`).
    pub escaped: bool,
    /// Report `~/...` paths. Off for languages where `~/` is a module alias.
    pub home_paths: bool,
}

/// A path found in a fragment; byte offsets are relative to the fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMatch {
    pub start: usize,
    pub end: usize,
    pub path: String,
    pub style: PathStyle,
}

/// Finds absolute paths in a piece of text.
pub fn find_paths(fragment: &str, opts: ScanOptions) -> Vec<PathMatch> {
    let bytes = fragment.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !at_boundary(bytes, i) {
            i += 1;
            continue;
        }
        let quoted = i > 0 && matches!(bytes[i - 1], b'"' | b'\'' | b'`');
        if let Some(m) = match_at(fragment, i, quoted, opts) {
            i = m.end.max(i + 1);
            out.push(m);
        } else {
            i += 1;
        }
    }
    out
}

fn at_boundary(bytes: &[u8], i: usize) -> bool {
    i == 0
        || matches!(
            bytes[i - 1],
            b' ' | b'\t' | b'"' | b'\'' | b'`' | b'=' | b'(' | b'[' | b'{' | b',' | b'>' | b';'
        )
}

fn match_at(text: &str, start: usize, quoted: bool, opts: ScanOptions) -> Option<PathMatch> {
    let rest = &text[start..];
    let b = rest.as_bytes();

    // file:// URLs name a local path explicitly.
    if let Some(after) = rest.strip_prefix("file://") {
        // `file:///C:/x` carries a slash before the drive letter.
        let inner_start = start + "file://".len() + usize::from(is_drive_after_slash(after));
        let end = path_end(text, inner_start, quoted, opts.escaped);
        let inner = percent_decode(&text[inner_start..end]);
        let style = if is_drive(inner.as_bytes()) {
            PathStyle::WindowsDrive
        } else if inner.starts_with('/') {
            PathStyle::Unix
        } else {
            return None;
        };
        return Some(PathMatch {
            start,
            end,
            path: inner,
            style,
        });
    }

    // In an escaped string a path separator is written `\\`; `x:\n` is a
    // name followed by a newline escape.
    let escaped_drive_ok = !opts.escaped || b.get(2) != Some(&b'\\') || b.get(3) == Some(&b'\\');
    let style = if is_drive(b) && escaped_drive_ok {
        PathStyle::WindowsDrive
    } else if is_unc(b, opts.escaped) {
        PathStyle::Unc
    } else if b.first() == Some(&b'/') && unix_root_ok(rest) {
        PathStyle::Unix
    } else if opts.home_paths
        && rest.starts_with("~/")
        && b.get(2)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'.' || *c == b'_')
    {
        PathStyle::Home
    } else {
        return None;
    };

    let end = path_end(text, start, quoted, opts.escaped);
    let raw = &text[start..end];
    let mut path = if opts.escaped {
        raw.replace(r"\\", r"\")
    } else {
        raw.to_string()
    };
    if style == PathStyle::Unc && !path.starts_with(r"\\") {
        return None;
    }
    if style == PathStyle::Unix && !unix_root_ok(&path) {
        return None;
    }
    if path.len() > 1 {
        let trimmed = path.trim_end_matches(['.', ',', ':']).len();
        path.truncate(trimmed);
    }
    Some(PathMatch {
        start,
        end: start + raw.len(),
        path,
        style,
    })
}

fn is_drive_after_slash(after: &str) -> bool {
    after
        .strip_prefix('/')
        .is_some_and(|s| is_drive(s.as_bytes()))
}

fn is_drive(b: &[u8]) -> bool {
    b.len() >= 3
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b[2] == b'\\' || b[2] == b'/')
        && !(b[2] == b'/' && b.get(3) == Some(&b'/'))
}

/// `\\server\share`, or `\\\\server\\share` inside an escaped string.
fn is_unc(b: &[u8], escaped: bool) -> bool {
    let slashes = if escaped { 4 } else { 2 };
    b.len() > slashes
        && b[..slashes].iter().all(|c| *c == b'\\')
        && (b[slashes].is_ascii_alphanumeric() || b[slashes] == b'.')
        && {
            // server name, then a separator and a share name
            let rest = &b[slashes..];
            let name_len = rest
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_' | b'$'))
                .count();
            let sep = rest.get(name_len) == Some(&b'\\');
            let share = rest.get(name_len + if escaped { 2 } else { 1 });
            sep && share.is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'$'))
        }
}

/// `/root/segment` for a known root; ambiguous roots need `/root/a/b`.
fn unix_root_ok(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('/') else {
        return false;
    };
    let segments: Vec<&str> = rest
        .split(['/', '"', '\'', '`', ' ', '\n'])
        .take_while(|seg| !seg.is_empty())
        .collect();
    match segments.first() {
        Some(root) if AMBIGUOUS_UNIX_ROOTS.contains(root) => segments.len() >= 3,
        Some(root) if UNIX_ROOTS.contains(root) => segments.len() >= 2,
        _ => false,
    }
}

/// End of a path starting at `start`. Spaces belong to the path only when it
/// is the whole of a quoted value (`"C:\Program Files\app"`). In an escaped
/// string a single backslash starts an escape sequence, which ends the path.
fn path_end(text: &str, start: usize, quoted: bool, escaped: bool) -> usize {
    let mut end = start;
    let mut chars = text[start..].char_indices().peekable();
    while let Some((off, c)) = chars.next() {
        if escaped && c == '\\' {
            if chars.peek().is_some_and(|(_, next)| *next == '\\') {
                chars.next();
                end = start + off + 2;
                continue;
            }
            break;
        }
        let stop = match c {
            '"' | '\'' | '`' | '\n' | '\r' | '\t' | '<' | '>' | '|' | ';' => true,
            ' ' | ',' => !quoted,
            _ => false,
        };
        if stop {
            break;
        }
        end = start + off + c.len_utf8();
    }
    // Trailing punctuation belongs to the sentence, not the path.
    let s = &text[start..end];
    let mut trimmed = s.trim_end_matches(['.', ',', ':', ' ']);
    while trimmed.ends_with(')') && trimmed.matches('(').count() < trimmed.matches(')').count() {
        trimmed = &trimmed[..trimmed.len() - 1];
    }
    start + trimmed.len()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// Characters that make a path a template or pattern rather than one path.
const TEMPLATE_CHARS: &[char] = &['{', '}', '$', '%', '*', '?'];

/// Checks a reference on this machine: existence (a single `metadata`
/// call on the path and possibly its parent), whether it points into the
/// project, and whether it is user-specific.
///
/// In test code every finding is INFO: fixture paths are often deliberately
/// fake, so a missing one is not evidence of breakage.
pub fn evaluate(
    path: &str,
    style: PathStyle,
    project_root: &Path,
    in_test_code: bool,
) -> ExternalCheck {
    let machine_specific = user_specific(path, style);
    let local = local_path(path, style);
    let inside_repository = local.as_deref().is_some_and(|p| is_inside(p, project_root));

    let existence = if path.contains(TEMPLATE_CHARS) {
        Existence::Unchecked {
            reason: "contains placeholders or wildcards".to_string(),
        }
    } else if style == PathStyle::Unc {
        Existence::Unchecked {
            reason: "network path; not probed to avoid network access".to_string(),
        }
    } else {
        match &local {
            None => Existence::Unchecked {
                reason: "path for another operating system".to_string(),
            },
            Some(p) => match std::fs::metadata(p) {
                Ok(meta) => Existence::Exists {
                    is_directory: meta.is_dir(),
                },
                Err(_) => Existence::Missing {
                    parent_exists: p.parent().is_some_and(|d| d.is_dir()),
                },
            },
        }
    };

    let (rule, severity, certainty, message) = match &existence {
        Existence::Missing {
            parent_exists: false,
        } => (
            "external.missing",
            Severity::Error,
            Certainty::Fact,
            format!("Referenced external resource does not exist on this machine: {path}"),
        ),
        Existence::Missing {
            parent_exists: true,
        } => (
            "external.missing_parent_exists",
            Severity::Warning,
            Certainty::Inference,
            format!(
                "Referenced resource does not exist, but its directory does (it may be created at runtime): {path}"
            ),
        ),
        Existence::Exists { .. } if inside_repository => (
            "external.absolute_into_repository",
            Severity::Warning,
            Certainty::Fact,
            format!(
                "Absolute path into the repository; it breaks when the project is cloned or moved. Prefer a relative path: {path}"
            ),
        ),
        Existence::Exists { .. } => (
            "external.dependency",
            Severity::Warning,
            Certainty::Fact,
            format!(
                "External filesystem dependency: exists on this machine but outside the repository, and may not exist elsewhere: {path}"
            ),
        ),
        Existence::Unchecked { reason } => (
            "external.unverified",
            Severity::Warning,
            Certainty::Possibility,
            format!("Possible external filesystem dependency (not checked: {reason}): {path}"),
        ),
    };

    let (severity, message) = if in_test_code {
        (Severity::Info, format!("{message} (in test code)"))
    } else {
        (severity, message)
    };
    ExternalCheck {
        existence,
        inside_repository,
        machine_specific,
        rule: rule.to_string(),
        severity,
        certainty,
        message,
    }
}

/// Whether a location is test code, judged by the file path and the name of
/// the enclosing symbol (`tests::...`, `test_x`, `FooTests`).
pub fn is_test_code(file_path: &str, symbol: Option<&str>) -> bool {
    const TEST_DIRS: &[&str] = &[
        "test",
        "tests",
        "__tests__",
        "spec",
        "specs",
        "testdata",
        "fixtures",
        "e2e",
    ];
    let lower = file_path.to_lowercase();
    let mut parts: Vec<&str> = lower.split('/').collect();
    let name = parts.pop().unwrap_or("");
    if parts.iter().any(|d| TEST_DIRS.contains(d)) {
        return true;
    }
    let stem = name.split('.').next().unwrap_or("");
    if name.contains(".test.")
        || name.contains(".spec.")
        || stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("_spec")
        || stem.ends_with("tests")
    {
        return true;
    }
    symbol.is_some_and(|s| {
        s.split(['.', ':']).any(|seg| {
            seg == "tests"
                || seg == "test"
                || seg.starts_with("test_")
                || seg.ends_with("Tests")
                || seg.ends_with("Test")
        })
    })
}

/// The path as this machine would open it, or `None` when it is written for
/// another operating system.
fn local_path(path: &str, style: PathStyle) -> Option<PathBuf> {
    match style {
        PathStyle::WindowsDrive | PathStyle::Unc => cfg!(windows).then(|| PathBuf::from(path)),
        PathStyle::Unix => (!cfg!(windows)).then(|| PathBuf::from(path)),
        PathStyle::Home => {
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
            Some(PathBuf::from(home).join(&path[2..]))
        }
    }
}

fn is_inside(path: &Path, root: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        let s = s.trim_end_matches('/').to_string();
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s
        }
    };
    let (p, r) = (norm(path), norm(root));
    p == r || p.starts_with(&format!("{r}/"))
}

/// Why a path belongs to one user's account, if it does.
fn user_specific(path: &str, style: PathStyle) -> Option<String> {
    if style == PathStyle::Home {
        return Some("relative to the current user's home directory".to_string());
    }
    let norm = path.replace('\\', "/");
    let segments: Vec<&str> = norm.split('/').filter(|s| !s.is_empty()).collect();
    let user = match (style, segments.as_slice()) {
        (PathStyle::WindowsDrive, [_, users, name, ..]) if users.eq_ignore_ascii_case("users") => {
            Some(*name)
        }
        (PathStyle::Unix, ["home", name, ..]) | (PathStyle::Unix, ["Users", name, ..]) => {
            Some(*name)
        }
        _ => None,
    }?;
    if user.contains(TEMPLATE_CHARS)
        || ["public", "default", "shared", "all users"].contains(&user.to_lowercase().as_str())
    {
        return None;
    }
    Some(format!("inside the home directory of user '{user}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ESC: ScanOptions = ScanOptions {
        escaped: true,
        home_paths: true,
    };
    const RAW: ScanOptions = ScanOptions {
        escaped: false,
        home_paths: true,
    };

    fn paths(text: &str, opts: ScanOptions) -> Vec<(String, PathStyle)> {
        find_paths(text, opts)
            .into_iter()
            .map(|m| (m.path, m.style))
            .collect()
    }

    #[test]
    fn test_windows_paths_in_both_slash_styles() {
        assert_eq!(
            paths(r#""C:/assets/bear_butt.png""#, ESC),
            vec![("C:/assets/bear_butt.png".into(), PathStyle::WindowsDrive)]
        );
        assert_eq!(
            paths(r#""C:\\assets\\bear_butt.png""#, ESC),
            vec![(r"C:\assets\bear_butt.png".into(), PathStyle::WindowsDrive)]
        );
        assert_eq!(
            paths(r#"@"C:\Program Files\App\app.exe""#, RAW),
            vec![(
                r"C:\Program Files\App\app.exe".into(),
                PathStyle::WindowsDrive
            )]
        );
    }

    #[test]
    fn test_unquoted_paths_stop_at_whitespace() {
        assert_eq!(
            paths(r"copy C:\a\b.txt D:\c", RAW),
            vec![
                (r"C:\a\b.txt".into(), PathStyle::WindowsDrive),
                (r"D:\c".into(), PathStyle::WindowsDrive)
            ]
        );
        assert_eq!(
            paths("log_file = /var/log/app.log  # rotated", RAW),
            vec![("/var/log/app.log".into(), PathStyle::Unix)]
        );
    }

    #[test]
    fn test_routes_urls_and_regexes_are_not_paths() {
        for text in [
            r#""/api/users/1""#,
            r#""/home""#,
            r#""/home/settings""#,
            r#""https://example.com/usr/lib""#,
            r#""\\d+\\s""#,
            r#""\\bword\\b""#,
            r#""a:b""#,
            r#""http://C:/x""#,
            r#""~/components/Button""#,
        ] {
            let opts = ScanOptions {
                escaped: true,
                home_paths: false,
            };
            assert!(paths(text, opts).is_empty(), "{text}");
        }
    }

    #[test]
    fn test_unc_needs_server_and_share() {
        assert_eq!(
            paths(r#""\\\\fileserver\\share\\data.csv""#, ESC),
            vec![(r"\\fileserver\share\data.csv".into(), PathStyle::Unc)]
        );
        assert_eq!(
            paths(r"\\fileserver\share", RAW),
            vec![(r"\\fileserver\share".into(), PathStyle::Unc)]
        );
    }

    #[test]
    fn test_file_urls_and_home_paths() {
        assert_eq!(
            paths(r#""file:///C:/data/My%20File.txt""#, ESC),
            vec![("C:/data/My File.txt".into(), PathStyle::WindowsDrive)]
        );
        assert_eq!(
            paths(r#""file:///etc/app.conf""#, ESC),
            vec![("/etc/app.conf".into(), PathStyle::Unix)]
        );
        assert_eq!(
            paths(r#""~/.aws/credentials""#, ESC),
            vec![("~/.aws/credentials".into(), PathStyle::Home)]
        );
    }

    #[test]
    fn test_trailing_sentence_punctuation_is_trimmed() {
        assert_eq!(
            paths("(see C:\\docs\\a.txt).", RAW),
            vec![(r"C:\docs\a.txt".into(), PathStyle::WindowsDrive)]
        );
    }

    #[test]
    fn test_evaluation_severity_follows_evidence() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let outside = dir.path().join("asset.png");
        std::fs::write(&outside, b"x").unwrap();
        let inside = root.join("data.json");
        std::fs::write(&inside, b"{}").unwrap();
        let style = if cfg!(windows) {
            PathStyle::WindowsDrive
        } else {
            PathStyle::Unix
        };
        let s = |p: &Path| p.to_string_lossy().into_owned();

        let c = evaluate(&s(&outside), style, &root, false);
        assert_eq!(
            (c.rule.as_str(), c.severity),
            ("external.dependency", Severity::Warning)
        );
        assert!(!c.inside_repository);

        let c = evaluate(&s(&inside), style, &root, false);
        assert_eq!(c.rule, "external.absolute_into_repository");
        assert!(c.inside_repository);

        let c = evaluate(&s(&dir.path().join("gone.png")), style, &root, false);
        assert_eq!(c.rule, "external.missing_parent_exists");
        assert_eq!(c.severity, Severity::Warning);

        let c = evaluate(
            &s(&dir.path().join("no_dir").join("gone.png")),
            style,
            &root,
            false,
        );
        assert_eq!(
            (c.rule.as_str(), c.severity),
            ("external.missing", Severity::Error)
        );
        assert_eq!(c.certainty, Certainty::Fact);

        let c = evaluate(r"C:\logs\*.log", PathStyle::WindowsDrive, &root, false);
        assert_eq!(c.rule, "external.unverified");
        let c = evaluate(r"\\server\share\x", PathStyle::Unc, &root, false);
        assert!(matches!(c.existence, Existence::Unchecked { .. }));
    }

    #[test]
    fn test_escape_sequences_are_not_separators() {
        assert!(paths(r#""let x:\n""#, ESC).is_empty());
        assert_eq!(
            paths(r#""C:\\old\\a.png\nnext""#, ESC),
            vec![(r"C:\old\a.png".into(), PathStyle::WindowsDrive)]
        );
    }

    #[test]
    fn test_test_code_is_info_only() {
        let root = Path::new("/nonexistent-project");
        let c = evaluate(r"Q:\no\such\file.png", PathStyle::WindowsDrive, root, true);
        assert_eq!(c.severity, Severity::Info);
        assert!(is_test_code("tests/json_tests.rs", None));
        assert!(is_test_code("src/auth.test.ts", None));
        assert!(is_test_code("src/a.rs", Some("tests::test_x")));
        assert!(is_test_code("App/LoginTests.cs", None));
        assert!(!is_test_code("src/testing_utils.rs", Some("parse")));
        assert!(!is_test_code("src/contest.rs", Some("latest")));
    }

    #[test]
    fn test_user_specific_paths() {
        assert!(user_specific(r"C:\Users\alice\data.csv", PathStyle::WindowsDrive).is_some());
        assert!(user_specific(r"C:\Users\Public\data.csv", PathStyle::WindowsDrive).is_none());
        assert!(user_specific("/home/bob/x", PathStyle::Unix).is_some());
        assert!(user_specific("/etc/hosts", PathStyle::Unix).is_none());
    }
}
