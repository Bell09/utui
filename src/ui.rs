use crate::app::{
    visible_home_items, App, FeedKind, HomeItem, Overlay, ThumbnailState, ToastKind, View,
    QUALITIES,
};
use crate::theme::Theme;
use crate::util::{format_date, format_duration, format_views, relative_time, truncate};
use crate::yt::{self, Video};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Cell, Clear, List, ListItem, ListState, Padding, Paragraph, Row, Table, Wrap,
};
use ratatui::Frame;
use ratatui_image::StatefulImage;
use std::time::Instant;

/// The decorative underline motif under the search input.
const TOP_RULE: &str =
    "°𝄞·──────°,─────────｡“─────────────˚｡────────────˳◜───────────｡`──────────°,──────────,'─────────`,────────.`───────',──────⸝'─────`.────.ᐣ───ᐟ.──.ᐟ─~";

/// Decorative rule provided by the user, rendered flipped (reversed)
/// just above the keybind guide.
const BOTTOM_RULE: &str =
    "°𝄞·──────°,─────────｡“─────────────˚｡────────────˳◜───────────｡`──────────°,──────────,'─────────`,────────.`───────',──────⸝'─────`.────.ᐣ───ᐟ.──.ᐟ─~";

/// Reverse a string character by character for the flipped bottom rule.
fn flip_rule(s: &str) -> String {
    s.chars().rev().collect()
}

pub fn draw(frame: &mut Frame, app: &mut App, now: Instant) {
    let area = frame.area();
    // No background paint: the terminal's own surface (transparency,
    // blur, wallpaper) shows through everywhere.
    let layout = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // search prompt
        Constraint::Length(1), // decorative rule
        Constraint::Min(0),    // list + preview (feed bar menu on top row)
        Constraint::Length(1), // flipped decorative rule (above keybind guide)
        Constraint::Length(1), // status (keybind guide)
    ])
    .split(area);

    draw_header(frame, layout[0], app);
    draw_search(frame, layout[1], app);
    draw_rule(frame, layout[2], &app.theme);
    match app.view {
        View::Home => draw_home(frame, layout[3], app, now),
        View::Browse => draw_browse(frame, layout[3], app, now),
    }
    draw_bottom_rule(frame, layout[4], app);
    draw_status(frame, layout[5], app, now);

    if let Some(overlay) = &app.overlay {
        draw_overlay(frame, area, app, overlay);
    }
}

/// The header line above the search input (ytmtui style): the app name, and
/// — when the cookie jar carries a signed-in YouTube session — the account
/// name trailing it. Purely informational; no background is ever painted.
fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let mut spans = vec![Span::styled(
        " UTUI",
        Style::default()
            .fg(app.theme.brand)
            .add_modifier(Modifier::BOLD),
    )];
    if let Some(name) = &app.account_name {
        spans.push(Span::styled(" ⟢ ", Style::default().fg(app.theme.muted)));
        // " UTUI ⟢ " reserves eight columns; keep a small margin so the
        // name never sits flush against the terminal edge.
        let budget = usize::from(area.width).saturating_sub(9).max(1);
        spans.push(Span::styled(
            truncate(name, budget),
            Style::default().fg(app.theme.accent2),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
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
            spans.push(Span::styled(
                separator,
                Style::default().fg(app.theme.muted),
            ));
        }
        let selected = index == app.home_index;
        let key_style = if selected {
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(app.theme.muted)
        };
        let title_style = if selected {
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(app.theme.text)
        };
        // The bar shows the digit that jumps to each feed — w/l became
        // toggles, so the letters no longer all work as shortcuts. The
        // digit carries the dot ("1.") and the separator owns the padding,
        // so "·" sits in exactly two spaces on both sides.
        spans.push(Span::styled(format!("{}.", index + 1), key_style));
        spans.push(Span::styled(format!(" {}", item.title), title_style));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Left),
        area,
    );
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

    // A paged feed wears a one-row banner naming the window — which page is
    // open.
    // The play queue wears one naming the keys that edit and play it.
    let area = if app.feed_page > 1 {
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
        let banner = Line::from(vec![
            Span::styled(" page ", Style::default().fg(app.theme.muted)),
            Span::styled(
                app.feed_page.to_string(),
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        frame.render_widget(Paragraph::new(banner), rows[0]);
        rows[1]
    } else if app.feed.kind == FeedKind::Queue {
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
        let banner = Line::from(vec![
            Span::styled(" queue ", Style::default().fg(app.theme.muted)),
            Span::styled("J/K", Style::default().fg(app.theme.accent)),
            Span::styled(" move  ", Style::default().fg(app.theme.muted)),
            Span::styled("d", Style::default().fg(app.theme.accent)),
            Span::styled(" remove  ", Style::default().fg(app.theme.muted)),
            Span::styled("↵", Style::default().fg(app.theme.accent)),
            Span::styled(" play from here", Style::default().fg(app.theme.muted)),
        ]);
        frame.render_widget(Paragraph::new(banner), rows[0]);
        rows[1]
    } else {
        area
    };

    // Numbering runs continuously across pages — page 1 ends at 50, page 2
    // starts at 51 — so the number column widens on its own once a window
    // passes 99 or 999.
    let page_offset = app.feed_page.saturating_sub(1) * app.config.results;
    let number_width = (page_offset + app.feed.items.len())
        .to_string()
        .len()
        .max(2);

    let show_views = area.width >= 58;
    let mut widths = vec![
        Constraint::Length(number_width as u16),
        Constraint::Fill(1),
        Constraint::Length(7),
    ];
    if show_views {
        widths.push(Constraint::Length(7));
    }

    let title_width = area.width.saturating_sub(if show_views {
        number_width as u16 + 7 + 7 + 5
    } else {
        number_width as u16 + 7 + 3
    }) as usize;

    let rows: Vec<Row> = app
        .feed
        .items
        .iter()
        .enumerate()
        .map(|(index, video)| {
            let mut title_spans = vec![Span::styled(
                truncate(&video.title, title_width.max(8)),
                Style::default().fg(app.theme.brand),
            )];
            if video.is_live() {
                title_spans.push(Span::styled(" live", Style::default().fg(app.theme.error)));
            }
            if app.store.is_liked(&video.id) {
                title_spans.push(Span::styled(" ♥", Style::default().fg(app.theme.error)));
            }
            if app.store.is_watch_later(&video.id) {
                title_spans.push(Span::styled(" »", Style::default().fg(app.theme.warn)));
            }
            if app.store.is_saved(&video.id) {
                title_spans.push(Span::styled(" ·", Style::default().fg(app.theme.accent)));
            }

            let mut cells = vec![
                Cell::from(Span::styled(
                    format!("{:width$}", page_offset + index + 1, width = number_width),
                    Style::default().fg(Color::White),
                )),
                Cell::from(Line::from(title_spans)),
                Cell::from(Span::styled(
                    format_duration(video.duration),
                    Style::default().fg(app.theme.muted),
                )),
            ];
            if show_views {
                cells.push(Cell::from(Span::styled(
                    format_views(video.view_count),
                    Style::default().fg(app.theme.muted),
                )));
            }
            Row::new(cells)
        })
        .collect();

    let table = Table::new(rows, widths)
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
        Style::default().fg(app.theme.muted),
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
        let rows = thumbnail_rows(inner);
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
            Style::default()
                .fg(app.theme.error)
                .add_modifier(Modifier::BOLD),
        )));
    }
    if app.store.is_liked(&video.id) {
        lines.push(Line::from(Span::styled(
            "liked",
            Style::default()
                .fg(app.theme.error)
                .add_modifier(Modifier::BOLD),
        )));
    }
    if app.store.is_watch_later(&video.id) {
        lines.push(Line::from(Span::styled(
            "watch later",
            Style::default()
                .fg(app.theme.warn)
                .add_modifier(Modifier::BOLD),
        )));
    }
    if app.store.is_saved(&video.id) {
        lines.push(Line::from(Span::styled(
            "saved",
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));
    }
    if !lines.is_empty() {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        video.title.clone(),
        Style::default()
            .fg(app.theme.brand)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        video.channel.clone(),
        Style::default().fg(app.theme.muted),
    )));
    lines.push(Line::from(Span::styled(
        format!(
            "{} · {} views · {}",
            format_duration(video.duration),
            format_views(video.view_count),
            relative_time(video.timestamp)
        ),
        Style::default().fg(app.theme.muted),
    )));
    lines.push(Line::from(Span::styled(
        video
            .upload_date
            .clone()
            .unwrap_or_else(|| format_date(video.timestamp)),
        Style::default().fg(app.theme.muted),
    )));

    if let Some(description) = &video.description {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            truncate(description, 360),
            Style::default().fg(app.theme.muted),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "KEYBINDS",
        Style::default()
            .fg(app.theme.accent)
            .add_modifier(Modifier::BOLD),
    )));
    let keybinds = vec![
        ("enter", "play"),
        ("w", "watch later"),
        ("d", "download"),
        ("c", "channel"),
        ("r", "refresh"),
        ("esc", "back"),
        ("s", "save"),
        ("l", "like"),
        ("1-5", "feeds"),
        ("/", "search"),
        ("q", "quit"),
    ];
    for (key, label) in keybinds {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:>12} ", key),
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(label, Style::default().fg(app.theme.muted)),
        ]));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        body,
    );
}

/// Fixed number of rows reserved for a thumbnail — every thumbnail takes the
/// same vertical space regardless of aspect ratio.
fn thumbnail_rows(inner: Rect) -> u16 {
    let max_rows = inner.height.saturating_sub(6);
    if max_rows < 4 {
        return 0;
    }
    let fixed_rows = 17u16;
    fixed_rows.clamp(4, max_rows)
}

fn draw_status(frame: &mut Frame, area: Rect, app: &App, now: Instant) {
    // ytmtui footer layout: status message on the left, the keybind guide
    // pinned to the right with its keys picked out in the accent.
    // The feed bar on Home shows its digits by itself, so the 1-5 hint
    // only appears while there is no stacked feed to go back to.
    let hints: Vec<(&str, &str)> = vec![
        ("enter", "play"),
        ("s", "save"),
        ("l", "like"),
        ("1-5", "feeds"),
        ("/", "search"),
        ("q", "quit"),
    ];

    // Wide terminals keep the roomier two-space separator; narrow ones
    // thin the guide instead so the message never drops below a few
    // columns (the same budgeting ytmtui's footer does).
    let separator = if area.width >= 128 { "  " } else { " " };
    let (mut guide, mut guide_w) = guide_line(&hints, separator, &app.theme);
    let budget = usize::from(area.width).saturating_sub(guide_w + 1);
    if budget < 8 && hints.len() > 6 {
        let (thinned, thinned_w) = guide_line(&hints[..6], separator, &app.theme);
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
            Style::default().fg(app.theme.muted),
        ))
    } else {
        let (text, color) = if let Some(toast) = app.active_toast(now) {
            let prefix = match toast.kind {
                ToastKind::Info => "· ",
                ToastKind::Success => "✓ ",
                ToastKind::Error => "! ",
            };
            let color = match toast.kind {
                ToastKind::Info => app.theme.muted,
                ToastKind::Success => app.theme.success,
                ToastKind::Error => app.theme.error,
            };
            (format!("{prefix}{}", toast.text), color)
        } else if let Some(download) = &app.download {
            (
                format!("↓ {}%  {}", download.percent, truncate(&download.title, 34)),
                app.theme.warn,
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
                app.theme.muted,
            )
        } else {
            (
                format!(
                    "{} saved · {} history",
                    app.store.saved.len(),
                    app.store.history.len()
                ),
                app.theme.muted,
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
        Overlay::Quality { index } => draw_quality(frame, area, app, *index),
        Overlay::Help => draw_help(frame, area, app),
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
        Paragraph::new(Line::from(Span::styled(
            title,
            Style::default().fg(app.theme.muted),
        ))),
        sections[0],
    );

    let items: Vec<ListItem> = QUALITIES
        .iter()
        .map(|&(label, _)| {
            ListItem::new(Line::from(Span::styled(
                label,
                Style::default().fg(app.theme.text),
            )))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(index));
    let list = List::new(items).highlight_symbol("▌ ").highlight_style(
        Style::default()
            .fg(app.theme.accent)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_stateful_widget(list, sections[1], &mut state);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "enter download · esc",
            Style::default().fg(app.theme.muted),
        ))),
        sections[2],
    );
}

/// ytmtui-style decorative rule under the search input: one full copy of the
/// motif (or the row width, whichever is shorter) so ultra-wide terminals
/// keep its intended shape, painted in the same pywal color ytmtui uses for
/// its rule (color6 / accent2).
fn draw_rule(frame: &mut Frame, area: Rect, theme: &Theme) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let width = usize::from(area.width).min(TOP_RULE.chars().count());
    let rule: String = TOP_RULE.chars().take(width).collect();
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            rule,
            Style::default().fg(theme.accent2),
        ))),
        Rect::new(area.x, area.y, area.width, 1),
    );
}

/// The user-provided decorative rule, flipped (reversed) and rendered
/// just above the keybind guide.
fn draw_bottom_rule(frame: &mut Frame, area: Rect, app: &App) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let flipped = flip_rule(BOTTOM_RULE);
    let width = usize::from(area.width).min(flipped.chars().count());
    let width_u16 = width as u16;
    let start_x = area.x.saturating_add(area.width.saturating_sub(width_u16));
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            flipped.chars().take(width).collect::<String>(),
            Style::default().fg(app.theme.accent2),
        ))),
        Rect::new(start_x, area.y, width_u16, 1),
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
        Style::default()
            .fg(app.theme.accent)
            .add_modifier(Modifier::BOLD),
    )];
    let row = Rect::new(area.x, area.y, area.width, 1);
    match input {
        Some(input) => {
            spans.push(Span::styled(
                input.value.clone(),
                Style::default().fg(app.theme.text),
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
                Style::default().fg(app.theme.muted),
            ));
            frame.render_widget(Paragraph::new(Line::from(spans)), row);
        }
    }
}

fn draw_help(frame: &mut Frame, area: Rect, app: &App) {
    let width = area.width.saturating_sub(6).clamp(36, 86).min(area.width);
    let height = area.height.saturating_sub(2).min(34).min(area.height);
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);

    let inner = Block::default()
        .padding(Padding::new(2, 2, 0, 0))
        .inner(rect);

    let lines = vec![
        section("global", &app.theme),
        hint_line(&[("/", "search"), ("q", "quit")], &app.theme),
        Line::from(Span::styled(
            "search accepts queries, colon filters, and plain URLs",
            Style::default().fg(app.theme.muted),
        )),
        Line::from(""),
        section("feeds", &app.theme),
        hint_line(
            &[
                ("1", "my feeds"),
                ("2", "watch later"),
                ("3", "liked"),
                ("4", "saved"),
                ("5", "history"),
            ],
            &app.theme,
        ),
        Line::from(Span::styled(
            "letters F S H f still jump to my feeds · saved · history · subs",
            Style::default().fg(app.theme.muted),
        )),
        Line::from(""),
        section("list", &app.theme),
        hint_line(
            &[
                ("j/k/n/p", "move"),
                ("enter/space", "play"),
                ("s", "save/unsave"),
                ("r", "remove saved"),
                ("l", "like/unlike"),
                ("w", "watch later"),
                ("d", "pick quality"),
                ("c", "channel"),
                ("R", "refresh"),
                ("b/esc", "back"),
            ],
            &app.theme,
        ),
        Line::from(""),
        section("filters", &app.theme),
        Line::from(Span::styled(
            filter_names(0, 10),
            Style::default().fg(app.theme.muted),
        )),
        Line::from(Span::styled(
            filter_names(10, 21),
            Style::default().fg(app.theme.muted),
        )),
        Line::from(Span::styled(
            "recall  !1  !2  !3",
            Style::default().fg(app.theme.muted),
        )),
        Line::from(""),
        section("cookies", &app.theme),
        Line::from(Span::styled(
            "set cookies_browser in config.toml for subscriptions,",
            Style::default().fg(app.theme.muted),
        )),
        Line::from(Span::styled(
            "watch later, and liked videos",
            Style::default().fg(app.theme.muted),
        )),
        Line::from(""),
        section("account", &app.theme),
        Line::from(Span::styled(
            "l and w write real likes / Watch Later to your YouTube",
            Style::default().fg(app.theme.muted),
        )),
        Line::from(Span::styled(
            "account when cookies_browser is set (account_sync = true);",
            Style::default().fg(app.theme.muted),
        )),
        Line::from(Span::styled(
            "otherwise they toggle local mirrors only",
            Style::default().fg(app.theme.muted),
        )),
        Line::from(""),
        section("downloads", &app.theme),
        Line::from(Span::styled(
            "download_dir/video and download_dir/audio",
            Style::default().fg(app.theme.muted),
        )),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn hint_line(hints: &[(&str, &str)], theme: &Theme) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (key_value, label)) in hints.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("  ", Style::default().fg(theme.muted)));
        }
        spans.push(Span::styled(
            (*key_value).to_string(),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(theme.muted),
        ));
    }
    Line::from(spans)
}

/// The right-side key guide (ytmtui footer style): key picked out in the
/// accent bold, label muted, custom separator — returned with its rendered
/// width so the status layout can budget the message next to it.
fn guide_line(hints: &[(&str, &str)], separator: &str, theme: &Theme) -> (Line<'static>, usize) {
    let mut spans = Vec::new();
    let mut width = 0usize;
    for (index, (key_value, label)) in hints.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                separator.to_string(),
                Style::default().fg(theme.muted),
            ));
            width += separator.chars().count();
        }
        spans.push(Span::styled(
            (*key_value).to_string(),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
        width += key_value.chars().count();
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(theme.muted),
        ));
        width += 1 + label.chars().count();
    }
    (Line::from(spans), width)
}

fn section(label: &'static str, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        label,
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
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

    /// Every cell symbol on screen, in reading order.
    fn screen_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
            .map(|(x, y)| buffer[(x, y)].symbol())
            .collect()
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
    fn header_shows_signed_in_account() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app();
        app.account_name = Some("Qu!m Ashley Cast!ll0".into());
        terminal
            .draw(|frame| draw(frame, &mut app, Instant::now()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row: String = (0..buffer.area.width)
            .map(|x| buffer[(x, 0)].symbol())
            .collect();
        assert!(row.contains("UTUI"), "{row}");
        assert!(row.contains("Qu!m Ashley Cast!ll0"), "{row}");
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

    #[test]
    fn paged_feed_shows_page_banner() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app();
        app.view = View::Browse;
        app.feed = Feed {
            title: "Search · rust".into(),
            source: "https://example.com".into(),
            kind: FeedKind::Search,
            subtitle: "SEARCH  ·  50 RESULTS  ·  PAGE 2".into(),
            items: (0..5)
                .map(|i| Video {
                    id: format!("v{i}"),
                    title: format!("Video {i}"),
                    ..Video::default()
                })
                .collect(),
            notice: None,
            auth: false,
            fetched_at: 0,
        };
        app.feed_page = 2;
        app.table_state.select(Some(0));

        terminal
            .draw(|frame| draw(frame, &mut app, Instant::now()))
            .unwrap();
        let all = screen_text(&terminal);
        assert!(all.contains(" page 2"), "banner missing: {all}");

        // Page one carries no banner.
        app.feed_page = 1;
        terminal
            .draw(|frame| draw(frame, &mut app, Instant::now()))
            .unwrap();
        let all = screen_text(&terminal);
        assert!(!all.contains(" page "), "page 1 must not banner: {all}");
    }

    #[test]
    fn numbering_continues_across_pages() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app();
        app.view = View::Browse;
        app.feed = Feed {
            title: "Search · rust".into(),
            source: "https://example.com".into(),
            kind: FeedKind::Search,
            subtitle: "SEARCH  ·  3 RESULTS".into(),
            items: (0..3)
                .map(|i| Video {
                    id: format!("v{i}"),
                    title: format!("Video {i}"),
                    ..Video::default()
                })
                .collect(),
            notice: None,
            auth: false,
            fetched_at: 0,
        };
        app.table_state.select(Some(0));

        // Page 1 numbers 1..3 — nothing like 51 is on screen.
        terminal
            .draw(|frame| draw(frame, &mut app, Instant::now()))
            .unwrap();
        let all = screen_text(&terminal);
        assert!(!all.contains("51"), "page 1 must not show 51: {all}");

        // Page 2 of the same list continues at 51 (offset 50 + index).
        app.feed_page = 2;
        terminal
            .draw(|frame| draw(frame, &mut app, Instant::now()))
            .unwrap();
        let all = screen_text(&terminal);
        assert!(all.contains("51"), "page 2 must start at 51: {all}");
        assert!(all.contains("53"), "page 2 ends at 53: {all}");
    }

    #[test]
    fn queue_page_renders_queued_videos_and_keys() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app();
        app.view = View::Browse;
        app.store.queue = vec![
            Video {
                id: "a".into(),
                title: "First queued video".into(),
                ..Video::default()
            },
            Video {
                id: "b".into(),
                title: "Second queued video".into(),
                ..Video::default()
            },
        ];
        app.show_queue(false);
        app.table_state.select(Some(0));

        terminal
            .draw(|frame| draw(frame, &mut app, Instant::now()))
            .unwrap();
        let all = screen_text(&terminal);
        assert!(all.contains("First queued video"), "{all}");
        assert!(all.contains("Second queued video"), "{all}");
        // The banner names the keys that edit and play the queue.
        assert!(all.contains("J/K"), "queue banner: {all}");
        assert!(all.contains("play from here"), "queue banner: {all}");

        // The empty queue explains how to fill it instead of a blank list.
        app.store.queue.clear();
        app.show_queue(false);
        terminal
            .draw(|frame| draw(frame, &mut app, Instant::now()))
            .unwrap();
        let all = screen_text(&terminal);
        assert!(all.contains("Nothing queued"), "empty notice: {all}");
    }
}
