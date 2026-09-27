//! `generated_from` links read from JavaScript/CSS source maps.
//!
//! A source map lists the files an output was built from, so a link found
//! there is a fact. Only maps of generated files already in the index are
//! read; nothing outside the project is opened.

use crate::analysis::language::{join_rel, normalize_rel, parent_dir};
use crate::intelligence::{RelationshipKind, ResolvedRelationship};
use crate::issues::Certainty;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const MAPPED_EXTENSIONS: &[&str] = &[".js", ".mjs", ".cjs", ".css"];
/// `sourceMappingURL` comments sit at the end of the file.
const TAIL_BYTES: u64 = 4096;

/// `generated` lists (file id, path) of generated files; `path_ids` maps
/// every indexed path to its file id.
pub fn source_map_links(
    root: &Path,
    generated: &[(i64, String)],
    path_ids: &HashMap<String, i64>,
) -> Vec<ResolvedRelationship> {
    let mut out = Vec::new();
    for (file_id, path) in generated {
        if !MAPPED_EXTENSIONS.iter().any(|e| path.ends_with(e)) {
            continue;
        }
        let Some(map_path) = find_map(root, path, path_ids) else {
            continue;
        };
        let Ok(text) = fs::read_to_string(root.join(&map_path)) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let source_root = json
            .get("sourceRoot")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let Some(sources) = json.get("sources").and_then(|v| v.as_array()) else {
            continue;
        };
        for source in sources.iter().filter_map(|s| s.as_str()) {
            let Some(rel) = project_path(&map_path, source_root, source) else {
                continue;
            };
            let Some(&target_id) = path_ids.get(&rel) else {
                continue;
            };
            if target_id == *file_id {
                continue;
            }
            out.push(ResolvedRelationship {
                kind: RelationshipKind::GeneratedFrom,
                source_file_id: *file_id,
                source_symbol_id: None,
                target_file_id: Some(target_id),
                target_symbol_id: None,
                target_name: source.to_string(),
                certainty: Certainty::Fact,
                line: 0,
                evidence: format!("{map_path} lists `{source}` as a source of {path}."),
            });
        }
    }
    out
}

/// `<file>.map` next to the output, or the file named by its
/// `sourceMappingURL` comment. Inline `data:` maps are not read.
fn find_map(root: &Path, path: &str, path_ids: &HashMap<String, i64>) -> Option<String> {
    let sibling = format!("{path}.map");
    if path_ids.contains_key(&sibling) {
        return Some(sibling);
    }
    let url = source_mapping_url(&root.join(path))?;
    if url.starts_with("data:") || url.contains("://") {
        return None;
    }
    let candidate = normalize_rel(&join_rel(parent_dir(path), &url));
    path_ids.contains_key(&candidate).then_some(candidate)
}

fn source_mapping_url(file: &Path) -> Option<String> {
    let mut f = fs::File::open(file).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))
        .ok()?;
    let mut tail = Vec::new();
    f.read_to_end(&mut tail).ok()?;
    let tail = String::from_utf8_lossy(&tail);
    let idx = tail.rfind("sourceMappingURL=")?;
    let rest = &tail[idx + "sourceMappingURL=".len()..];
    let url: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '*')
        .collect();
    (!url.is_empty()).then_some(url)
}

/// Project-relative path of a source entry, if it is a plain relative path.
fn project_path(map_path: &str, source_root: &str, source: &str) -> Option<String> {
    // Bundler-specific schemes (webpack://, vite:) do not name files directly.
    let source = source.strip_prefix("webpack:///").unwrap_or(source);
    if source.contains("://") || source.starts_with('/') {
        return None;
    }
    let base = join_rel(parent_dir(map_path), source_root);
    let joined = join_rel(&base, source);
    // Paths leaving the project root would normalise to something inside it.
    let depth_ok = {
        let mut depth: i64 = 0;
        for seg in joined.split('/') {
            match seg {
                ".." => depth -= 1,
                "" | "." => {}
                _ => depth += 1,
            }
            if depth < 0 {
                break;
            }
        }
        depth >= 0
    };
    depth_ok.then(|| normalize_rel(&joined))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_path_resolution() {
        assert_eq!(
            project_path("dist/app.js.map", "", "../src/app.ts").as_deref(),
            Some("src/app.ts")
        );
        assert_eq!(
            project_path("dist/app.js.map", "../src", "app.ts").as_deref(),
            Some("src/app.ts")
        );
        assert_eq!(project_path("app.js.map", "", "../../outside.ts"), None);
        assert_eq!(project_path("a.js.map", "", "webpack://x/y.ts"), None);
    }

    #[test]
    fn test_links_from_sibling_map() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("dist")).unwrap();
        fs::write(
            root.join("dist/app.js"),
            "x()\n//# sourceMappingURL=app.js.map\n",
        )
        .unwrap();
        fs::write(
            root.join("dist/app.js.map"),
            r#"{"version":3,"sources":["../src/app.ts","../node_modules/lib.js"]}"#,
        )
        .unwrap();
        let ids: HashMap<String, i64> = [
            ("dist/app.js".to_string(), 1),
            ("dist/app.js.map".to_string(), 2),
            ("src/app.ts".to_string(), 3),
        ]
        .into_iter()
        .collect();
        let links = source_map_links(root, &[(1, "dist/app.js".into())], &ids);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target_file_id, Some(3));
        assert_eq!(links[0].kind, RelationshipKind::GeneratedFrom);
    }
}
