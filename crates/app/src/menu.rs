//! The launcher menu: pick a game with the arrow keys, Enter to play.

use std::time::Duration;

use anyhow::Result;
use game_core::{GameEntry, registered_games};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::runner::Term;

/// Run the menu until the user picks a game (`Some`) or quits (`None`).
pub fn run_menu(term: &mut Term) -> Result<Option<&'static GameEntry>> {
    let games = registered_games();
    let mut state = ListState::default();
    if !games.is_empty() {
        state.select(Some(0));
    }

    loop {
        term.draw(|f| draw(f, &games, &mut state))?;

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
            KeyCode::Down | KeyCode::Char('j') => move_selection(&mut state, games.len(), 1),
            KeyCode::Up | KeyCode::Char('k') => move_selection(&mut state, games.len(), -1),
            KeyCode::Enter => {
                if let Some(entry) = state.selected().and_then(|i| games.get(i)) {
                    return Ok(Some(*entry));
                }
            }
            _ => {}
        }
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

fn draw(frame: &mut ratatui::Frame, games: &[&'static GameEntry], state: &mut ListState) {
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

    if games.is_empty() {
        let empty = Paragraph::new("No games registered yet. See docs/ADD_A_GAME.md.")
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).title(" Games "));
        frame.render_widget(empty, list_area);
    } else {
        let items: Vec<ListItem> = games
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
            .block(Block::default().borders(Borders::ALL).title(" Games "))
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("➜ ");
        frame.render_stateful_widget(list, list_area, state);
    }

    let footer = Paragraph::new("↑/↓ navigate   ·   Enter play   ·   q quit")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(footer, footer_area);
}
