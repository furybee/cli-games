//! The launcher menu: pick a game with the arrow keys, Enter to play.
//!
//! Press `f` (or `/`) to filter: type and the list narrows by name, id or
//! description; Esc clears the filter, Enter launches the highlighted game.

use std::time::Duration;

use anyhow::Result;
use game_core::{GameEntry, registered_games};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::runner::Term;

/// Run the menu until the user picks a game (`Some`) or quits (`None`).
pub fn run_menu(term: &mut Term) -> Result<Option<&'static GameEntry>> {
    let games = registered_games();
    let mut state = ListState::default();
    state.select(Some(0));
    let mut filter = String::new();
    let mut filtering = false;

    loop {
        // Recompute the visible subset every frame and keep the cursor in range.
        let visible: Vec<&'static GameEntry> = games
            .iter()
            .copied()
            .filter(|g| matches(g, &filter))
            .collect();
        clamp_selection(&mut state, visible.len());

        term.draw(|f| draw(f, &visible, &mut state, filtering, &filter))?;

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if filtering {
            match key.code {
                KeyCode::Esc => {
                    filtering = false;
                    filter.clear();
                }
                KeyCode::Enter => {
                    if let Some(entry) = state.selected().and_then(|i| visible.get(i)) {
                        return Ok(Some(*entry));
                    }
                }
                KeyCode::Backspace => {
                    filter.pop();
                }
                KeyCode::Down => move_selection(&mut state, visible.len(), 1),
                KeyCode::Up => move_selection(&mut state, visible.len(), -1),
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    filter.push(c);
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
                KeyCode::Char('f') | KeyCode::Char('/') => filtering = true,
                KeyCode::Down | KeyCode::Char('j') => move_selection(&mut state, visible.len(), 1),
                KeyCode::Up | KeyCode::Char('k') => move_selection(&mut state, visible.len(), -1),
                KeyCode::Enter => {
                    if let Some(entry) = state.selected().and_then(|i| visible.get(i)) {
                        return Ok(Some(*entry));
                    }
                }
                _ => {}
            }
        }
    }
}

/// Case-insensitive substring match on name, id or description.
fn matches(entry: &GameEntry, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let q = query.to_lowercase();
    entry.name.to_lowercase().contains(&q)
        || entry.id.to_lowercase().contains(&q)
        || entry.description.to_lowercase().contains(&q)
}

fn clamp_selection(state: &mut ListState, len: usize) {
    if len == 0 {
        state.select(None);
    } else {
        let sel = state.selected().unwrap_or(0).min(len - 1);
        state.select(Some(sel));
    }
}

fn move_selection(state: &mut ListState, len: usize, delta: isize) {
    if len == 0 {
        return;
    }
    let current = state.selected().unwrap_or(0) as isize;
    let next = (current + delta).rem_euclid(len as isize);
    state.select(Some(next as usize));
}

fn draw(
    frame: &mut ratatui::Frame,
    visible: &[&'static GameEntry],
    state: &mut ListState,
    filtering: bool,
    filter: &str,
) {
    let [title_area, list_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let title = Paragraph::new(Line::from(vec![
        Span::styled("▶ ", Style::default().fg(Color::LightGreen)),
        Span::styled(
            "cli-games",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]))
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, title_area);

    let block_title = if filter.is_empty() {
        format!(" Games ({}) ", visible.len())
    } else {
        format!(" Games ({} match) ", visible.len())
    };

    if visible.is_empty() {
        let msg = if filter.is_empty() {
            "No games registered yet. See docs/ADD_A_GAME.md.".to_string()
        } else {
            format!("No games match \"{filter}\".")
        };
        let empty = Paragraph::new(msg)
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title(block_title));
        frame.render_widget(empty, list_area);
    } else {
        let items: Vec<ListItem> = visible
            .iter()
            .map(|e| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:<14}", e.name),
                        Style::default()
                            .fg(Color::LightGreen)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(e.description, Style::default().fg(Color::Gray)),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(block_title))
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("➜ ");
        frame.render_stateful_widget(list, list_area, state);
    }

    let footer = if filtering {
        Line::from(vec![
            Span::styled("Filter: ", Style::default().fg(Color::Yellow)),
            Span::styled(
                filter,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("▏", Style::default().fg(Color::Yellow)),
            Span::styled(
                "   ↑/↓ navigate · Enter play · Esc clear",
                Style::default().fg(Color::DarkGray),
            ),
        ])
    } else {
        Line::from(Span::styled(
            "↑/↓ navigate   ·   Enter play   ·   f filter   ·   q quit",
            Style::default().fg(Color::DarkGray),
        ))
    };
    frame.render_widget(
        Paragraph::new(footer).alignment(Alignment::Center),
        footer_area,
    );
}
