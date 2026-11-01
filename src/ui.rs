use crate::app::{
    visible_home_items, Action, App, HomeItem, Overlay, ThumbnailState, ToastKind, View, QUALITIES,
};
use crate::theme::*;
use crate::util::{format_date, format_duration, format_views, relative_time, truncate};
use crate::yt::{self, Video};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Cell, Clear, List, ListItem, ListState, Padding, Paragraph, Row, Table, Wrap,
};
use ratatui::Frame;
use ratatui_image::StatefulImage;
use std::time::Instant;

/// The decorative underline motif under the search input — the same rule
/// ytmtui draws below its own search line.
const TOP_RULE: &str = "',─────,'───────',─────────,'─────────────'.────────────,'───────────`,──────────,'──────────,'─────────`,────────.`───────',──────',─────`,────.`───',──`.─";

pub fn draw(frame: &mut Frame, app: &mut App, now: Instant) {
    let area = frame.area();
    // No background paint: the terminal's own surface (transparency,
    // blur, wallpaper) shows through everywhere.
    let layout = Layout::vertical([
        Constraint::Length(1),                                          // search prompt
        Constraint::Length(1),                                          // decorative rule
        Constraint::Length(if app.view == View::Home { 1 } else { 0 }), // feed bar
        Constraint::Min(0),                                             // list + preview
        Constraint::Length(1),                                          // status
    ])
    .split(area);

    draw_search(frame, layout[0], app);
    draw_rule(frame, layout[1]);
    match app.view {
        View::Home => draw_home(frame, layout[3], app, now),
        View::Browse => draw_browse(frame, layout[3], app, now),
    }
    draw_status(frame, layout[4], app, now);

    if let Some(overlay) = &app.overlay {
        draw_overlay(frame, area, app, overlay);
    }
}

fn draw_home(frame: &mut Frame, area: Rect, app: &mut App, now: Instant) {
    let sections = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);

    draw_home_menu(frame, sections[0], app);
    draw_feed_content(frame, sections[1], app, now);
}

/// One compact line of feed shortcuts; the selected entry is highlighted in
/// the accent color.
fn draw_home_menu(frame: &mut Frame, area: Rect, app: &mut App) {
    if area.height == 0 || area.width < 20 {
        return;
    }

    // "1. My Feeds  ·  2. Watch Later  ·  …" on wide rows; narrow rows
    // drop to single-space separators so all five digits still fit.
    let separator = if area.width < 92 { " · " } else { "  ·  " };

    let items: Vec<&HomeItem> = visible_home_items().collect();
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(separator, Style::default().fg(*MUTED)));
        }
        let selected = index == app.home_index;
        let key_style = if selected {
            Style::default().fg(*ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(*MUTED)
        };
        let title_style = if selected {
            Style::default().fg(*ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(*TEXT)
        };
        // The bar shows the digit that jumps to each feed — w/l became
        // toggles, so the letters no longer all work as shortcuts. The
        // digit carries the dot ("1.") and the separator owns the padding,
        // so "·" sits in exactly two spaces on both sides.
        spans.push(Span::styled(format!("{}.", index + 1), key_style));
        spans.push(Span::styled(format!(" {}", item.title), title_style));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_browse(frame: &mut Frame, area: Rect, app: &mut App, now: Instant) {
    draw_feed_content(frame, area, app, now);
}

fn draw_feed_content(frame: &mut Frame, area: Rect, app: &mut App, now: Instant) {
    // List plus preview only; chrome lives in the search, feed-bar, and
    // status rows.
    let selected = app.selected_video().cloned();

    if area.width >= 88 {
        let columns = Layout::horizontal([Constraint::Percentage(64), Constraint::Percentage(36)])
            .split(area);
        draw_list(frame, columns[0], app, now);
        if let Some(video) = selected {
            draw_details(frame, columns[1], app, &video);
        }
    } else {
        draw_list(frame, area, app, now);
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

fn draw_status(frame: &mut Frame, area: Rect, app: &App, now: Instant) {
    // ytmtui footer layout: status message on the left, the keybind guide
    // pinned to the right with its keys picked out in the accent.
    // The feed bar on Home shows its digits by itself, so the 1-5 hint
    // only appears while there is no stacked feed to go back to.
    let mut hints: Vec<(&str, &str)> = vec![
        ("j/k", "move"),
        ("enter", "play"),
        ("m", "menu"),
        ("s", "save"),
        ("l", "like"),
    ];
    if app.can_go_back() {
        hints.push(("b", "back"));
    } else {
        hints.push(("1-5", "feeds"));
    }
    hints.extend_from_slice(&[("/", "search"), ("?", "help"), ("q", "quit")]);

    // Wide terminals keep the roomier two-space separator; narrow ones
    // thin the guide instead so the message never drops below a few
    // columns (the same budgeting ytmtui's footer does).
    let separator = if area.width >= 128 { "  " } else { " " };
    let (mut guide, mut guide_w) = guide_line(&hints, separator);
    let budget = usize::from(area.width).saturating_sub(guide_w + 1);
    if budget < 8 && hints.len() > 6 {
        let (thinned, thinned_w) = guide_line(&hints[..6], separator);
        if usize::from(area.width).saturating_sub(thinned_w + 1) > budget {
            guide = thinned;
            guide_w = thinned_w;
        }
    }
    let chunks = Layout::horizontal([Constraint::Fill(1), Constraint::Length(guide_w as u16 + 1)])
        .split(area);

    // Message left: live search guidance while the search row is focused,
    // otherwise toast → download → fetch spinner → library counts.
    let left = if let Some(Overlay::Search(input)) = &app.overlay {
        let hint = if crate::app::App::looks_like_url(&input.value) {
            "enter opens this url  ·  esc cancel".to_string()
        } else {
            match yt::parse_search(&input.value).filter {
                Some(filter) => format!("filter :{filter}  ·  enter search  ·  esc cancel"),
                None => "filters  :today  :week  :4k  :hd  :live  :short".to_string(),
            }
        };
        Line::from(Span::styled(
            truncate(&hint, chunks[0].width as usize),
            Style::default().fg(*MUTED),
        ))
    } else {
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
        } else if app.loading {
            // The removed header used to carry the fetch spinner; keep it
            // visible while a feed loads and no toast is up.
            (
                format!(
                    "{} {}",
                    app.spinner_frame(now),
                    truncate(&app.loading_label, 30)
                ),
                *MUTED,
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
        Line::from(Span::styled(
            truncate(&text, chunks[0].width as usize),
            Style::default().fg(color),
        ))
    };
    frame.render_widget(Paragraph::new(left), chunks[0]);
    frame.render_widget(Paragraph::new(guide).alignment(Alignment::Right), chunks[1]);
}

fn draw_overlay(frame: &mut Frame, area: Rect, app: &App, overlay: &Overlay) {
    match overlay {
        // The search prompt renders in its own row (see draw_search); its
        // live hints take over the status line while it is focused.
        Overlay::Search(_) => {}
        Overlay::Actions { index, actions } => draw_actions(frame, area, app, *index, actions),
        Overlay::Quality { index } => draw_quality(frame, area, app, *index),
        Overlay::Help => draw_help(frame, area),
    }
}

/// The `d` quality picker: a right-side panel (over the preview pane) listing
/// the offered qualities; Enter downloads the highlighted one.
fn draw_quality(frame: &mut Frame, area: Rect, app: &App, index: usize) {
    if area.width < 20 || area.height < 4 {
        return;
    }
    let width = (area.width / 4).clamp(22, 34);
    let height = (QUALITIES.len() as u16 + 3).min(area.height.saturating_sub(1));
    let rect = Rect::new(
        area.right().saturating_sub(width),
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);

    let sections = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(rect);
    let title = app
        .selected_video()
        .map(|video| {
            format!(
                "quality · {}",
                truncate(&video.title, width.saturating_sub(10) as usize)
            )
        })
        .unwrap_or_else(|| "quality".to_string());
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(title, Style::default().fg(*MUTED)))),
        sections[0],
    );

    let items: Vec<ListItem> = QUALITIES
        .iter()
        .map(|&(label, _)| {
            ListItem::new(Line::from(Span::styled(label, Style::default().fg(*TEXT))))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(index));
    let list = List::new(items)
        .highlight_symbol("▌ ")
        .highlight_style(Style::default().fg(*ACCENT).add_modifier(Modifier::BOLD));
    frame.render_stateful_widget(list, sections[1], &mut state);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "enter download · esc",
            Style::default().fg(*MUTED),
        ))),
        sections[2],
    );
}

/// ytmtui-style decorative rule under the search input: one full copy of the
/// motif (or the row width, whichever is shorter) so ultra-wide terminals
/// keep its intended shape, painted in the same pywal color ytmtui uses for
/// its rule (color6 / accent2).
fn draw_rule(frame: &mut Frame, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let width = usize::from(area.width).min(TOP_RULE.chars().count());
    let rule: String = TOP_RULE.chars().take(width).collect();
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            rule,
            Style::default().fg(*ACCENT2),
        ))),
        Rect::new(area.x, area.y, area.width, 1),
    );
}

/// The search prompt is part of the screen, not a popup: it sits in its own
/// top row (ytmtui style). `/` focuses it and the cursor appears; otherwise a
/// muted placeholder shows what it accepts.
fn draw_search(frame: &mut Frame, area: Rect, app: &App) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let input = match &app.overlay {
        Some(Overlay::Search(input)) => Some(input),
        _ => None,
    };
    let prefix = "/ ";
    let mut spans = vec![Span::styled(
        prefix,
        Style::default().fg(*ACCENT).add_modifier(Modifier::BOLD),
    )];
    let row = Rect::new(area.x, area.y, area.width, 1);
    match input {
        Some(input) => {
            spans.push(Span::styled(
                input.value.clone(),
                Style::default().fg(*TEXT),
            ));
            frame.render_widget(Paragraph::new(Line::from(spans)), row);
            let cursor_x = area
                .x
                .saturating_add(prefix.chars().count() as u16)
                .saturating_add(input.cursor_width() as u16)
                .min(row.right().saturating_sub(1));
            frame.set_cursor_position((cursor_x, row.y));
        }
        None => {
            spans.push(Span::styled(
                "search youtube · filters".to_string(),
                Style::default().fg(*MUTED),
            ));
            frame.render_widget(Paragraph::new(Line::from(spans)), row);
        }
    }
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
        hint_line(&[("/", "search"), ("m", "menu"), ("?", "help"), ("q", "quit")]),
        Line::from(Span::styled(
            "search accepts queries, colon filters, and plain URLs",
            Style::default().fg(*MUTED),
        )),
        Line::from(""),
        section("feeds"),
        hint_line(&[
            ("1", "my feeds"),
            ("2", "watch later"),
            ("3", "liked"),
            ("4", "saved"),
            ("5", "history"),
        ]),
        Line::from(Span::styled(
            "letters F S H f still jump to my feeds · saved · history · subs",
            Style::default().fg(*MUTED),
        )),
        Line::from(""),
        section("list"),
        hint_line(&[
            ("j/k/n/p", "move"),
            ("g/G", "first/last"),
            ("enter/space", "play"),
            ("s", "save/unsave"),
            ("r", "remove saved"),
            ("l", "like/unlike"),
            ("w", "watch later"),
            ("A", "audio only"),
            ("d", "pick quality"),
            ("D", "download audio"),
            ("c", "channel"),
            ("y", "copy"),
            ("o", "open"),
            ("R", "refresh"),
            ("b/esc", "back"),
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
            "l and w write real likes / Watch Later to your YouTube",
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

/// The right-side key guide (ytmtui footer style): key picked out in the
/// accent bold, label muted, custom separator — returned with its rendered
/// width so the status layout can budget the message next to it.
fn guide_line(hints: &[(&str, &str)], separator: &str) -> (Line<'static>, usize) {
    let mut spans = Vec::new();
    let mut width = 0usize;
    for (index, (key_value, label)) in hints.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                separator.to_string(),
                Style::default().fg(*MUTED),
            ));
            width += separator.chars().count();
        }
        spans.push(Span::styled(
            (*key_value).to_string(),
            Style::default().fg(*ACCENT).add_modifier(Modifier::BOLD),
        ));
        width += key_value.chars().count();
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(*MUTED),
        ));
        width += 1 + label.chars().count();
    }
    (Line::from(spans), width)
}

fn section(label: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        label,
        Style::default().fg(*ACCENT).add_modifier(Modifier::BOLD),
    ))
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
