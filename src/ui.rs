use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph, Wrap},
};

use crate::app::{App, Screen};

const SELECTED_BG: Color = Color::Rgb(36, 72, 120);
const MUTED: Style = Style::new().fg(Color::DarkGray);
const SELECTED_STYLE: Style = Style::new()
    .fg(Color::White)
    .bg(SELECTED_BG)
    .add_modifier(Modifier::BOLD);
const WARNING_STYLE: Style = Style::new().fg(Color::Yellow);
const OUTER_MARGIN_X: u16 = 1;
const OUTER_MARGIN_Y: u16 = 1;
const SECTION_GAP_Y: u16 = 1;
const PANEL_GAP_X: u16 = 2;
const PANEL_PADDING_X: u16 = 1;

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let area = area.inner(Margin {
        vertical: OUTER_MARGIN_Y,
        horizontal: OUTER_MARGIN_X,
    });
    let warning_line = app.current_warning_line();
    let show_warning = !is_divider_line(&warning_line);
    let footer = app.footer_lines();
    let footer_height = if footer[1].is_empty() { 1 } else { 2 };

    let mut constraints = vec![Constraint::Length(1), Constraint::Length(1)];
    if show_warning {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(SECTION_GAP_Y));
    constraints.push(Constraint::Min(8));
    constraints.push(Constraint::Length(SECTION_GAP_Y));
    constraints.push(Constraint::Length(footer_height));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let title = format!("Cleanroom · {}", app.current_screen_title());
    frame.render_widget(
        Paragraph::new(Span::styled(
            title,
            Style::new().add_modifier(Modifier::BOLD),
        )),
        chunks[0],
    );

    let context = format!(
        "Context: {} · {}",
        app.current_context_label(),
        app.current_path_label()
    );
    frame.render_widget(Paragraph::new(Span::styled(context, MUTED)), chunks[1]);

    let main_chunk_index = if show_warning {
        frame.render_widget(
            Paragraph::new(Span::styled(warning_line, WARNING_STYLE)),
            chunks[2],
        );
        4
    } else {
        3
    };

    render_main_area(frame, chunks[main_chunk_index], app);

    let footer_lines = footer
        .into_iter()
        .filter(|line| !line.is_empty())
        .map(|line| Line::from(Span::styled(line, MUTED)))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(footer_lines), chunks[main_chunk_index + 2]);
}

fn render_main_area(frame: &mut Frame, area: Rect, app: &App) {
    match app.screen {
        Screen::SourceList => render_split_panels(
            frame,
            area,
            PanelSpec::new("Sources", None, app.source_rows(), app.selected_index()),
            TextPanelSpec::new("Details", app.detail_panel_lines()),
        ),
        Screen::CategorySummary => render_split_panels(
            frame,
            area,
            PanelSpec::new(
                "Categories",
                Some(app.category_table_header()),
                app.category_rows(),
                app.selected_index(),
            ),
            TextPanelSpec::new(&app.detail_panel_title(), app.detail_panel_lines()),
        ),
        Screen::EntryChecklist => render_split_panels(
            frame,
            area,
            PanelSpec::new(
                "Entries",
                Some(app.entry_table_header()),
                app.entry_rows(),
                app.selected_index(),
            ),
            TextPanelSpec::new(&app.detail_panel_title(), app.detail_panel_lines()),
        ),
        Screen::PreviewCleanup => render_split_panels(
            frame,
            area,
            PanelSpec::new("Preview", None, app.preview_rows(), None),
            TextPanelSpec::new("Summary", app.detail_panel_lines()),
        ),
        Screen::ConfirmCleanup => render_split_panels(
            frame,
            area,
            PanelSpec::new("Confirm Cleanup", None, app.confirmation_rows(), None),
            TextPanelSpec::new("Summary", app.detail_panel_lines()),
        ),
        Screen::CleanupResult => render_split_panels(
            frame,
            area,
            PanelSpec::new("Cleanup Result", None, app.result_rows(), None),
            TextPanelSpec::new("Summary", app.detail_panel_lines()),
        ),
    }
}

fn render_split_panels(frame: &mut Frame, area: Rect, left: PanelSpec, right: TextPanelSpec) {
    let columns = two_column_chunks(area);
    render_bordered_list(
        frame,
        columns[0],
        left.title.as_str(),
        left.header.as_deref(),
        &left.rows,
        left.selected,
    );
    render_bordered_text(frame, columns[1], right.title.as_str(), &right.rows);
}

fn render_bordered_list(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    header: Option<&str>,
    rows: &[String],
    selected: Option<usize>,
) {
    let block = bordered_block(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut constraints = Vec::new();
    if header.is_some() {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(0));
    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let list_area = if let Some(header_text) = header {
        frame.render_widget(
            Paragraph::new(Span::styled(
                header_text,
                Style::new().add_modifier(Modifier::BOLD),
            )),
            inner_chunks[0],
        );
        inner_chunks[1]
    } else {
        inner_chunks[0]
    };

    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let prefix = if Some(index) == selected {
                "› "
            } else {
                "  "
            };
            let text = format!("{prefix}{row}");
            if Some(index) == selected {
                ListItem::new(Line::from(Span::styled(text, SELECTED_STYLE)))
            } else {
                ListItem::new(Line::from(text))
            }
        })
        .collect();

    let mut state = ListState::default();
    if let Some(selected) = selected.filter(|_| !rows.is_empty()) {
        state.select(Some(selected));
    }

    frame.render_stateful_widget(List::new(items), list_area, &mut state);
}

fn render_bordered_text(frame: &mut Frame, area: Rect, title: &str, rows: &[String]) {
    let block = bordered_block(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = rows
        .iter()
        .map(|row| Line::from(Span::styled(row.clone(), MUTED)))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn bordered_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .padding(Padding::horizontal(PANEL_PADDING_X))
}

fn two_column_chunks(area: Rect) -> Vec<Rect> {
    let left_percent = if area.width <= 84 { 58 } else { 62 };
    let gap = if area.width > 48 { PANEL_GAP_X } else { 1 };
    let usable_width = area.width.saturating_sub(gap);
    let left_width = usable_width.saturating_mul(left_percent) / 100;
    let right_width = usable_width.saturating_sub(left_width);

    vec![
        Rect::new(area.x, area.y, left_width, area.height),
        Rect::new(
            area.x.saturating_add(left_width).saturating_add(gap),
            area.y,
            right_width,
            area.height,
        ),
    ]
}

fn is_divider_line(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && trimmed.chars().all(|ch| ch == '-')
}

struct PanelSpec {
    title: String,
    header: Option<String>,
    rows: Vec<String>,
    selected: Option<usize>,
}

impl PanelSpec {
    fn new(
        title: &str,
        header: Option<String>,
        rows: Vec<String>,
        selected: Option<usize>,
    ) -> Self {
        Self {
            title: title.to_string(),
            header,
            rows,
            selected,
        }
    }
}

struct TextPanelSpec {
    title: String,
    rows: Vec<String>,
}

impl TextPanelSpec {
    fn new(title: &str, rows: Vec<String>) -> Self {
        Self {
            title: title.to_string(),
            rows,
        }
    }
}
