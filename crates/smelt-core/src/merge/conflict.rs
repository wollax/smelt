//! Conflict marker scanning utilities.

use std::path::Path;

use tracing::warn;

/// A contiguous region of conflict markers in a file.
#[derive(Debug, Clone)]
pub struct ConflictHunk {
    /// 1-based line number of the opening `<<<<<<<` marker.
    pub start_line: usize,
    /// 1-based line number of the closing `>>>>>>>` marker.
    pub end_line: usize,
}

/// Result of scanning a file (or set of files) for conflict markers.
#[derive(Debug, Clone)]
pub struct ConflictScan {
    /// Detected conflict hunks (regions between `<<<<<<<` and `>>>>>>>`).
    pub hunks: Vec<ConflictHunk>,
    /// Total number of lines within conflict regions.
    pub total_conflict_lines: usize,
}

impl ConflictScan {
    /// Whether any conflict markers were found.
    pub fn has_markers(&self) -> bool {
        !self.hunks.is_empty()
    }
}

/// Scan a string for git conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`).
///
/// A valid hunk requires all three markers in sequence. Partial/malformed
/// sequences (e.g. `<<<<<<<` without a closing `>>>>>>>`) are discarded.
pub fn scan_conflict_markers(content: &str) -> ConflictScan {
    let mut hunks = Vec::new();
    let mut total_conflict_lines: usize = 0;

    // State machine: None -> saw open -> saw separator -> saw close
    let mut open_line: Option<usize> = None;
    let mut saw_separator = false;

    for (idx, line) in content.lines().enumerate() {
        let line_num = idx + 1; // 1-based
        let trimmed = line.trim_end();

        if trimmed.starts_with("<<<<<<<") {
            // Starting a new hunk — discard any in-progress partial hunk.
            open_line = Some(line_num);
            saw_separator = false;
        } else if trimmed.starts_with("=======") && open_line.is_some() {
            saw_separator = true;
        } else if trimmed.starts_with(">>>>>>>") && open_line.is_some() && saw_separator {
            let start = open_line.unwrap();
            let end = line_num;
            total_conflict_lines += end - start + 1;
            hunks.push(ConflictHunk {
                start_line: start,
                end_line: end,
            });
            open_line = None;
            saw_separator = false;
        }
    }

    ConflictScan {
        hunks,
        total_conflict_lines,
    }
}

/// Scan multiple files in a working directory for conflict markers.
///
/// Reads each file, runs [`scan_conflict_markers`] on its content, and
/// aggregates results into a single [`ConflictScan`]. Files that cannot
/// be read (binary, deleted, permission errors) are silently skipped.
pub fn scan_files_for_markers(work_dir: &Path, files: &[String]) -> ConflictScan {
    let mut all_hunks = Vec::new();
    let mut total_lines: usize = 0;

    for file in files {
        let path = work_dir.join(file);
        let Ok(content) = std::fs::read_to_string(&path) else {
            warn!("skipping conflict scan for '{}': not readable as UTF-8", file);
            continue;
        };
        let scan = scan_conflict_markers(&content);
        total_lines += scan.total_conflict_lines;
        all_hunks.extend(scan.hunks);
    }

    ConflictScan {
        hunks: all_hunks,
        total_conflict_lines: total_lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_markers() {
        let scan = scan_conflict_markers("hello\nworld\n");
        assert!(!scan.has_markers());
        assert!(scan.hunks.is_empty());
        assert_eq!(scan.total_conflict_lines, 0);
    }

    #[test]
    fn one_complete_hunk() {
        let content = "\
line 1
<<<<<<< HEAD
our change
=======
their change
>>>>>>> feature
line 7
";
        let scan = scan_conflict_markers(content);
        assert!(scan.has_markers());
        assert_eq!(scan.hunks.len(), 1);
        assert_eq!(scan.hunks[0].start_line, 2);
        assert_eq!(scan.hunks[0].end_line, 6);
        assert_eq!(scan.total_conflict_lines, 5);
    }

    #[test]
    fn multiple_hunks() {
        let content = "\
<<<<<<< HEAD
ours-1
=======
theirs-1
>>>>>>> branch
clean line
<<<<<<< HEAD
ours-2
=======
theirs-2
>>>>>>> branch
";
        let scan = scan_conflict_markers(content);
        assert!(scan.has_markers());
        assert_eq!(scan.hunks.len(), 2);
        assert_eq!(scan.hunks[0].start_line, 1);
        assert_eq!(scan.hunks[0].end_line, 5);
        assert_eq!(scan.hunks[1].start_line, 7);
        assert_eq!(scan.hunks[1].end_line, 11);
        assert_eq!(scan.total_conflict_lines, 10);
    }

    #[test]
    fn partial_markers_no_closing() {
        let content = "\
<<<<<<< HEAD
ours
=======
theirs
";
        let scan = scan_conflict_markers(content);
        assert!(!scan.has_markers());
        assert!(scan.hunks.is_empty());
    }

    #[test]
    fn partial_markers_no_separator() {
        let content = "\
<<<<<<< HEAD
ours
>>>>>>> branch
";
        let scan = scan_conflict_markers(content);
        assert!(!scan.has_markers());
        assert!(scan.hunks.is_empty());
    }

    #[test]
    fn nested_open_discards_outer() {
        // If a second <<<<<<< appears before the first is closed, restart.
        let content = "\
<<<<<<< HEAD
ours-1
<<<<<<< HEAD
ours-2
=======
theirs-2
>>>>>>> branch
";
        let scan = scan_conflict_markers(content);
        assert!(scan.has_markers());
        assert_eq!(scan.hunks.len(), 1);
        // The second <<<<<<< at line 3 restarted the hunk.
        assert_eq!(scan.hunks[0].start_line, 3);
        assert_eq!(scan.hunks[0].end_line, 7);
    }

    #[test]
    fn empty_content() {
        let scan = scan_conflict_markers("");
        assert!(!scan.has_markers());
        assert!(scan.hunks.is_empty());
        assert_eq!(scan.total_conflict_lines, 0);
    }

    #[test]
    fn scan_files_aggregates() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");

        std::fs::write(
            &file_a,
            "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> b\n",
        )
        .unwrap();
        std::fs::write(&file_b, "clean content\n").unwrap();

        let scan = scan_files_for_markers(
            dir.path(),
            &["a.txt".to_string(), "b.txt".to_string()],
        );
        assert!(scan.has_markers());
        assert_eq!(scan.hunks.len(), 1);
        assert_eq!(scan.total_conflict_lines, 5);
    }

    #[test]
    fn scan_files_skips_missing() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let scan = scan_files_for_markers(
            dir.path(),
            &["nonexistent.txt".to_string()],
        );
        assert!(!scan.has_markers());
        assert!(scan.hunks.is_empty());
    }
}
