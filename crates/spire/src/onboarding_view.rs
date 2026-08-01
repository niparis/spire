//! Reusable section components for the onboarding editor.
//!
//! Every onboarding question is one of four interaction shapes. A section
//! describes the shape and the rows to show; this module owns the cursor,
//! navigation, key hints, and rendering for all of them. Adding a question is
//! then one view description and one mutation, instead of another arm in the
//! renderer, the key handler, and the footer — which is how the sections
//! drifted apart in the first place.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crossterm::event::KeyCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Muted,
    Warning,
    Good,
}

impl Tone {
    fn style(self) -> Style {
        match self {
            Self::Normal => Style::default(),
            Self::Muted => Style::default().fg(Color::DarkGray),
            Self::Warning => Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            Self::Good => Style::default().fg(Color::Green),
        }
    }
}

/// Pick exactly one entry. `current` marks the value already held by the model
/// so an operator can tell a highlighted row from a chosen one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceRow {
    pub label: String,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToggleRow {
    pub label: String,
    pub selected: bool,
}

/// A named setting whose value advances through a fixed list of alternatives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleRow {
    pub name: String,
    pub value: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadoutRow {
    pub text: String,
    pub tone: Tone,
}

impl ReadoutRow {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tone: Tone::Normal,
        }
    }

    pub fn toned(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionView {
    Choose {
        rows: Vec<ChoiceRow>,
        empty: String,
    },
    /// Toggling writes straight through to the model, so leaving the section is
    /// the only confirmation step there is.
    Toggle {
        rows: Vec<ToggleRow>,
        empty: String,
    },
    Cycle {
        headers: (&'static str, &'static str),
        rows: Vec<CycleRow>,
        empty: String,
    },
    Readout {
        rows: Vec<ReadoutRow>,
        activates: Option<&'static str>,
    },
}

/// What a key press means for the section that owns the view. Navigation is
/// resolved by the view itself and never reaches the section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionAction {
    Activate(usize),
    Toggle(usize),
}

impl SectionView {
    pub fn len(&self) -> usize {
        match self {
            Self::Choose { rows, .. } => rows.len(),
            Self::Toggle { rows, .. } => rows.len(),
            Self::Cycle { rows, .. } => rows.len(),
            Self::Readout { activates, .. } => usize::from(activates.is_some()),
        }
    }

    /// Moves the cursor and reports the action a section must apply. The cursor
    /// is clamped here because a view rebuilt from changed discovery data can be
    /// shorter than it was when the cursor last moved.
    pub fn navigate(&self, key: KeyCode, cursor: &mut usize) -> Option<SectionAction> {
        let last = self.len().saturating_sub(1);
        *cursor = (*cursor).min(last);
        match key {
            KeyCode::Up => {
                *cursor = cursor.saturating_sub(1);
                None
            }
            KeyCode::Down => {
                *cursor = (*cursor + 1).min(last);
                None
            }
            KeyCode::Char(' ') if matches!(self, Self::Toggle { .. }) && self.len() > 0 => {
                Some(SectionAction::Toggle(*cursor))
            }
            KeyCode::Enter if !matches!(self, Self::Toggle { .. }) && self.len() > 0 => {
                Some(SectionAction::Activate(*cursor))
            }
            _ => None,
        }
    }

    /// The keys this shape responds to, so the footer cannot promise an
    /// interaction the section does not implement.
    pub fn key_hint(&self) -> &'static str {
        match self {
            Self::Choose { .. } => "up/down move  Enter select",
            Self::Toggle { .. } => "up/down move  Space toggle (saved immediately)",
            Self::Cycle { .. } => "up/down move  Enter next value",
            Self::Readout {
                activates: Some(_), ..
            } => "Enter write",
            Self::Readout { .. } => "",
        }
    }

    pub fn lines(&self, cursor: usize) -> Vec<Line<'static>> {
        let cursor_at = |row: usize| if row == cursor { "> " } else { "  " };
        match self {
            Self::Choose { rows, empty } => {
                if rows.is_empty() {
                    return vec![Line::from(Span::styled(empty.clone(), Tone::Muted.style()))];
                }
                rows.iter()
                    .enumerate()
                    .map(|(index, row)| {
                        Line::from(format!(
                            "{}{} {}",
                            cursor_at(index),
                            if row.current { "(*)" } else { "( )" },
                            row.label
                        ))
                    })
                    .collect()
            }
            Self::Toggle { rows, empty } => {
                if rows.is_empty() {
                    return vec![Line::from(Span::styled(empty.clone(), Tone::Muted.style()))];
                }
                rows.iter()
                    .enumerate()
                    .map(|(index, row)| {
                        Line::from(format!(
                            "{}[{}] {}",
                            cursor_at(index),
                            if row.selected { "x" } else { " " },
                            row.label
                        ))
                    })
                    .collect()
            }
            Self::Cycle {
                headers,
                rows,
                empty,
            } => {
                if rows.is_empty() {
                    return vec![Line::from(Span::styled(empty.clone(), Tone::Muted.style()))];
                }
                // Naming both columns is what tells the operator which side is
                // Spire's own vocabulary and which side came from Linear.
                let width = rows
                    .iter()
                    .map(|row| row.name.chars().count())
                    .chain(std::iter::once(headers.0.chars().count()))
                    .max()
                    .unwrap_or(0);
                let mut lines = vec![Line::from(Span::styled(
                    format!("  {:<width$}   {}", headers.0, headers.1),
                    Tone::Muted.style(),
                ))];
                lines.extend(rows.iter().enumerate().map(|(index, row)| {
                    let mut text =
                        format!("{}{:<width$} = {}", cursor_at(index), row.name, row.value);
                    if let Some(note) = &row.note {
                        text.push_str(&format!("  ({note})"));
                    }
                    Line::from(text)
                }));
                lines
            }
            Self::Readout { rows, .. } => rows
                .iter()
                .map(|row| Line::from(Span::styled(row.text.clone(), row.tone.style())))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toggle_view() -> SectionView {
        SectionView::Toggle {
            rows: vec![
                ToggleRow {
                    label: "one".to_owned(),
                    selected: false,
                },
                ToggleRow {
                    label: "two".to_owned(),
                    selected: true,
                },
            ],
            empty: "nothing".to_owned(),
        }
    }

    #[test]
    fn toggling_is_the_only_activation_a_toggle_view_reports() {
        let view = toggle_view();
        let mut cursor = 1;
        assert_eq!(
            view.navigate(KeyCode::Char(' '), &mut cursor),
            Some(SectionAction::Toggle(1))
        );
        assert_eq!(view.navigate(KeyCode::Enter, &mut cursor), None);
    }

    #[test]
    fn a_cursor_left_past_the_end_is_clamped_before_it_acts() {
        let view = toggle_view();
        let mut cursor = 9;
        assert_eq!(
            view.navigate(KeyCode::Char(' '), &mut cursor),
            Some(SectionAction::Toggle(1))
        );
        assert_eq!(cursor, 1);
    }

    #[test]
    fn an_empty_view_reports_no_action_and_renders_its_hint() {
        let view = SectionView::Choose {
            rows: Vec::new(),
            empty: "no teams loaded".to_owned(),
        };
        let mut cursor = 0;
        assert_eq!(view.navigate(KeyCode::Enter, &mut cursor), None);
        let rendered = view.lines(0)[0].to_string();
        assert!(rendered.contains("no teams loaded"), "{rendered}");
    }

    #[test]
    fn a_cycle_view_names_both_columns() {
        let view = SectionView::Cycle {
            headers: ("spire role", "linear state"),
            rows: vec![CycleRow {
                name: "ready".to_owned(),
                value: "Ready".to_owned(),
                note: None,
            }],
            empty: String::new(),
        };
        let header = view.lines(0)[0].to_string();
        assert!(
            header.contains("spire role") && header.contains("linear state"),
            "{header}"
        );
    }
}
