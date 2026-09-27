use chrono::Utc;
use serde::Serialize;
pub use similar::ChangeTag;
use similar::TextDiff;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionType {
    New,
    Modified,
    Restored,
    RestoredModified,
    Removed,
    Unchanged,
}

impl TransitionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Modified => "modified",
            Self::Restored => "restored",
            Self::RestoredModified => "restored_modified",
            Self::Removed => "removed",
            Self::Unchanged => "unchanged",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "new" => Self::New,
            "modified" => Self::Modified,
            "restored" => Self::Restored,
            "restored_modified" => Self::RestoredModified,
            "removed" => Self::Removed,
            _ => Self::Unchanged,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileState {
    pub id: Option<i64>,
    pub project_id: i64,
    pub file_id: i64,
    pub path: String,
    pub content_hash: String,
    pub recorded_at: String,
    pub source: String,
    pub git_commit: Option<String>,
    pub line_count: Option<usize>,
    pub size_bytes: u64,
    #[serde(skip)]
    pub content: Option<String>,
    pub exists_on_disk: bool,
}

impl FileState {
    pub fn from_record(rec: &crate::database::FileStateRecord, path: String) -> Self {
        Self {
            id: Some(rec.id),
            project_id: rec.project_id,
            file_id: rec.file_id,
            path,
            content_hash: rec.content_hash.clone(),
            recorded_at: rec.recorded_at.clone(),
            source: rec.source.clone(),
            git_commit: rec.git_commit.clone(),
            line_count: rec.line_count.map(|c| c as usize),
            size_bytes: rec.size_bytes as u64,
            content: rec.content.clone(),
            exists_on_disk: rec.exists_on_disk,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    #[serde(serialize_with = "serialize_tag")]
    pub tag: ChangeTag,
    pub old_line_no: Option<usize>,
    pub new_line_no: Option<usize>,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StateTransition {
    pub file_id: i64,
    pub path: String,
    pub transition_type: TransitionType,
    pub from_state: Option<FileState>,
    pub to_state: FileState,
    pub restored_to_state: Option<FileState>,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub hunks: Vec<DiffHunk>,
    pub created_at: String,
    /// True when the current state is the working tree and has not yet been
    /// recorded by a scan; false when this describes the last recorded transition.
    pub pending: bool,
    /// Symbols whose code differs between the two states, when the language
    /// is supported and both states have text snapshots.
    pub symbol_changes: Vec<crate::intelligence::SymbolChange>,
}

impl StateTransition {
    pub fn new_file(to_state: FileState) -> Self {
        let added_lines = to_state.line_count.unwrap_or(0);
        let path = to_state.path.clone();
        let file_id = to_state.file_id;
        Self {
            file_id,
            path,
            transition_type: TransitionType::New,
            from_state: None,
            to_state,
            restored_to_state: None,
            added_lines,
            removed_lines: 0,
            hunks: Vec::new(),
            created_at: Utc::now().to_rfc3339(),
            pending: false,
            symbol_changes: Vec::new(),
        }
    }

    pub fn removed(from_state: FileState, to_state: FileState) -> Self {
        let removed_lines = from_state.line_count.unwrap_or(0);
        let path = from_state.path.clone();
        let file_id = from_state.file_id;
        Self {
            file_id,
            path,
            transition_type: TransitionType::Removed,
            from_state: Some(from_state),
            to_state,
            restored_to_state: None,
            added_lines: 0,
            removed_lines,
            hunks: Vec::new(),
            created_at: Utc::now().to_rfc3339(),
            pending: false,
            symbol_changes: Vec::new(),
        }
    }

    pub fn unchanged(state: FileState) -> Self {
        let path = state.path.clone();
        let file_id = state.file_id;
        Self {
            file_id,
            path,
            transition_type: TransitionType::Unchanged,
            from_state: Some(state.clone()),
            to_state: state,
            restored_to_state: None,
            added_lines: 0,
            removed_lines: 0,
            hunks: Vec::new(),
            created_at: Utc::now().to_rfc3339(),
            pending: false,
            symbol_changes: Vec::new(),
        }
    }

    pub fn compute_transition(
        from_state: FileState,
        to_state: FileState,
        transition_type: TransitionType,
        restored_to_state: Option<FileState>,
    ) -> Self {
        let path = to_state.path.clone();
        let file_id = to_state.file_id;

        let (hunks, added_lines, removed_lines) = match (&from_state.content, &to_state.content) {
            (Some(old_c), Some(new_c)) => {
                let hunks = compute_line_diff(old_c, new_c);
                let mut added = 0;
                let mut removed = 0;
                for h in &hunks {
                    for l in &h.lines {
                        match l.tag {
                            ChangeTag::Insert => added += 1,
                            ChangeTag::Delete => removed += 1,
                            ChangeTag::Equal => {}
                        }
                    }
                }
                (hunks, added, removed)
            }
            _ => (Vec::new(), 0, 0),
        };

        Self {
            file_id,
            path,
            transition_type,
            from_state: Some(from_state),
            to_state,
            restored_to_state,
            added_lines,
            removed_lines,
            hunks,
            created_at: Utc::now().to_rfc3339(),
            pending: false,
            symbol_changes: Vec::new(),
        }
    }

    /// Marks the transition as describing unscanned working-tree changes.
    pub fn into_pending(mut self) -> Self {
        self.pending = true;
        self
    }

    pub fn format_terminal(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("File: {}\n", self.path));
        if self.transition_type != TransitionType::Unchanged {
            if self.pending {
                out.push_str("Source: working tree (pending, not yet recorded by a scan)\n");
            } else {
                out.push_str(&format!(
                    "Source: last recorded change (scan at {})\n",
                    self.to_state.recorded_at
                ));
            }
        }
        out.push('\n');

        match self.transition_type {
            TransitionType::Unchanged => {
                out.push_str("No pending or recorded changes for this file.\n");
                out.push_str("Latest state:\n");
                out.push_str(&format!(
                    "  Hash:     {}\n",
                    short_hash(&self.to_state.content_hash)
                ));
                out.push_str(&format!("  Recorded: {}\n", self.to_state.recorded_at));
                if let Some(lc) = self.to_state.line_count {
                    out.push_str(&format!("  Lines:    {}\n", lc));
                }
            }
            TransitionType::New => {
                out.push_str("Transition: New file (no previous state)\n");
                out.push_str("Current state:\n");
                out.push_str(&format!(
                    "  Hash:     {}\n",
                    short_hash(&self.to_state.content_hash)
                ));
                out.push_str(&format!("  Recorded: {}\n", self.to_state.recorded_at));
                if let Some(lc) = self.to_state.line_count {
                    out.push_str(&format!("  Lines:    {}\n", lc));
                }
                out.push_str(&format!("  Size:     {} bytes\n", self.to_state.size_bytes));
            }
            TransitionType::Removed => {
                out.push_str("Transition: Removed\n");
                out.push_str("Current state: Unavailable (file does not exist on disk)\n");
                if let Some(prev) = &self.from_state {
                    out.push_str("Previous state:\n");
                    out.push_str(&format!("  Hash:     {}\n", short_hash(&prev.content_hash)));
                    out.push_str(&format!("  Recorded: {}\n", prev.recorded_at));
                    if let Some(lc) = prev.line_count {
                        out.push_str(&format!("  Lines:    {}\n", lc));
                    }
                }
            }
            TransitionType::Restored => {
                if let Some(restored) = &self.restored_to_state {
                    out.push_str(&format!(
                        "Transition: Restored (content matches state recorded at {})\n",
                        restored.recorded_at
                    ));
                } else {
                    out.push_str("Transition: Restored (content matches previous known state)\n");
                }
                self.format_diff_details(&mut out);
            }
            TransitionType::RestoredModified => {
                out.push_str("Transition: Restored with modifications\n");
                self.format_diff_details(&mut out);
            }
            TransitionType::Modified => {
                out.push_str(&format!(
                    "Transition: Modified (+{} lines, -{} lines)\n",
                    self.added_lines, self.removed_lines
                ));
                self.format_diff_details(&mut out);
            }
        }

        out
    }

    fn format_symbol_changes(&self, out: &mut String) {
        if self.symbol_changes.is_empty() {
            return;
        }
        out.push_str(
            "Symbols changed:
",
        );
        for c in &self.symbol_changes {
            let lines = c
                .new_range
                .or(c.old_range)
                .map(|r| format!("lines {}-{}", r.start_line, r.end_line))
                .unwrap_or_default();
            out.push_str(&format!(
                "  {:<9} {:<12} {}  ({})
",
                c.change.as_str(),
                c.kind.as_str(),
                c.qualified_name,
                lines
            ));
        }
    }

    fn format_diff_details(&self, out: &mut String) {
        self.format_symbol_changes(out);
        if let Some(prev) = &self.from_state {
            out.push_str("Previous state:\n");
            out.push_str(&format!("  Hash:     {}\n", short_hash(&prev.content_hash)));
            out.push_str(&format!("  Recorded: {}\n", prev.recorded_at));
            if let Some(commit) = &prev.git_commit {
                out.push_str(&format!("  Commit:   {}\n", short_hash(commit)));
            }
        }
        out.push_str("Current state:\n");
        out.push_str(&format!(
            "  Hash:     {}\n",
            short_hash(&self.to_state.content_hash)
        ));
        let source_note = if self.to_state.source == "working_tree" {
            format!("{} (working tree)", self.to_state.recorded_at)
        } else {
            self.to_state.recorded_at.clone()
        };
        out.push_str(&format!("  Recorded: {}\n", source_note));
        if let Some(commit) = &self.to_state.git_commit {
            out.push_str(&format!("  Commit:   {}\n", short_hash(commit)));
        }

        if !self.hunks.is_empty() {
            out.push('\n');
            for hunk in &self.hunks {
                out.push_str(&format!(
                    "@@ -{},{} +{},{} @@\n",
                    hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
                ));
                for line in &hunk.lines {
                    match line.tag {
                        ChangeTag::Equal => {
                            let no = line.new_line_no.unwrap_or(0);
                            out.push_str(&format!("  {:4}  {}\n", no, line.content));
                        }
                        ChangeTag::Delete => {
                            let no = line.old_line_no.unwrap_or(0);
                            out.push_str(&format!("- {:4}  {}\n", no, line.content));
                        }
                        ChangeTag::Insert => {
                            let no = line.new_line_no.unwrap_or(0);
                            out.push_str(&format!("+ {:4}  {}\n", no, line.content));
                        }
                    }
                }
            }
        }
    }
}

fn serialize_tag<S: serde::Serializer>(tag: &ChangeTag, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(match tag {
        ChangeTag::Equal => "equal",
        ChangeTag::Delete => "delete",
        ChangeTag::Insert => "insert",
    })
}

fn short_hash(hash: &str) -> String {
    if hash.len() > 12 {
        format!("{}...", &hash[..12])
    } else {
        hash.to_string()
    }
}

pub fn compute_line_diff(old_text: &str, new_text: &str) -> Vec<DiffHunk> {
    if old_text == new_text {
        return Vec::new();
    }

    let diff = TextDiff::from_lines(old_text, new_text);
    let mut hunks = Vec::new();

    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        let ops = hunk.ops();
        let (old_start, old_lines, new_start, new_lines) =
            if let (Some(first), Some(last)) = (ops.first(), ops.last()) {
                let old_s = first.old_range().start;
                let old_l = last.old_range().end.saturating_sub(old_s);
                let new_s = first.new_range().start;
                let new_l = last.new_range().end.saturating_sub(new_s);
                (old_s + 1, old_l, new_s + 1, new_l)
            } else {
                (0, 0, 0, 0)
            };

        let mut lines = Vec::new();
        for change in hunk.iter_changes() {
            lines.push(DiffLine {
                tag: change.tag(),
                old_line_no: change.old_index().map(|i| i + 1),
                new_line_no: change.new_index().map(|i| i + 1),
                content: change.value().trim_end_matches(['\r', '\n']).to_string(),
            });
        }

        hunks.push(DiffHunk {
            old_start,
            old_lines,
            new_start,
            new_lines,
            lines,
        });
    }

    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_line_diff_addition() {
        let old = "fn main() {\n}\n";
        let new = "fn main() {\n    println!(\"hello\");\n}\n";

        let hunks = compute_line_diff(old, new);
        assert_eq!(hunks.len(), 1);
        let h = &hunks[0];
        assert_eq!(h.lines.len(), 3);
        assert_eq!(h.lines[0].tag, ChangeTag::Equal);
        assert_eq!(h.lines[0].old_line_no, Some(1));
        assert_eq!(h.lines[0].new_line_no, Some(1));

        assert_eq!(h.lines[1].tag, ChangeTag::Insert);
        assert_eq!(h.lines[1].old_line_no, None);
        assert_eq!(h.lines[1].new_line_no, Some(2));
        assert_eq!(h.lines[1].content, "    println!(\"hello\");");

        assert_eq!(h.lines[2].tag, ChangeTag::Equal);
        assert_eq!(h.lines[2].old_line_no, Some(2));
        assert_eq!(h.lines[2].new_line_no, Some(3));
    }

    #[test]
    fn test_compute_line_diff_deletion() {
        let old = "line 1\nline 2 to delete\nline 3\n";
        let new = "line 1\nline 3\n";

        let hunks = compute_line_diff(old, new);
        assert_eq!(hunks.len(), 1);
        let h = &hunks[0];
        assert_eq!(h.lines[1].tag, ChangeTag::Delete);
        assert_eq!(h.lines[1].old_line_no, Some(2));
        assert_eq!(h.lines[1].new_line_no, None);
        assert_eq!(h.lines[1].content, "line 2 to delete");
    }

    #[test]
    fn test_compute_line_diff_modification() {
        let old = "var x = old_val;\n";
        let new = "var x = new_val;\n";

        let hunks = compute_line_diff(old, new);
        assert_eq!(hunks.len(), 1);
        let h = &hunks[0];
        assert_eq!(h.lines.len(), 2);
        assert_eq!(h.lines[0].tag, ChangeTag::Delete);
        assert_eq!(h.lines[0].old_line_no, Some(1));
        assert_eq!(h.lines[1].tag, ChangeTag::Insert);
        assert_eq!(h.lines[1].new_line_no, Some(1));
    }

    #[test]
    fn test_compute_line_diff_multiple_hunks() {
        let mut old_lines = Vec::new();
        for i in 1..=30 {
            old_lines.push(format!("line {}", i));
        }
        let mut new_lines = old_lines.clone();
        new_lines[2] = "line 3 modified".to_string(); // early line
        new_lines[25] = "line 26 modified".to_string(); // late line

        let old = old_lines.join("\n") + "\n";
        let new = new_lines.join("\n") + "\n";

        let hunks = compute_line_diff(&old, &new);
        assert_eq!(hunks.len(), 2);
    }

    #[test]
    fn test_compute_line_diff_unchanged() {
        let old = "same content\n";
        let new = "same content\n";
        let hunks = compute_line_diff(old, new);
        assert!(hunks.is_empty());
    }
}
