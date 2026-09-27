//! Ranked project lists built from explicit counts.
//!
//! Each ranking names its metric in plain words and ranks by a single
//! countable value, so every position can be checked by hand. There is no
//! combined "importance" or "risk" score.

use serde::Serialize;

/// Ranking categories, as selected on the command line.
pub const CATEGORIES: &[&str] = &[
    "changed",
    "diagnostics",
    "dependencies",
    "symbols",
    "size",
    "complexity",
    "partial-classes",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RankRow {
    pub label: String,
    pub value: usize,
    pub detail: String,
    /// The project file the row is about, when it is about one file.
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ranking {
    pub category: &'static str,
    pub title: String,
    /// What `value` counts.
    pub metric: String,
    pub rows: Vec<RankRow>,
}

impl Ranking {
    /// Keeps the `limit` highest non-zero rows, ties broken by label.
    pub fn new(
        category: &'static str,
        title: impl Into<String>,
        metric: impl Into<String>,
        mut rows: Vec<RankRow>,
        limit: usize,
    ) -> Self {
        rows.retain(|r| r.value > 0);
        if category != "partial-classes" {
            for r in &mut rows {
                r.path = Some(path_of_label(&r.label));
            }
        }
        rows.sort_by(|a, b| b.value.cmp(&a.value).then_with(|| a.label.cmp(&b.label)));
        rows.truncate(limit);
        Self {
            category,
            title: title.into(),
            metric: metric.into(),
            rows,
        }
    }
}

pub fn row(label: impl Into<String>, value: usize, detail: impl Into<String>) -> RankRow {
    RankRow {
        label: label.into(),
        value,
        detail: detail.into(),
        path: None,
    }
}

/// Labels are a path, or `name  (path)` / `name  (path:line)`.
fn path_of_label(label: &str) -> String {
    match label.rsplit_once("  (") {
        Some((_, rest)) => {
            let inner = rest.trim_end_matches(')');
            match inner.rsplit_once(':') {
                Some((p, line)) if line.chars().all(|c| c.is_ascii_digit()) => p.to_string(),
                _ => inner.to_string(),
            }
        }
        None => label.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ranking_orders_filters_and_truncates() {
        let r = Ranking::new(
            "size",
            "t",
            "m",
            vec![
                row("b", 5, ""),
                row("a", 5, ""),
                row("c", 0, ""),
                row("d", 9, ""),
            ],
            2,
        );
        let labels: Vec<&str> = r.rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["d", "a"]);
    }
}
