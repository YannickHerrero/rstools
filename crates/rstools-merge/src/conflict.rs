#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkChoice {
    Ours,
    Theirs,
    Both,
}

#[derive(Debug, Clone)]
pub struct ConflictHunk {
    pub start_line: usize,
    pub end_line: usize,
    pub ours: Vec<String>,
    pub theirs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedConflictFile {
    pub lines: Vec<String>,
    pub hunks: Vec<ConflictHunk>,
    pub trailing_newline: bool,
}

#[derive(Debug, Clone)]
pub struct HunkPreview {
    pub before: Vec<String>,
    pub ours: Vec<String>,
    pub theirs: Vec<String>,
    pub after: Vec<String>,
}

pub fn parse_conflicts(text: &str) -> ParsedConflictFile {
    let lines: Vec<String> = text.lines().map(|line| line.to_string()).collect();
    let trailing_newline = text.ends_with('\n');

    let mut hunks = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        if !lines[i].starts_with("<<<<<<<") {
            i += 1;
            continue;
        }

        let start_line = i;
        i += 1;

        let mut ours = Vec::new();
        while i < lines.len()
            && !lines[i].starts_with("|||||||")
            && !lines[i].starts_with("=======")
        {
            ours.push(lines[i].clone());
            i += 1;
        }

        if i >= lines.len() {
            break;
        }

        if lines[i].starts_with("|||||||") {
            i += 1;
            while i < lines.len() && !lines[i].starts_with("=======") {
                i += 1;
            }
            if i >= lines.len() {
                break;
            }
        }

        i += 1;
        let mut theirs = Vec::new();
        while i < lines.len() && !lines[i].starts_with(">>>>>>>") {
            theirs.push(lines[i].clone());
            i += 1;
        }

        if i >= lines.len() {
            break;
        }

        let end_line = i;
        hunks.push(ConflictHunk {
            start_line,
            end_line,
            ours,
            theirs,
        });
        i += 1;
    }

    ParsedConflictFile {
        lines,
        hunks,
        trailing_newline,
    }
}

pub fn apply_hunk_choice(text: &str, hunk_index: usize, choice: HunkChoice) -> Option<String> {
    let parsed = parse_conflicts(text);
    let hunk = parsed.hunks.get(hunk_index)?;

    let replacement: Vec<String> = match choice {
        HunkChoice::Ours => hunk.ours.clone(),
        HunkChoice::Theirs => hunk.theirs.clone(),
        HunkChoice::Both => {
            let mut combined = hunk.ours.clone();
            combined.extend(hunk.theirs.clone());
            combined
        }
    };

    let mut new_lines = Vec::new();
    new_lines.extend(parsed.lines[..hunk.start_line].iter().cloned());
    new_lines.extend(replacement);
    if hunk.end_line + 1 < parsed.lines.len() {
        new_lines.extend(parsed.lines[hunk.end_line + 1..].iter().cloned());
    }

    let mut output = new_lines.join("\n");
    if parsed.trailing_newline {
        output.push('\n');
    }
    Some(output)
}

pub fn has_conflict_markers(text: &str) -> bool {
    text.contains("<<<<<<<") || text.contains("=======") || text.contains(">>>>>>>")
}

pub fn hunk_preview(text: &str, hunk_index: usize, context: usize) -> Option<HunkPreview> {
    let parsed = parse_conflicts(text);
    let hunk = parsed.hunks.get(hunk_index)?;

    let before_start = hunk.start_line.saturating_sub(context);
    let before = parsed.lines[before_start..hunk.start_line].to_vec();

    let after_start = hunk.end_line.saturating_add(1).min(parsed.lines.len());
    let after_end = (after_start + context).min(parsed.lines.len());
    let after = parsed.lines[after_start..after_end].to_vec();

    Some(HunkPreview {
        before,
        ours: hunk.ours.clone(),
        theirs: hunk.theirs.clone(),
        after,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_conflict() {
        let text = "line1\n<<<<<<< HEAD\na\n=======\nb\n>>>>>>> feature\nline2\n";
        let parsed = parse_conflicts(text);

        assert_eq!(parsed.hunks.len(), 1);
        assert_eq!(parsed.hunks[0].ours, vec!["a"]);
        assert_eq!(parsed.hunks[0].theirs, vec!["b"]);
    }

    #[test]
    fn applies_both_choice_in_order() {
        let text = "start\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\nend\n";
        let merged = apply_hunk_choice(text, 0, HunkChoice::Both).unwrap();

        assert_eq!(merged, "start\nours\ntheirs\nend\n");
    }
}
