use crate::app::{
    visible_home_items, Action, App, HomeItem, Overlay, TextInput, ThumbnailState, ToastKind, View,
};
use crate::theme::*;
use crate::util::{
    display_width, format_date, format_duration, format_views, relative_time, truncate,
};
use crate::yt::{self, Video};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Padding, Paragraph, Row,
    Table, Wrap,
};
use ratatui::Frame;
use ratatui_image::StatefulImage;
use std::time::Instant;

pub fn draw(frame: &mut Frame, app: &mut App, now: Instant) {
    let area = frame.area();
    if bg_active() {
        frame.render_widget(Block::default().style(Style::new().bg(*BG)), area);
    }
    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);

    draw_header(frame, layout[0], app, now);
    match app.view {
        View::Home => draw_home(frame, layout[1], app),
        View::Browse => draw_browse(frame, layout[1], app, now),
        View::Settings => draw_settings(frame, layout[1], app),
    }
    draw_status(frame, layout[2], app, now);

    if let Some(overlay) = &app.overlay {
        draw_overlay(frame, area, app, overlay);
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App, now: Instant) {
    let chunks = Layout::horizontal([Constraint::Fill(1), Constraint::Length(28)]).split(area);

    let mut spans = vec![Span::styled(
        truncate(
            &app.current_title(),
            chunks[0].width.saturating_sub(10) as usize,
        ),
        Style::default().fg(*TEXT).add_modifier(Modifier::BOLD),
    )];
    if app.can_go_back() {
        spans.push(Span::styled("  b back", Style::default().fg(*MUTED)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[0]);

    let right = if app.loading {
        Line::from(vec![
            Span::styled(app.spinner_frame(now), Style::default().fg(*ACCENT)),
            Span::styled(
                format!(" {}", truncate(&app.loading_label, 22)),
                Style::default().fg(*MUTED),
            ),
        ])
    } else if app.view == View::Browse {
        Line::from(Span::styled(
            format!("{} items ", app.feed.items.len()),
            Style::default().fg(*MUTED),
        ))
    } else {
        Line::from("")
    };
    frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), chunks[1]);
}

fn draw_home(frame: &mut Frame, area: Rect, app: &mut App) {
    let show_intro = area.height >= 10;
    let intro_height = if show_intro { 2 } else { 0 };

    let sections = Layout::vertical([
        Constraint::Length(intro_height),
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .split(area);

    if show_intro {
        let lines = vec![
            Line::from(Span::styled(
                "YouTube at terminal velocity",
                Style::default().fg(*MUTED),
            )),
            Line::from(""),
        ];
        frame.render_widget(Paragraph::new(Text::from(lines)), sections[0]);
    }

    draw_home_menu(frame, sections[1], app);
}

/// One horizontal row of bordered cells, directly under the search prompt.
fn draw_home_menu(frame: &mut Frame, area: Rect, app: &mut App) {
    if area.height < 2 || area.width < 20 {
        return;
    }

    let items: Vec<&HomeItem> = visible_home_items().collect();
    let count = items.len();
    let widths: Vec<Constraint> = vec![Constraint::Ratio(1, count as u32); count];
    let cells = Layout::horizontal(widths).spacing(1).split(area);

    for (index, (item, cell)) in items.iter().zip(cells.iter()).enumerate() {
        let selected = index == app.home_index;
        let (border, title, key) = if selected {
            (
                Style::default().fg(*ACCENT).add_modifier(Modifier::BOLD),
                Style::default().fg(*TEXT).add_modifier(Modifier::BOLD),
                Style::default().fg(*ACCENT).add_modifier(Modifier::BOLD),
            )
        } else {
            (
                Style::default().fg(*MUTED),
                Style::default().fg(*TEXT),
                Style::default().fg(*MUTED),
            )
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border);
        let line = Line::from(vec![
            Span::styled(format!(" {} ", item.key), key),
            Span::raw(" "),
            Span::styled(item.title.to_string(), title),
        ]);
        frame.render_widget(
            Paragraph::new(line).block(block).wrap(Wrap { trim: true }),
            *cell,
        );
    }
}

fn draw_browse(frame: &mut Frame, area: Rect, app: &mut App, now: Instant) {
    let sections = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);

    let title = truncate(
        &app.feed.title,
        sections[0].width.saturating_sub(30) as usize,
    );
    let left = Line::from(Span::styled(
        title,
        Style::default().add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(Paragraph::new(left), sections[0]);

    let right = if app.loading {
        Line::from(vec![
            Span::styled(app.spinner_frame(now), Style::default().fg(*ACCENT)),
            Span::styled(
                format!(" {}", truncate(&app.loading_label, 24)),
                Style::default().fg(*MUTED),
            ),
        ])
    } else if app.feed.subtitle.is_empty() {
        Line::from("")
    } else {
        Line::from(Span::styled(
            truncate(&app.feed.subtitle, 28),
            Style::default().fg(*MUTED),
        ))
    };
    frame.render_widget(
        Paragraph::new(right).alignment(Alignment::Right),
        sections[0],
    );

    let body = sections[1];
    let selected = app.selected_video().cloned();

    if body.width >= 100 {
        let columns = Layout::horizontal([Constraint::Percentage(68), Constraint::Percentage(32)])
            .split(body);
        draw_list(frame, columns[0], app, now);
        if let Some(video) = selected {
            draw_details(frame, columns[1], app, &video);
        }
    } else {
        draw_list(frame, body, app, now);
    }
}

fn draw_list(frame: &mut Frame, area: Rect, app: &mut App, now: Instant) {
    if app.feed.items.is_empty() {
        draw_empty(frame, area, app, now);
        return;
    }

    let show_views = area.width >= 58;
    let mut widths = vec![
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(7),
    ];
    if show_views {
        widths.push(Constraint::Length(7));
    }

    let header = Row::new({
        let mut cells = vec![
            Cell::from(""),
            Cell::from(Span::styled("title", Style::default().fg(*MUTED))),
            Cell::from(Span::styled("len", Style::default().fg(*MUTED))),
        ];
        if show_views {
            cells.push(Cell::from(Span::styled(
                "views",
                Style::default().fg(*MUTED),
            )));
        }
        cells
    });

    let title_width =
        area.width
            .saturating_sub(if show_views { 2 + 7 + 7 + 5 } else { 2 + 7 + 3 }) as usize;

    let rows: Vec<Row> = app
        .feed
        .items
        .iter()
        .enumerate()
        .map(|(index, video)| {
            let mut title_spans = vec![Span::styled(
                truncate(&video.title, title_width.max(8)),
                Style::default().fg(*TEXT),
            )];
            if video.is_live() {
                title_spans.push(Span::styled(" live", Style::default().fg(*ERROR)));
            }
            if app.store.is_liked(&video.id) {
                title_spans.push(Span::styled(" ♥", Style::default().fg(*ERROR)));
            }
            if app.store.is_watch_later(&video.id) {
                title_spans.push(Span::styled(" »", Style::default().fg(*WARN)));
            }
            if app.store.is_saved(&video.id) {
                title_spans.push(Span::styled(" ·", Style::default().fg(*ACCENT)));
            }

            let mut cells = vec![
                Cell::from(Span::styled(
                    format!("{:>2}", index + 1),
                    Style::default().fg(*MUTED),
                )),
                Cell::from(Line::from(title_spans)),
                Cell::from(Span::styled(
                    format_duration(video.duration),
                    Style::default().fg(*MUTED),
                )),
            ];
            if show_views {
                cells.push(Cell::from(Span::styled(
                    format_views(video.view_count),
                    Style::default().fg(*MUTED),
                )));
            }
            Row::new(cells)
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .highlight_symbol("▌ ")
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_empty(frame: &mut Frame, area: Rect, app: &App, now: Instant) {
    let message = if app.loading {
        format!("{} {}", app.spinner_frame(now), app.loading_label)
    } else if let Some(notice) = &app.feed.notice {
        notice.clone()
    } else {
        "no results · press / to search".to_string()
    };
    let lines = vec![Line::from(Span::styled(
        truncate(&message, area.width.saturating_sub(4) as usize),
        Style::default().fg(*MUTED),
    ))];
    let chunks = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).alignment(Alignment::Center),
        chunks[1],
    );
}

fn draw_details(frame: &mut Frame, area: Rect, app: &mut App, video: &Video) {
    let inner = Block::default()
        .padding(Padding::new(2, 0, 0, 0))
        .inner(area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut body = inner;
    if video.thumbnail.is_some() {
        let rows = thumbnail_rows(app, inner);
        if rows > 0 {
            let thumb_area = Rect::new(inner.x, inner.y, inner.width, rows);
            if let ThumbnailState::Ready { id, protocol } = &mut app.thumbnail {
                if id == &video.id {
                    frame.render_stateful_widget(StatefulImage::new(), thumb_area, protocol);
                }
            }
            body = Rect::new(
                inner.x,
                inner.y + rows + 1,
                inner.width,
                inner.height.saturating_sub(rows + 1),
            );
        }
    }
    if body.height == 0 {
        return;
    }

    let mut lines = Vec::new();
    if video.is_live() {
        lines.push(Line::from(Span::styled(
            "live",
            Style::default().fg(*ERROR).add_modifier(Modifier::BOLD),
        )));
    }
    if app.store.is_liked(&video.id) {
        lines.push(Line::from(Span::styled(
            "liked",
            Style::default().fg(*ERROR).add_modifier(Modifier::BOLD),
        )));
    }
    if app.store.is_watch_later(&video.id) {
        lines.push(Line::from(Span::styled(
            "watch later",
            Style::default().fg(*WARN).add_modifier(Modifier::BOLD),
        )));
    }
    if app.store.is_saved(&video.id) {
        lines.push(Line::from(Span::styled(
            "saved",
            Style::default().fg(*ACCENT).add_modifier(Modifier::BOLD),
        )));
    }
    if !lines.is_empty() {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        video.title.clone(),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        video.channel.clone(),
        Style::default().fg(*MUTED),
    )));
    lines.push(Line::from(Span::styled(
        format!(
            "{} · {} views · {}",
            format_duration(video.duration),
            format_views(video.view_count),
            relative_time(video.timestamp)
        ),
        Style::default().fg(*MUTED),
    )));
    lines.push(Line::from(Span::styled(
        video
            .upload_date
            .clone()
            .unwrap_or_else(|| format_date(video.timestamp)),
        Style::default().fg(*MUTED),
    )));

    if let Some(description) = &video.description {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            truncate(description, 360),
            Style::default().fg(*MUTED),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        truncate(&video.url, inner.width.saturating_sub(2) as usize),
        Style::default().fg(*MUTED),
    )));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        body,
    );
}

/// Rows to reserve for a 16:9 thumbnail, using the terminal's cell pixel size
/// when known so the image is not distorted.
fn thumbnail_rows(app: &App, inner: Rect) -> u16 {
    let max_rows = inner.height.saturating_sub(6);
    if max_rows < 4 {
        return 0;
    }
    let (cell_width, cell_height) = app
        .picker
        .as_ref()
        .map(|picker| picker.font_size())
        .unwrap_or((8, 16));
    let cell_width = cell_width.max(1) as u32;
    let cell_height = cell_height.max(1) as u32;
    let pixel_width = inner.width as u32 * cell_width;
    let pixel_height = pixel_width * 9 / 16;
    let rows = pixel_height.div_ceil(cell_height) as u16;
    rows.clamp(4, max_rows)
}

fn draw_settings(frame: &mut Frame, area: Rect, app: &App) {
    let inner = Block::default()
        .padding(Padding::new(2, 2, 1, 0))
        .inner(area);

    let lines = vec![
        section("config"),
        kv("config", &app.paths.config_file().display().to_string()),
        kv("state", &app.paths.state_file().display().to_string()),
        kv("cache", &app.paths.feed_cache_dir().display().to_string()),
        Line::from(""),
        section("player"),
        kv("player", &app.config.player),
        kv(
            "cookies",
            app.config.cookies_browser.as_deref().unwrap_or("off"),
        ),
        kv("results", &app.config.results.to_string()),
        kv("downloads", &app.config.download_dir.display().to_string()),
        Line::from(""),
        kv("video args", &app.config.video_args.join(" ")),
        kv("audio args", &app.config.audio_args.join(" ")),
        Line::from(""),
        section("about"),
        Line::from(Span::styled(
            format!("utui v{}", env!("CARGO_PKG_VERSION")),
            Style::default().fg(*TEXT),
        )),
        Line::from(Span::styled(
            "native Rust · yt-dlp · mpv",
            Style::default().fg(*MUTED),
        )),
        Line::from(""),
        Line::from(vec![
            key("e"),
            Span::styled(" edit config  ", Style::default().fg(*MUTED)),
            key("o"),
            Span::styled(" downloads  ", Style::default().fg(*MUTED)),
            key("r"),
            Span::styled(" reload", Style::default().fg(*MUTED)),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_status(frame: &mut Frame, area: Rect, app: &App, now: Instant) {
    let chunks = Layout::horizontal([Constraint::Fill(1), Constraint::Length(48)]).split(area);

    let hints: &[(&str, &str)] = match app.view {
        View::Home => &[
            ("←→", "select"),
            ("enter", "open"),
            ("/", "search"),
            ("?", "help"),
            ("q", "quit"),
        ],
        View::Browse => &[
            ("enter", "actions"),
            ("p", "play"),
            ("a", "audio"),
            ("d", "download"),
            ("l", "like"),
            ("W", "later"),
            ("s", "save"),
            ("/", "search"),
            ("b", "back"),
            ("?", "help"),
        ],
        View::Settings => &[
            ("e", "edit"),
            ("o", "downloads"),
            ("r", "reload"),
            ("esc", "home"),
            ("?", "help"),
        ],
    };
    frame.render_widget(Paragraph::new(hint_line(hints)), chunks[0]);

    let (text, color) = if let Some(toast) = app.active_toast(now) {
        let prefix = match toast.kind {
            ToastKind::Info => "· ",
            ToastKind::Success => "✓ ",
            ToastKind::Error => "! ",
        };
        let color = match toast.kind {
            ToastKind::Info => *MUTED,
            ToastKind::Success => *SUCCESS,
            ToastKind::Error => *ERROR,
        };
        (format!("{prefix}{}", toast.text), color)
    } else if let Some(download) = &app.download {
        (
            format!("↓ {}%  {}", download.percent, truncate(&download.title, 34)),
            *WARN,
        )
    } else {
        (
            format!(
                "{} saved · {} history",
                app.store.saved.len(),
                app.store.history.len()
            ),
            *MUTED,
        )
    };

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate(&text, 47),
            Style::default().fg(color),
        )))
        .alignment(Alignment::Right),
        chunks[1],
    );
}

fn draw_overlay(frame: &mut Frame, area: Rect, app: &App, overlay: &Overlay) {
    match overlay {
        Overlay::Search(input) => draw_prompt(frame, area, input),
        Overlay::Actions { index, actions } => draw_actions(frame, area, app, *index, actions),
        Overlay::Help => draw_help(frame, area),
    }
}

fn draw_prompt(frame: &mut Frame, area: Rect, input: &TextInput) {
    if area.height == 0 {
        return;
    }
    let height = 2.min(area.height);
    let rect = Rect::new(area.x, area.y, area.width, height);
    frame.render_widget(Clear, rect);

    let prefix = "/ ";
    let prefix_width = prefix.chars().count() as u16;
    let mut spans = vec![
        Span::styled(
            prefix.to_string(),
            Style::default().fg(*ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            input.value.clone(),
            Style::default()
                .fg(*TEXT)
                .add_modifier(Modifier::UNDERLINED),
        ),
    ];
    // Extend the underline across the row so the input reads as a field.
    let filler = rect
        .width
        .saturating_sub(prefix_width)
        .saturating_sub(display_width(&input.value) as u16);
    if filler > 0 {
        spans.push(Span::styled(
            " ".repeat(filler as usize),
            Style::default().add_modifier(Modifier::UNDERLINED),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect::new(rect.x, rect.y, rect.width, 1),
    );

    if rect.height > 1 {
        let hint = if crate::app::App::looks_like_url(&input.value) {
            "enter opens this url  ·  esc cancel".to_string()
        } else {
            match yt::parse_search(&input.value).filter {
                Some(filter) => format!("filter :{filter}  ·  enter search  ·  esc cancel"),
                None => "paste a url to open  ·  filters  :today  :week  :4k  :hd  :live  :short"
                    .to_string(),
            }
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                truncate(&hint, rect.width.saturating_sub(2) as usize),
                Style::default().fg(*MUTED),
            ))),
            Rect::new(rect.x, rect.y + 1, rect.width, 1),
        );
    }

    let cursor_x = rect
        .x
        .saturating_add(prefix_width)
        .saturating_add(input.cursor_width() as u16)
        .min(rect.right().saturating_sub(1));
    frame.set_cursor_position((cursor_x, rect.y));
}

fn draw_actions(frame: &mut Frame, area: Rect, app: &App, index: usize, actions: &[Action]) {
    let width = area.width.saturating_sub(8).clamp(32, 64).min(area.width);
    let height = (actions.len() as u16 + 2).min(area.height.saturating_sub(2));
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);

    let sections = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(rect);
    let title = app
        .selected_video()
        .map(|video| truncate(&video.title, width.saturating_sub(2) as usize))
        .unwrap_or_else(|| "actions".to_string());
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(title, Style::default().fg(*MUTED)))),
        sections[0],
    );

    let items: Vec<ListItem> = actions
        .iter()
        .map(|action| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:>2}  ", action.key),
                    Style::default().fg(*ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(action.label, Style::default().fg(*TEXT)),
            ]))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(index));
    let list = List::new(items)
        .highlight_symbol("▌ ")
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_stateful_widget(list, sections[1], &mut state);
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let width = area.width.saturating_sub(6).clamp(36, 86).min(area.width);
    let height = area.height.saturating_sub(2).min(34).min(area.height);
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);

    let inner = Block::default()
        .padding(Padding::new(2, 2, 0, 0))
        .inner(rect);

    let lines = vec![
        section("global"),
        hint_line(&[("/", "search"), ("?", "help"), ("q", "quit")]),
        Line::from(Span::styled(
            "search accepts queries, colon filters, and plain URLs",
            Style::default().fg(*MUTED),
        )),
        Line::from(""),
        section("browse"),
        hint_line(&[
            ("j/k", "move"),
            ("g/G", "first/last"),
            ("enter", "actions"),
            ("p", "play"),
            ("a", "audio"),
            ("d/D", "download"),
            ("l", "like"),
            ("W", "later"),
            ("w", "watch later page"),
            ("s", "save"),
            ("c", "channel"),
            ("y", "copy"),
            ("o", "open"),
            ("r", "refresh"),
            ("b", "back"),
        ]),
        Line::from(""),
        section("filters"),
        Line::from(Span::styled(
            filter_names(0, 10),
            Style::default().fg(*MUTED),
        )),
        Line::from(Span::styled(
            filter_names(10, 21),
            Style::default().fg(*MUTED),
        )),
        Line::from(Span::styled(
            "recall  !1  !2  !3",
            Style::default().fg(*MUTED),
        )),
        Line::from(""),
        section("cookies"),
        Line::from(Span::styled(
            "set cookies_browser in config.toml for subscriptions,",
            Style::default().fg(*MUTED),
        )),
        Line::from(Span::styled(
            "watch later, and liked videos",
            Style::default().fg(*MUTED),
        )),
        Line::from(""),
        section("account"),
        Line::from(Span::styled(
            "l and W write real likes / Watch Later to your YouTube",
            Style::default().fg(*MUTED),
        )),
        Line::from(Span::styled(
            "account when cookies_browser is set (account_sync = true);",
            Style::default().fg(*MUTED),
        )),
        Line::from(Span::styled(
            "otherwise they toggle local mirrors only",
            Style::default().fg(*MUTED),
        )),
        Line::from(""),
        section("downloads"),
        Line::from(Span::styled(
            "download_dir/video and download_dir/audio",
            Style::default().fg(*MUTED),
        )),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn key(value: &'static str) -> Span<'static> {
    Span::styled(
        value,
        Style::default().fg(*ACCENT).add_modifier(Modifier::BOLD),
    )
}

fn hint_line(hints: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (key_value, label)) in hints.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("  ", Style::default().fg(*MUTED)));
        }
        spans.push(Span::styled(
            (*key_value).to_string(),
            Style::default().fg(*ACCENT).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(*MUTED),
        ));
    }
    Line::from(spans)
}

fn section(label: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        label,
        Style::default().fg(*ACCENT).add_modifier(Modifier::BOLD),
    ))
}

fn kv(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<12}"), Style::default().fg(*MUTED)),
        Span::styled(value.to_string(), Style::default().fg(*TEXT)),
    ])
}

fn filter_names(start: usize, end: usize) -> String {
    yt::FILTER_HELP
        .iter()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join("  ")
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Feed, FeedKind};
    use crate::config::{Config, Paths};
    use crate::store::Store;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn app() -> App {
        let root = std::env::temp_dir().join(format!("utui-ui-test-{}", std::process::id()));
        let paths = Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
        };
        App::new(paths, Config::default(), Store::default())
    }

    #[test]
    fn renders_home_without_panicking() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app();
        terminal
            .draw(|frame| draw(frame, &mut app, Instant::now()))
            .unwrap();
    }

    #[test]
    fn renders_browse_without_panicking() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app();
        app.view = View::Browse;
        app.feed = Feed {
            title: "Search · rust".into(),
            source: "https://example.com".into(),
            kind: FeedKind::Search,
            subtitle: "SEARCH · 1 RESULTS".into(),
            items: vec![Video {
                id: "abc".into(),
                title: "A very long video title about Rust and terminals".into(),
                url: "https://www.youtube.com/watch?v=abc".into(),
                channel: "Example channel".into(),
                channel_url: Some("https://www.youtube.com/@example".into()),
                duration: Some(3_661),
                view_count: Some(1_200_000),
                timestamp: Some(1_760_000_000),
                upload_date: None,
                thumbnail: None,
                description: Some("A description".into()),
                live: false,
            }],
            notice: None,
            auth: false,
            fetched_at: 0,
        };
        app.table_state.select(Some(0));
        terminal
            .draw(|frame| draw(frame, &mut app, Instant::now()))
            .unwrap();
    }

    #[test]
    fn renders_details_with_thumbnail_without_panicking() {
        let backend = TestBackend::new(140, 44);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app();
        app.view = View::Browse;
        app.feed = Feed {
            title: "Search · rust".into(),
            source: "https://example.com".into(),
            kind: FeedKind::Search,
            subtitle: "SEARCH · 1 RESULTS".into(),
            items: vec![Video {
                id: "abc".into(),
                title: "A video with a thumbnail".into(),
                url: "https://www.youtube.com/watch?v=abc".into(),
                channel: "Example channel".into(),
                channel_url: None,
                duration: Some(600),
                view_count: Some(42),
                timestamp: Some(1_760_000_000),
                upload_date: None,
                thumbnail: Some("https://example.com/thumb.jpg".into()),
                description: None,
                live: false,
            }],
            notice: None,
            auth: false,
            fetched_at: 0,
        };
        app.table_state.select(Some(0));

        let picker = crate::thumb::halfblocks_picker();
        let image = image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
            320,
            180,
            image::Rgba([20u8, 40, 60, 255]),
        ));
        let protocol = picker.new_resize_protocol(image);
        app.set_picker(picker);
        app.thumbnail = ThumbnailState::Ready {
            id: "abc".into(),
            protocol,
        };

        terminal
            .draw(|frame| draw(frame, &mut app, Instant::now()))
            .unwrap();
    }
}
