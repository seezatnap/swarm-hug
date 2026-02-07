use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Terminal,
};

use crate::git::git_repo_root;
use swarm::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorktreeGroup {
    Shared,
    Project,
}

#[derive(Debug, Clone)]
struct WorktreeEntry {
    path: PathBuf,
    branch: Option<String>,
    group: WorktreeGroup,
    display: String,
}

#[derive(Debug, Clone)]
enum RowKind {
    Header(WorktreeGroup),
    SelectAll(WorktreeGroup),
    DeselectAll(WorktreeGroup),
    Entry(usize),
    Empty,
    Spacer,
    Ok,
    Cancel,
}

#[derive(Debug, Clone)]
struct Row {
    kind: RowKind,
}

#[derive(Debug, Clone)]
enum SelectorOutcome {
    Selected(Vec<usize>),
    Cancelled,
}

pub fn cmd_cleanup_worktrees(_config: &Config) -> Result<(), String> {
    let repo_root = git_repo_root()?;
    let entries = list_worktrees(&repo_root)?;
    if entries.is_empty() {
        println!("No worktrees found under .swarm-hug.");
        return Ok(());
    }

    let selection =
        run_selector(&entries).map_err(|e| format!("cleanup selector failed: {}", e))?;

    let selected = match selection {
        Some(selected) => selected,
        None => {
            println!("Cleanup cancelled.");
            return Ok(());
        }
    };

    if selected.is_empty() {
        println!("No worktrees selected.");
        return Ok(());
    }

    if !confirm_cleanup(&entries, &selected)? {
        println!("Cleanup cancelled.");
        return Ok(());
    }

    let mut errors = Vec::new();
    for idx in selected {
        let entry = &entries[idx];
        if let Err(e) = remove_worktree(&repo_root, entry) {
            errors.push(e);
        }
    }

    if errors.is_empty() {
        println!("Cleanup complete.");
        Ok(())
    } else {
        let joined = errors.join("\n");
        Err(format!("cleanup completed with errors:\n{}", joined))
    }
}

fn list_worktrees(repo_root: &Path) -> Result<Vec<WorktreeEntry>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .map_err(|e| format!("failed to run git worktree list: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree list failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_worktree_list_output(repo_root, &stdout))
}

fn parse_worktree_list_output(repo_root: &Path, stdout: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_branch: Option<String> = None;

    for line in stdout.lines().chain(std::iter::once("")) {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.trim().to_string());
            current_branch = None;
        } else if let Some(branch) = line.strip_prefix("branch ") {
            let trimmed = branch.trim();
            if let Some(stripped) = trimmed.strip_prefix("refs/heads/") {
                current_branch = Some(stripped.to_string());
            }
        } else if line.is_empty() {
            if let Some(path) = current_path.take() {
                if let Some(entry) = build_entry(repo_root, &path, current_branch.take()) {
                    entries.push(entry);
                }
            }
        }
    }

    entries.sort_by(|a, b| a.display.cmp(&b.display));
    entries
}

fn build_entry(repo_root: &Path, raw_path: &str, branch: Option<String>) -> Option<WorktreeEntry> {
    let raw = PathBuf::from(raw_path);
    let resolved = if raw.is_absolute() {
        raw
    } else {
        repo_root.join(raw)
    };

    let shared_root = repo_root
        .join(".swarm-hug")
        .join(".shared")
        .join("worktrees");
    let projects_root = repo_root.join(".swarm-hug");

    let (group, team_label) = if resolved.starts_with(&shared_root) {
        (WorktreeGroup::Shared, None)
    } else if resolved.starts_with(&projects_root) {
        let rel = resolved.strip_prefix(&projects_root).ok()?;
        let mut components = rel.components();
        let first = components.next()?.as_os_str().to_string_lossy().to_string();
        if first == ".shared" {
            return None;
        }

        if first == "worktrees" {
            // Legacy single-project layout: .swarm-hug/worktrees/<worktree>
            if components.next().is_none() {
                return None;
            }
            (WorktreeGroup::Project, Some("default".to_string()))
        } else {
            let worktrees_marker = components.next()?.as_os_str().to_string_lossy();
            if worktrees_marker != "worktrees" {
                return None;
            }
            (WorktreeGroup::Project, Some(first))
        }
    } else {
        return None;
    };

    let display_path = resolved
        .strip_prefix(repo_root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| resolved.to_string_lossy().to_string());

    let label = match (&branch, &team_label) {
        (Some(branch), Some(team)) => format!("{}/{}", team, branch),
        (Some(branch), None) => branch.to_string(),
        (None, Some(team)) => format!("{}/(detached)", team),
        (None, None) => "(detached)".to_string(),
    };

    Some(WorktreeEntry {
        path: resolved,
        branch,
        group,
        display: format!("{} - {}", label, display_path),
    })
}

fn run_selector(entries: &[WorktreeEntry]) -> io::Result<Option<Vec<usize>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut selections = vec![false; entries.len()];
    let mut rows = build_rows(entries);
    let mut cursor = first_selectable(&rows).unwrap_or(0);

    let finished = loop {
        rows = build_rows(entries);
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(2)].as_ref())
                .split(f.area());

            let items: Vec<ListItem> = rows
                .iter()
                .map(|row| {
                    let text = row_label(row, entries, &selections);
                    ListItem::new(text)
                })
                .collect();

            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Cleanup Worktrees"),
                )
                .highlight_style(
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                );

            let mut state = ratatui::widgets::ListState::default();
            state.select(Some(cursor));
            f.render_stateful_widget(list, chunks[0], &mut state);

            let help = Paragraph::new(
                "Up/Down: move  Space/Enter: toggle  Enter on OK: confirm  q: cancel",
            )
            .block(Block::default().borders(Borders::ALL));
            f.render_widget(help, chunks[1]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if let Some(result) = handle_key(key, &rows, &mut cursor, &mut selections, entries)
                {
                    break result;
                }
            }
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(match finished {
        SelectorOutcome::Selected(selection) => Some(selection),
        SelectorOutcome::Cancelled => None,
    })
}

fn confirm_cleanup(entries: &[WorktreeEntry], selected: &[usize]) -> Result<bool, String> {
    println!();
    println!("Selected worktrees:");
    for idx in selected {
        if let Some(entry) = entries.get(*idx) {
            println!("  - {}", entry.display);
        }
    }
    println!();
    println!("Proceed with cleanup? [y/N]");
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| format!("failed to read confirmation: {}", e))?;
    let answer = input.trim().to_lowercase();
    Ok(answer == "y" || answer == "yes")
}

fn remove_worktree(repo_root: &Path, entry: &WorktreeEntry) -> Result<(), String> {
    let path_str = entry.path.to_string_lossy().to_string();
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "remove", "--force", &path_str])
        .output()
        .map_err(|e| format!("git worktree remove failed for {}: {}", path_str, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "git worktree remove failed for {}: {}",
            path_str,
            stderr.trim()
        ));
    }

    if entry.path.exists() {
        fs::remove_dir_all(&entry.path)
            .map_err(|e| format!("failed to remove {}: {}", entry.path.display(), e))?;
    }

    if let Some(branch) = entry.branch.as_deref() {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(["branch", "-D", branch])
            .output()
            .map_err(|e| format!("git branch -D {} failed: {}", branch, e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "git branch -D {} failed: {}",
                branch,
                stderr.trim()
            ));
        }
    }

    Ok(())
}

fn build_rows(entries: &[WorktreeEntry]) -> Vec<Row> {
    let mut rows = Vec::new();

    rows.push(Row {
        kind: RowKind::Header(WorktreeGroup::Shared),
    });
    rows.push(Row {
        kind: RowKind::SelectAll(WorktreeGroup::Shared),
    });
    rows.push(Row {
        kind: RowKind::DeselectAll(WorktreeGroup::Shared),
    });
    let shared_entries: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.group == WorktreeGroup::Shared)
        .map(|(i, _)| i)
        .collect();
    if shared_entries.is_empty() {
        rows.push(Row {
            kind: RowKind::Empty,
        });
    } else {
        for idx in shared_entries {
            rows.push(Row {
                kind: RowKind::Entry(idx),
            });
        }
    }

    rows.push(Row {
        kind: RowKind::Spacer,
    });

    rows.push(Row {
        kind: RowKind::Header(WorktreeGroup::Project),
    });
    rows.push(Row {
        kind: RowKind::SelectAll(WorktreeGroup::Project),
    });
    rows.push(Row {
        kind: RowKind::DeselectAll(WorktreeGroup::Project),
    });
    let project_entries: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.group == WorktreeGroup::Project)
        .map(|(i, _)| i)
        .collect();
    if project_entries.is_empty() {
        rows.push(Row {
            kind: RowKind::Empty,
        });
    } else {
        for idx in project_entries {
            rows.push(Row {
                kind: RowKind::Entry(idx),
            });
        }
    }

    rows.push(Row {
        kind: RowKind::Spacer,
    });
    rows.push(Row { kind: RowKind::Ok });
    rows.push(Row {
        kind: RowKind::Cancel,
    });

    rows
}

fn first_selectable(rows: &[Row]) -> Option<usize> {
    rows.iter().position(|row| row_is_selectable(row))
}

fn row_is_selectable(row: &Row) -> bool {
    matches!(
        row.kind,
        RowKind::SelectAll(_)
            | RowKind::DeselectAll(_)
            | RowKind::Entry(_)
            | RowKind::Ok
            | RowKind::Cancel
    )
}

fn row_label(row: &Row, entries: &[WorktreeEntry], selections: &[bool]) -> String {
    match row.kind {
        RowKind::Header(group) => {
            let (selected, total) = group_counts(entries, selections, group);
            match group {
                WorktreeGroup::Shared => {
                    format!("Shared worktrees (selected {}/{})", selected, total)
                }
                WorktreeGroup::Project => {
                    format!("Project worktrees (selected {}/{})", selected, total)
                }
            }
        }
        RowKind::SelectAll(_) => "  [Select all]".to_string(),
        RowKind::DeselectAll(_) => "  [Deselect all]".to_string(),
        RowKind::Entry(idx) => {
            if let Some(entry) = entries.get(idx) {
                let checked = if selections[idx] { "[x]" } else { "[ ]" };
                format!("  {} {}", checked, entry.display)
            } else {
                "  [ ] (missing entry)".to_string()
            }
        }
        RowKind::Empty => "  (none)".to_string(),
        RowKind::Spacer => "".to_string(),
        RowKind::Ok => "[ OK ]".to_string(),
        RowKind::Cancel => "[ Cancel ]".to_string(),
    }
}

fn group_counts(
    entries: &[WorktreeEntry],
    selections: &[bool],
    group: WorktreeGroup,
) -> (usize, usize) {
    let mut total = 0;
    let mut selected = 0;
    for (idx, entry) in entries.iter().enumerate() {
        if entry.group == group {
            total += 1;
            if selections.get(idx).copied().unwrap_or(false) {
                selected += 1;
            }
        }
    }
    (selected, total)
}

fn handle_key(
    key: KeyEvent,
    rows: &[Row],
    cursor: &mut usize,
    selections: &mut [bool],
    entries: &[WorktreeEntry],
) -> Option<SelectorOutcome> {
    match key.code {
        KeyCode::Char('q') => return Some(SelectorOutcome::Cancelled),
        KeyCode::Up => {
            *cursor = move_cursor(rows, *cursor, -1);
        }
        KeyCode::Down => {
            *cursor = move_cursor(rows, *cursor, 1);
        }
        KeyCode::Char(' ') | KeyCode::Enter => {
            if let Some(row) = rows.get(*cursor) {
                match row.kind {
                    RowKind::Entry(idx) => {
                        if let Some(selected) = selections.get_mut(idx) {
                            *selected = !*selected;
                        }
                    }
                    RowKind::SelectAll(group) => {
                        for (idx, entry) in entries.iter().enumerate() {
                            if entry.group == group {
                                selections[idx] = true;
                            }
                        }
                    }
                    RowKind::DeselectAll(group) => {
                        for (idx, entry) in entries.iter().enumerate() {
                            if entry.group == group {
                                selections[idx] = false;
                            }
                        }
                    }
                    RowKind::Ok => {
                        let selected = selections
                            .iter()
                            .enumerate()
                            .filter_map(|(idx, selected)| if *selected { Some(idx) } else { None })
                            .collect();
                        return Some(SelectorOutcome::Selected(selected));
                    }
                    RowKind::Cancel => return Some(SelectorOutcome::Cancelled),
                    _ => {}
                }
            }
        }
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            for selected in selections.iter_mut() {
                *selected = true;
            }
        }
        _ => {}
    }
    None
}

fn move_cursor(rows: &[Row], current: usize, delta: isize) -> usize {
    if rows.is_empty() {
        return current;
    }
    let mut idx = current as isize;
    let len = rows.len() as isize;
    loop {
        idx = (idx + delta + len) % len;
        if row_is_selectable(&rows[idx as usize]) {
            return idx as usize;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn test_parse_worktree_list_output_groups_entries() {
        let temp = TempDir::new().expect("tempdir");
        let repo_root = temp.path();

        let shared_rel = Path::new(".swarm-hug/.shared/worktrees/shared-1");
        let project_rel = Path::new(".swarm-hug/alpha/worktrees/feat-1");

        let shared_abs = repo_root.join(shared_rel);
        let project_abs = repo_root.join(project_rel);

        let stdout = format!(
            "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n\
worktree {}\nHEAD def\nbranch refs/heads/shared-1\n\n\
worktree {}\nHEAD ghi\nbranch refs/heads/feat-1\n\n",
            repo_root.display(),
            shared_rel.display(),
            project_abs.display(),
        );

        let entries = parse_worktree_list_output(repo_root, &stdout);
        assert_eq!(entries.len(), 2);

        let shared = entries
            .iter()
            .find(|entry| entry.group == WorktreeGroup::Shared)
            .expect("shared entry");
        assert_eq!(shared.path, shared_abs);
        assert_eq!(shared.branch.as_deref(), Some("shared-1"));

        let project = entries
            .iter()
            .find(|entry| entry.group == WorktreeGroup::Project)
            .expect("project entry");
        assert_eq!(project.path, project_abs);
        assert_eq!(project.branch.as_deref(), Some("feat-1"));
        assert!(project.display.starts_with("alpha/feat-1 -"));
    }

    #[test]
    fn test_parse_worktree_list_output_supports_legacy_single_project_layout() {
        let temp = TempDir::new().expect("tempdir");
        let repo_root = temp.path();

        let legacy_rel = Path::new(".swarm-hug/worktrees/agent-A-Aaron");
        let stdout = format!(
            "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n\
worktree {}\nHEAD def\nbranch refs/heads/agent-aaron\n\n",
            repo_root.display(),
            legacy_rel.display(),
        );

        let entries = parse_worktree_list_output(repo_root, &stdout);
        assert_eq!(entries.len(), 1);

        let legacy = &entries[0];
        assert_eq!(legacy.group, WorktreeGroup::Project);
        assert_eq!(legacy.path, repo_root.join(legacy_rel));
        assert_eq!(legacy.branch.as_deref(), Some("agent-aaron"));
        assert!(legacy.display.starts_with("default/agent-aaron -"));
    }
}
