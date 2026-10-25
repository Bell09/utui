use crate::app::{Action, App, Overlay, TextInput, ToastKind, View, HOME_ITEMS};
use crate::theme::*;
use crate::util::{
    display_width, format_date, format_duration, format_views, relative_time, truncate,
};
use crate::yt::{self, Video};
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, Wrap,
};
use ratatui::Frame;
use std::time::Instant;

const LOGO: &[&str] = &[
    "██╗   ██╗████████╗██╗   ██╗██╗",
    "██║   ██║╚══██╔══╝██║   ██║██║",
    "██║   ██║   ██║   ██║   ██║██║",
    "██║   ██║   ██║   ██║   ██║██║",
    "╚██╗ ██╔╝   ██║   ╚██████╔╝██║",
    " ╚████╔╝    ██║    ╚═════╝ ╚═╝",
    "  ╚═══╝     ╚═╝",
];

pub fn draw(frame: &mut Frame, app: &mut App, now: Instant) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    let layout = Layout::vertical([
        Constraint::Length(3),
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
    draw_status(frame, layout[2], app);

    if let Some(overlay) = &app.overlay {
        draw_overlay(frame, area, app, overlay);
    }
    if let Some(toast) = app.active_toast(now) {
        draw_toast(frame, area, toast.text.as_str(), toast.kind);
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App, now: Instant) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(SURFACE));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::horizontal([Constraint::Fill(1), Constraint::Length(30)]).split(inner);

    let context = truncate(
        &app.current_title(),
        chunks[0].width.saturating_sub(12) as usize,
    );
    let mut left_spans = vec![
        Span::styled(
            " UTUI ",
            Style::default()
                .bg(PRIMARY)
                .fg(BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            context,
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ),
    ];
    if app.can_go_back() {
        left_spans.push(Span::styled("  ‹ b back", Style::default().fg(MUTED)));
    }
    let left = Line::from(left_spans);
    frame.render_widget(
        Paragraph::new(left).style(Style::default().bg(SURFACE)),
        chunks[0],
    );

    let right = if app.loading {
        Line::from(vec![
            Span::styled(app.spinner_frame(now), Style::default().fg(CYAN)),
            Span::raw(" "),
            Span::styled(truncate(&app.loading_label, 24), Style::default().fg(CYAN)),
        ])
    } else if app.view == View::Browse {
        Line::from(vec![Span::styled(
            format!("{} items  ", app.feed.items.len()),
            Style::default().fg(MUTED),
        )])
    } else {
        Line::from(vec![Span::styled("  ready  ", Style::default().fg(MUTED))])
    };
    frame.render_widget(
        Paragraph::new(right)
            .alignment(Alignment::Right)
            .style(Style::default().bg(SURFACE)),
        chunks[1],
    );
}

fn draw_home(frame: &mut Frame, area: Rect, app: &mut App) {
    let show_logo = area.height >= 32;
    let logo_height = if show_logo { 9 } else { 3 };

    let columns = if area.width >= 104 {
        Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).split(area)
    } else {
        Layout::horizontal([Constraint::Percentage(100)]).split(area)
    };
    let left = columns[0];

    let sections =
        Layout::vertical([Constraint::Length(logo_height), Constraint::Min(0)]).split(left);

    if show_logo {
        draw_logo(frame, sections[0], app);
    } else {
        let line = Line::from(vec![
            Span::styled(
                "YouTube at terminal velocity",
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("native Rust · yt-dlp · mpv", Style::default().fg(MUTED)),
        ]);
        frame.render_widget(
            Paragraph::new(line).alignment(Alignment::Center),
            sections[0],
        );
    }

    draw_home_menu(frame, sections[1], app);

    if columns.len() > 1 {
        let cards = Layout::vertical([Constraint::Percentage(55), Constraint::Min(0)])
            .split(columns[1].inner(Margin::new(2, 1)));
        draw_recent_searches(frame, cards[0], app);
        draw_status_card(frame, cards[1], app);
    }
}

fn draw_logo(frame: &mut Frame, area: Rect, app: &App) {
    let colors = [PRIMARY, PRIMARY, CYAN, CYAN, ACCENT, ACCENT, PRIMARY];
    let mut lines: Vec<Line> = LOGO
        .iter()
        .enumerate()
        .map(|(index, line)| {
            Line::from(Span::styled(
                *line,
                Style::default()
                    .fg(colors[index])
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            "YouTube at terminal velocity",
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        Span::styled(
            format!(
                "{} saved · {} watched · {} searches",
                app.store.saved.len(),
                app.store.history.len(),
                app.store.search_history.len()
            ),
            Style::default().fg(MUTED),
        ),
    ]));
    frame.render_widget(
        Paragraph::new(Text::from(lines)).alignment(Alignment::Center),
        area,
    );
}

fn draw_home_menu(frame: &mut Frame, area: Rect, app: &mut App) {
    if area.height == 0 {
        return;
    }
    let header_height = 1;
    let list_height = area.height.saturating_sub(header_height) as usize;
    let compact = list_height < HOME_ITEMS.len() * 2;

    let sections =
        Layout::vertical([Constraint::Length(header_height), Constraint::Min(0)]).split(area);
    let header = Line::from(vec![
        Span::styled(
            " BROWSE ",
            Style::default()
                .fg(BG)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            if compact {
                "j/k move · enter open"
            } else {
                "auth feeds use cookies_browser · press ? for help"
            },
            Style::default().fg(MUTED),
        ),
    ]);
    frame.render_widget(Paragraph::new(header), sections[0]);

    let items: Vec<ListItem> = HOME_ITEMS
        .iter()
        .map(|item| {
            let key = Span::styled(
                format!(" {} ", item.key),
                Style::default()
                    .bg(SURFACE_ALT)
                    .fg(CYAN)
                    .add_modifier(Modifier::BOLD),
            );
            let title = Span::styled(
                format!("  {:<16}", item.title),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            );
            let description = Span::styled(item.description, Style::default().fg(MUTED));
            if compact {
                ListItem::new(Line::from(vec![key, title, description]))
            } else {
                ListItem::new(vec![
                    Line::from(vec![key, title]),
                    Line::from(vec![
                        Span::raw("      "),
                        Span::styled(item.description, Style::default().fg(MUTED)),
                    ]),
                ])
            }
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.home_index));
    let list = List::new(items)
        .highlight_symbol("▌ ")
        .highlight_style(Style::default().bg(SURFACE_ALT).fg(TEXT));
    frame.render_stateful_widget(list, sections[1], &mut state);
}

fn draw_recent_searches(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" RECENT SEARCHES ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    if app.store.search_history.is_empty() {
        lines.push(Line::from(Span::styled(
            "No searches yet.",
            Style::default().fg(MUTED),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press / and try `synthwave :4k`.",
            Style::default().fg(DIM),
        )));
    } else {
        for (index, query) in app.store.search_history.iter().take(6).enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("{:>2}  ", index + 1), Style::default().fg(DIM)),
                Span::styled(
                    truncate(query, inner.width.saturating_sub(5) as usize),
                    Style::default().fg(TEXT),
                ),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("recall  ", Style::default().fg(MUTED)),
            Span::styled("!1", Style::default().fg(CYAN)),
            Span::styled("  !2  …", Style::default().fg(MUTED)),
        ]));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_status_card(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" STATUS ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cookies = if app.config.cookies_enabled() {
        app.config.cookies_browser.as_deref().unwrap_or("off")
    } else {
        "off"
    };
    let lines = vec![
        kv("player", &app.config.player),
        kv("cookies", cookies),
        kv("results", &app.config.results.to_string()),
        kv(
            "downloads",
            &truncate(&app.config.download_dir.display().to_string(), 32),
        ),
        Line::from(""),
        Line::from(vec![
            Span::styled(" tip  ", Style::default().fg(ACCENT)),
            Span::styled("`?` shows every keybinding", Style::default().fg(MUTED)),
        ]),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_browse(frame: &mut Frame, area: Rect, app: &mut App, now: Instant) {
    let sections = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(area);

    let title = truncate(
        &app.feed.title,
        sections[0].width.saturating_sub(14) as usize,
    );
    let kind = app.feed.kind.label();
    let line1 = Line::from(vec![
        Span::styled(
            format!(" {} ", kind),
            Style::default()
                .bg(SURFACE_ALT)
                .fg(CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            title,
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ),
    ]);
    let subtitle = if app.loading {
        format!("{} {}", app.spinner_frame(now), app.loading_label)
    } else if app.feed.subtitle.is_empty() {
        format!("{} items", app.feed.items.len())
    } else {
        app.feed.subtitle.clone()
    };
    let line2 = Line::from(vec![
        Span::raw("  "),
        Span::styled(
            truncate(&subtitle, sections[0].width.saturating_sub(4) as usize),
            Style::default().fg(MUTED),
        ),
    ]);
    frame.render_widget(Paragraph::new(Text::from(vec![line1, line2])), sections[0]);

    let body = sections[1];
    let selected = app.selected_video().cloned();

    if body.width >= 108 {
        let columns = Layout::horizontal([Constraint::Percentage(66), Constraint::Percentage(34)])
            .split(body);
        draw_table(frame, columns[0], app, now);
        if let Some(video) = selected {
            draw_details(frame, columns[1], app, &video);
        } else {
            draw_details_empty(frame, columns[1]);
        }
    } else {
        draw_table(frame, body, app, now);
    }
}

fn draw_table(frame: &mut Frame, area: Rect, app: &mut App, now: Instant) {
    let block = panel(" RESULTS ");
    let inner = block.inner(area);

    if app.feed.items.is_empty() {
        frame.render_widget(block, area);
        draw_empty_results(frame, inner, app, now);
        return;
    }

    let show_channel = inner.width >= 82;
    let mut widths = vec![Constraint::Length(3), Constraint::Fill(1)];
    if show_channel {
        widths.push(Constraint::Length(18));
    }
    widths.push(Constraint::Length(7));
    widths.push(Constraint::Length(8));

    let header = Row::new({
        let mut cells = vec![
            Cell::from(Span::styled("#", Style::default().fg(DIM))),
            Cell::from(Span::styled("TITLE", Style::default().fg(DIM))),
        ];
        if show_channel {
            cells.push(Cell::from(Span::styled(
                "CHANNEL",
                Style::default().fg(DIM),
            )));
        }
        cells.push(Cell::from(Span::styled("LEN", Style::default().fg(DIM))));
        cells.push(Cell::from(Span::styled("VIEWS", Style::default().fg(DIM))));
        cells
    })
    .style(Style::default().bg(SURFACE));

    let title_width = if show_channel {
        inner.width.saturating_sub(3 + 18 + 7 + 8 + 6) as usize
    } else {
        inner.width.saturating_sub(3 + 7 + 8 + 4) as usize
    };

    let rows: Vec<Row> = app
        .feed
        .items
        .iter()
        .enumerate()
        .map(|(index, video)| {
            let mut title_spans = vec![Span::styled(
                truncate(&video.title, title_width.max(8)),
                Style::default().fg(TEXT),
            )];
            if video.is_live() {
                title_spans.push(Span::raw(" "));
                title_spans.push(Span::styled(
                    "LIVE",
                    Style::default().fg(BG).bg(RED).add_modifier(Modifier::BOLD),
                ));
            }
            if app.store.is_saved(&video.id) {
                title_spans.push(Span::raw(" "));
                title_spans.push(Span::styled("◆", Style::default().fg(GREEN)));
            }

            let mut cells = vec![
                Cell::from(Span::styled(
                    format!("{:>2}", index + 1),
                    Style::default().fg(DIM),
                )),
                Cell::from(Line::from(title_spans)),
            ];
            if show_channel {
                cells.push(Cell::from(Span::styled(
                    truncate(&video.channel, 18),
                    Style::default().fg(MUTED),
                )));
            }
            cells.push(Cell::from(Span::styled(
                format_duration(video.duration),
                Style::default().fg(MUTED),
            )));
            cells.push(Cell::from(Span::styled(
                format_views(video.view_count),
                Style::default().fg(MUTED),
            )));
            Row::new(cells)
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .column_spacing(1)
        .highlight_symbol("▸ ")
        .row_highlight_style(
            Style::default()
                .bg(SURFACE_ALT)
                .fg(TEXT)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_empty_results(frame: &mut Frame, area: Rect, app: &App, now: Instant) {
    let (icon, headline, detail) = if app.loading {
        (
            app.spinner_frame(now),
            app.loading_label.clone(),
            "Fetching flat metadata with yt-dlp".to_string(),
        )
    } else if let Some(notice) = &app.feed.notice {
        ("!", "Nothing to show".to_string(), notice.clone())
    } else {
        (
            "·",
            "No results".to_string(),
            "Press / to search or u to open a URL".to_string(),
        )
    };

    let height = if detail.is_empty() { 4 } else { 6 };
    let chunks = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .split(area);
    let text = Text::from(vec![
        Line::from(Span::styled(
            icon,
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            headline,
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(detail, Style::default().fg(MUTED))),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        chunks[1],
    );
}

fn draw_details(frame: &mut Frame, area: Rect, app: &App, video: &Video) {
    let block = panel(" DETAILS ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut badges = Vec::new();
    if video.is_live() {
        badges.push(Span::styled(
            " LIVE ",
            Style::default().bg(RED).fg(BG).add_modifier(Modifier::BOLD),
        ));
        badges.push(Span::raw(" "));
    }
    if app.store.is_saved(&video.id) {
        badges.push(Span::styled(
            " ◆ SAVED ",
            Style::default()
                .bg(GREEN)
                .fg(BG)
                .add_modifier(Modifier::BOLD),
        ));
        badges.push(Span::raw(" "));
    }
    badges.push(Span::styled(
        format!(" {} ", video.id),
        Style::default().bg(SURFACE_ALT).fg(MUTED),
    ));

    let mut lines = vec![
        Line::from(badges),
        Line::from(""),
        Line::from(Span::styled(
            video.title.clone(),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            video.channel.clone(),
            Style::default().fg(PRIMARY),
        )),
        Line::from(vec![
            Span::styled(format_duration(video.duration), Style::default().fg(CYAN)),
            Span::styled("  ·  ", Style::default().fg(DIM)),
            Span::styled(
                format!("{} views", format_views(video.view_count)),
                Style::default().fg(MUTED),
            ),
            Span::styled("  ·  ", Style::default().fg(DIM)),
            Span::styled(relative_time(video.timestamp), Style::default().fg(MUTED)),
        ]),
        Line::from(vec![
            Span::styled("uploaded ", Style::default().fg(DIM)),
            Span::styled(
                video
                    .upload_date
                    .clone()
                    .unwrap_or_else(|| format_date(video.timestamp)),
                Style::default().fg(MUTED),
            ),
        ]),
        Line::from(Span::styled(
            "─".repeat(inner.width.saturating_sub(2) as usize),
            Style::default().fg(BORDER),
        )),
    ];

    if let Some(description) = &video.description {
        lines.push(Line::from(Span::styled(
            truncate(description, 320),
            Style::default().fg(MUTED),
        )));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        "URL",
        Style::default().fg(DIM).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        video.url.clone(),
        Style::default().fg(CYAN),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "ACTIONS",
        Style::default().fg(DIM).add_modifier(Modifier::BOLD),
    )));
    lines.extend([
        action_line("enter", "all actions"),
        action_line("p", "play video"),
        action_line("a", "play audio"),
        action_line("d", "download video"),
        action_line("D", "download audio"),
        action_line(
            "s",
            if app.store.is_saved(&video.id) {
                "remove from saved"
            } else {
                "save video"
            },
        ),
        action_line("c", "open channel"),
        action_line("y", "copy URL"),
        action_line("o", "open browser"),
    ]);

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().padding(ratatui::widgets::Padding::new(1, 1, 0, 0)))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_details_empty(frame: &mut Frame, area: Rect) {
    let block = panel(" DETAILS ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(""),
            Line::from(Span::styled(
                "Select a result",
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Details and actions appear here.",
                Style::default().fg(MUTED),
            )),
        ]))
        .alignment(Alignment::Center),
        inner,
    );
}

fn action_line(key: &'static str, label: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {key} "),
            Style::default()
                .bg(SURFACE_ALT)
                .fg(CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(label, Style::default().fg(MUTED)),
    ])
}

fn kv(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<10}"), Style::default().fg(DIM)),
        Span::styled(value.to_string(), Style::default().fg(TEXT)),
    ])
}

fn draw_settings(frame: &mut Frame, area: Rect, app: &App) {
    let columns =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(area);
    let left = Layout::vertical([Constraint::Percentage(55), Constraint::Min(0)])
        .split(columns[0].inner(Margin::new(2, 1)));
    let right = Layout::vertical([Constraint::Percentage(55), Constraint::Min(0)])
        .split(columns[1].inner(Margin::new(2, 1)));

    let config_block = panel(" CONFIG ");
    let config_inner = config_block.inner(left[0]);
    frame.render_widget(config_block, left[0]);
    let config_lines = vec![
        kv("config", &app.paths.config_file().display().to_string()),
        kv("data", &app.paths.state_file().display().to_string()),
        kv("cache", &app.paths.feed_cache_dir().display().to_string()),
        kv("player", &app.config.player),
        kv(
            "cookies",
            app.config.cookies_browser.as_deref().unwrap_or("off"),
        ),
        kv("results", &app.config.results.to_string()),
        kv("downloads", &app.config.download_dir.display().to_string()),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Edit with ", Style::default().fg(MUTED)),
            Span::styled("e", Style::default().fg(CYAN)),
            Span::styled(" · reload with ", Style::default().fg(MUTED)),
            Span::styled("r", Style::default().fg(CYAN)),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(config_lines)).wrap(Wrap { trim: true }),
        config_inner,
    );

    let player_block = panel(" PLAYER ");
    let player_inner = player_block.inner(left[1]);
    frame.render_widget(player_block, left[1]);
    let player_lines = vec![
        Line::from(Span::styled("video arguments", Style::default().fg(DIM))),
        Line::from(Span::styled(
            app.config.video_args.join(" "),
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled("audio arguments", Style::default().fg(DIM))),
        Line::from(Span::styled(
            app.config.audio_args.join(" "),
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Playback hands the video URL directly to mpv.",
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(player_lines)).wrap(Wrap { trim: true }),
        player_inner,
    );

    let about_block = panel(" ABOUT ");
    let about_inner = about_block.inner(right[0]);
    frame.render_widget(about_block, right[0]);
    let about_lines = vec![
        Line::from(vec![
            Span::styled(
                "UTUI",
                Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  v{}", env!("CARGO_PKG_VERSION")),
                Style::default().fg(MUTED),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "A native Rust terminal browser for YouTube and anything yt-dlp supports.",
            Style::default().fg(TEXT),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Inspired by the yt-x workflow. Independent implementation, no shell pipeline, no fzf or jq.",
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Feeds are cached for 10 minutes. Press r to force a refresh.",
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(about_lines)).wrap(Wrap { trim: true }),
        about_inner,
    );

    let shortcuts_block = panel(" SHORTCUTS ");
    let shortcuts_inner = shortcuts_block.inner(right[1]);
    frame.render_widget(shortcuts_block, right[1]);
    let shortcut_lines = [
        action_line("e", "edit config.toml"),
        action_line("o", "open download folder"),
        action_line("r", "reload config"),
        action_line("esc", "back home"),
        action_line("?", "all keybindings"),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(shortcut_lines.to_vec())),
        shortcuts_inner,
    );
}

fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::horizontal([Constraint::Fill(1), Constraint::Length(48)]).split(area);
    frame.render_widget(Block::default().style(Style::default().bg(SURFACE)), area);

    let hints: &[(&str, &str)] = match app.view {
        View::Home => &[
            ("↑↓", "move"),
            ("enter", "open"),
            ("/", "search"),
            ("u", "url"),
            ("?", "help"),
            ("q", "quit"),
        ],
        View::Browse => &[
            ("enter", "actions"),
            ("p", "play"),
            ("a", "audio"),
            ("d", "download"),
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

    let status = app.status_line();
    let color = if app.download.is_some() {
        YELLOW
    } else {
        MUTED
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate(&status, 47),
            Style::default().fg(color),
        )))
        .alignment(Alignment::Right),
        chunks[1],
    );
}

fn hint_line(hints: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (key, label)) in hints.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("  ·  ", Style::default().fg(DIM)));
        }
        spans.push(Span::styled(
            (*key).to_string(),
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            (*label).to_string(),
            Style::default().fg(MUTED),
        ));
    }
    Line::from(spans)
}

fn draw_overlay(frame: &mut Frame, area: Rect, app: &App, overlay: &Overlay) {
    match overlay {
        Overlay::Search(input) => draw_search_overlay(frame, area, input),
        Overlay::Url(input) => draw_url_overlay(frame, area, input),
        Overlay::Actions { index, actions } => {
            draw_actions_overlay(frame, area, app, *index, actions)
        }
        Overlay::Help => draw_help(frame, area),
    }
}

fn draw_search_overlay(frame: &mut Frame, area: Rect, input: &TextInput) {
    let width = area.width.saturating_sub(8).clamp(32, 92).min(area.width);
    let height = 7.min(area.height);
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);

    let block = overlay_panel(" SEARCH YOUTUBE ");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let prompt = Line::from(vec![
        Span::styled("/ ", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        Span::styled(input.value.clone(), Style::default().fg(TEXT)),
    ]);
    frame.render_widget(Paragraph::new(prompt), inner);

    let plan = yt::parse_search(&input.value);
    let filter_line = if let Some(filter) = plan.filter {
        Line::from(vec![
            Span::styled("filter  ", Style::default().fg(DIM)),
            Span::styled(format!(":{filter}"), Style::default().fg(ACCENT)),
        ])
    } else {
        Line::from(Span::styled(
            "filters  :today  :week  :4k  :hd  :live  :short  :newest  :views",
            Style::default().fg(DIM),
        ))
    };
    if inner.height > 2 {
        frame.render_widget(
            Paragraph::new(filter_line),
            Rect::new(inner.x, inner.y + 2, inner.width, 1),
        );
    }
    if inner.height > 4 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "enter search  ·  esc cancel  ·  !1 recalls the latest search",
                Style::default().fg(MUTED),
            ))),
            Rect::new(inner.x, inner.y + 4, inner.width, 1),
        );
    }

    let cursor_x = inner
        .x
        .saturating_add(2)
        .saturating_add(input.cursor_width() as u16)
        .min(rect.right().saturating_sub(2));
    frame.set_cursor_position((cursor_x, inner.y));
}

fn draw_url_overlay(frame: &mut Frame, area: Rect, input: &TextInput) {
    let width = area.width.saturating_sub(8).clamp(32, 92).min(area.width);
    let height = 6.min(area.height);
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);

    let block = overlay_panel(" OPEN URL OR PLAYLIST ");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let prompt = Line::from(vec![
        Span::styled("↗ ", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        Span::styled(input.value.clone(), Style::default().fg(TEXT)),
    ]);
    frame.render_widget(Paragraph::new(prompt), inner);

    if inner.height > 2 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "YouTube video, playlist, channel, or any yt-dlp supported URL",
                Style::default().fg(MUTED),
            ))),
            Rect::new(inner.x, inner.y + 2, inner.width, 1),
        );
    }
    if inner.height > 3 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "enter open  ·  esc cancel",
                Style::default().fg(MUTED),
            ))),
            Rect::new(inner.x, inner.y + 3, inner.width, 1),
        );
    }

    let cursor_x = inner
        .x
        .saturating_add(2)
        .saturating_add(input.cursor_width() as u16)
        .min(rect.right().saturating_sub(2));
    frame.set_cursor_position((cursor_x, inner.y));
}

fn draw_actions_overlay(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    index: usize,
    actions: &[Action],
) {
    let title = app
        .selected_video()
        .map(|video| truncate(&video.title, 40))
        .unwrap_or_else(|| "Actions".to_string());
    let width = area.width.saturating_sub(8).clamp(32, 64).min(area.width);
    let height = (actions.len() as u16 + 4).min(area.height.saturating_sub(2));
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);

    let block = overlay_panel(&format!(" ACTIONS · {title} "));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let items: Vec<ListItem> = actions
        .iter()
        .map(|action| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", action.key),
                    Style::default()
                        .bg(SURFACE_ALT)
                        .fg(CYAN)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(action.label, Style::default().fg(TEXT)),
            ]))
        })
        .collect();
    let list_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1),
    );
    let mut state = ListState::default();
    state.select(Some(index));
    let list = List::new(items)
        .highlight_symbol("▸ ")
        .highlight_style(Style::default().bg(SURFACE_ALT).fg(PRIMARY));
    frame.render_stateful_widget(list, list_area, &mut state);

    if inner.height > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "enter run  ·  esc close",
                Style::default().fg(MUTED),
            ))),
            Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
        );
    }
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let width = area.width.saturating_sub(6).clamp(36, 86).min(area.width);
    let height = area.height.saturating_sub(4).clamp(10, 34).min(area.height);
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);

    let block = overlay_panel(" KEYBINDINGS ");
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let lines = vec![
        section("GLOBAL"),
        hint_line(&[("/", "search"), ("u", "url"), ("?", "help"), ("q", "quit")]),
        Line::from(""),
        section("BROWSE"),
        hint_line(&[
            ("j/k", "move"),
            ("g/G", "first/last"),
            ("enter", "actions"),
            ("p", "play"),
            ("a", "audio"),
            ("d/D", "download"),
            ("s", "save"),
            ("c", "channel"),
            ("y", "copy"),
            ("o", "browser"),
            ("r", "refresh"),
            ("b", "back"),
        ]),
        Line::from(""),
        section("SEARCH"),
        Line::from(Span::styled(
            "Use one colon filter at the start or end of a query.",
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(filter_names(0, 10), Style::default().fg(CYAN))),
        Line::from(Span::styled(
            filter_names(10, 21),
            Style::default().fg(CYAN),
        )),
        Line::from(Span::styled(
            "recall with !1, !2, …",
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        section("COOKIES & CONFIG"),
        Line::from(Span::styled(
            "Set cookies_browser = \"firefox\" in config.toml for subscriptions,",
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            "Watch Later, and Liked videos. Then press r in Settings.",
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        section("DOWNLOADS"),
        Line::from(Span::styled(
            "Video downloads go to download_dir/video, audio to download_dir/audio.",
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::default().padding(ratatui::widgets::Padding::new(2, 2, 1, 1)))
            .wrap(Wrap { trim: false }),
        inner,
    );
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

fn section(label: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        label,
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ))
}

fn draw_toast(frame: &mut Frame, area: Rect, message: &str, kind: ToastKind) {
    if area.height < 5 || area.width < 24 {
        return;
    }
    let width = (display_width(message) as u16 + 6)
        .min(area.width.saturating_sub(4))
        .max(20);
    let rect = Rect::new(
        area.right().saturating_sub(width + 2),
        area.height.saturating_sub(4),
        width,
        3,
    );
    let (border, icon) = match kind {
        ToastKind::Info => (CYAN, "·"),
        ToastKind::Success => (GREEN, "✓"),
        ToastKind::Error => (RED, "!"),
    };
    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(SURFACE));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let text = format!(
        "{icon} {}",
        truncate(message, width.saturating_sub(5) as usize)
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            text,
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ))),
        inner,
    );
}

fn panel(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(Span::styled(
            title.to_string(),
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )))
        .style(Style::default().bg(BG))
}

fn overlay_panel(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY))
        .title(Line::from(Span::styled(
            title.to_string(),
            Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD),
        )))
        .style(Style::default().bg(SURFACE))
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
    use crate::app::Feed;
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
            kind: crate::app::FeedKind::Search,
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
}
