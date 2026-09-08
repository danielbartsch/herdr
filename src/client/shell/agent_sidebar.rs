use std::collections::HashMap;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Paragraph, Widget},
};

use super::*;

pub(super) struct AgentRow {
    pub(super) pane_id: String,
    pub(super) status: crate::api::schema::AgentStatus,
    pub(super) focused: bool,
    pub(super) rows: Vec<Vec<crate::ui::ResolvedToken>>,
}

pub(super) fn ordered_agent_pane_ids(
    snapshot: &ClientShellSnapshot,
    sort: crate::config::AgentPanelSortConfig,
) -> Vec<String> {
    if snapshot.agent_view_label.is_some() {
        return snapshot
            .agent_order
            .iter()
            .filter(|pane_id| {
                snapshot
                    .agents
                    .iter()
                    .any(|agent| agent.pane_id == pane_id.as_str())
            })
            .cloned()
            .collect();
    }
    let mut agents = snapshot.agents.iter().collect::<Vec<_>>();
    if sort == crate::config::AgentPanelSortConfig::Priority {
        agents.sort_by_key(|agent| {
            (
                std::cmp::Reverse(status_priority(agent.agent_status)),
                std::cmp::Reverse(agent.state_change_seq),
            )
        });
    }
    agents
        .into_iter()
        .map(|agent| agent.pane_id.clone())
        .collect()
}

pub(super) fn render_agent_panel(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
    hover: Option<(u16, u16)>,
) {
    if !render_agent_panel_header(
        buffer,
        area,
        snapshot.agent_view_label.as_deref(),
        config,
        hits,
    ) {
        return;
    }

    let rows = agent_rows(snapshot, config, None);
    render_agent_list(
        buffer,
        area,
        &rows,
        snapshot
            .agent_view_label
            .as_ref()
            .map(|_| " no matching agents"),
        config,
        agent_scroll,
        hits,
        |row| row.rows.len(),
        |buffer, rect, row, hits| {
            hits.agents.push((rect, row.pane_id.clone()));
            render_agent_row(buffer, rect, row, config, hover, &mut hits.hover_tooltips);
        },
    );
}

pub(super) fn render_agent_panel_header(
    buffer: &mut Buffer,
    area: Rect,
    agent_view_label: Option<&str>,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) -> bool {
    if area.height == 0 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        &"─".repeat(area.width as usize),
        Style::default().fg(config.palette.surface_dim),
    );
    if area.height < 2 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        " agents",
        Style::default()
            .fg(config.palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    let sort_label = agent_view_label.unwrap_or(match config.agent_panel_sort {
        crate::config::AgentPanelSortConfig::Spaces => "grouped",
        crate::config::AgentPanelSortConfig::Priority => "priority",
    });
    let sort_width = display_width(sort_label).min(area.width as usize) as u16;
    let sort_rect = Rect::new(
        area.right().saturating_sub(sort_width),
        area.y + 1,
        sort_width,
        1,
    );
    hits.agent_sort_toggle = if config.mouse_capture && agent_view_label.is_none() {
        sort_rect
    } else {
        Rect::default()
    };
    put_text(
        buffer,
        sort_rect.x,
        sort_rect.y,
        sort_rect.width,
        sort_label,
        Style::default()
            .fg(if agent_view_label.is_some() {
                config.palette.accent
            } else {
                config.palette.overlay0
            })
            .add_modifier(Modifier::BOLD),
    );
    true
}

/// Header rows above the agent list (matches the `+3` body offset below).
const AGENT_PANEL_HEADER_ROWS: u16 = 3;
/// Blank spacer row between the agent list and the status-icon legend.
const AGENT_LEGEND_SPACER_ROWS: u16 = 1;
/// Two spaces separate legend chips packed onto the same line.
const AGENT_LEGEND_CHIP_GAP: u16 = 2;

/// The status/label pairs shown in the agents-pane legend, in display order.
/// One row per meaningfully distinct glyph/color. "none" spells out `Unknown`
/// (no detected agent / plain shell) so every glyph stays distinct.
const STATUS_LEGEND: [(crate::api::schema::AgentStatus, &str); 5] = [
    (crate::api::schema::AgentStatus::Working, "working"),
    (crate::api::schema::AgentStatus::Blocked, "blocked"),
    (crate::api::schema::AgentStatus::Done, "done"),
    (crate::api::schema::AgentStatus::Idle, "idle"),
    (crate::api::schema::AgentStatus::Unknown, "none"),
];

/// Columns one legend chip needs: glyph (1) + space (1) + label.
fn agent_legend_chip_width(label: &str) -> u16 {
    2u16.saturating_add(label.chars().count() as u16)
}

/// Greedily packs the legend labels into lines that each fit `width`, returning
/// the label index for every chip grouped by line. Kept in step with
/// [`agent_legend_line_count`], which computes the same geometry without
/// allocating.
fn agent_legend_lines(width: u16) -> Vec<Vec<usize>> {
    let mut lines: Vec<Vec<usize>> = Vec::new();
    if width == 0 {
        return lines;
    }
    let mut current: Vec<usize> = Vec::new();
    let mut used = 0u16;
    for (idx, (_, label)) in STATUS_LEGEND.iter().enumerate() {
        let chip = agent_legend_chip_width(label);
        let with_gap = AGENT_LEGEND_CHIP_GAP.saturating_add(chip);
        if current.is_empty() {
            current.push(idx);
            used = chip;
        } else if used.saturating_add(with_gap) > width {
            lines.push(std::mem::take(&mut current));
            current.push(idx);
            used = chip;
        } else {
            current.push(idx);
            used = used.saturating_add(with_gap);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Number of legend lines at `width`, without allocating. Mirrors the packing
/// in [`agent_legend_lines`].
fn agent_legend_line_count(width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    let mut lines = 0u16;
    let mut used = 0u16;
    let mut line_started = false;
    for (_, label) in STATUS_LEGEND {
        let chip = agent_legend_chip_width(label);
        let with_gap = AGENT_LEGEND_CHIP_GAP.saturating_add(chip);
        if !line_started {
            lines += 1;
            used = chip;
            line_started = true;
        } else if used.saturating_add(with_gap) > width {
            lines += 1;
            used = chip;
        } else {
            used = used.saturating_add(with_gap);
        }
    }
    lines
}

/// Rows the legend reserves at the bottom of the agent pane, including the blank
/// spacer above it. Returns `0` when the pane is too short to keep at least one
/// list row after reserving, so small panes render exactly as before.
fn agent_legend_reserved_rows(area: Rect) -> u16 {
    let content_rows = agent_legend_line_count(area.width);
    if content_rows == 0 {
        return 0;
    }
    let reserved = content_rows.saturating_add(AGENT_LEGEND_SPACER_ROWS);
    let list_capacity = area.height.saturating_sub(AGENT_PANEL_HEADER_ROWS);
    if list_capacity > reserved {
        reserved
    } else {
        0
    }
}

/// Rect the legend chips render into (the spacer row is excluded), or the empty
/// rect when no legend is shown.
fn agent_legend_rect(area: Rect) -> Rect {
    let reserved = agent_legend_reserved_rows(area);
    if reserved == 0 {
        return Rect::default();
    }
    let content_rows = reserved.saturating_sub(AGENT_LEGEND_SPACER_ROWS);
    let y = (area.y + area.height).saturating_sub(content_rows);
    Rect::new(area.x, y, area.width, content_rows)
}

/// Draws the status-icon legend across the reserved rows at the bottom of the
/// agent pane. Each chip pairs the status glyph (in its state color) with a
/// short label, so the color that distinguishes otherwise-identical dots is
/// explained in place.
fn render_agent_legend(buffer: &mut Buffer, area: Rect, config: &ClientShellConfig) {
    let legend_rect = agent_legend_rect(area);
    if legend_rect.width == 0 || legend_rect.height == 0 {
        return;
    }
    let palette = &config.palette;
    let label_style = Style::default()
        .fg(palette.overlay0)
        .add_modifier(Modifier::DIM);
    for (row, chips) in agent_legend_lines(area.width).iter().enumerate() {
        if row as u16 >= legend_rect.height {
            break;
        }
        let mut spans: Vec<ratatui::text::Span> = Vec::new();
        for (position, &idx) in chips.iter().enumerate() {
            if position > 0 {
                spans.push(ratatui::text::Span::raw("  "));
            }
            let (status, label) = STATUS_LEGEND[idx];
            spans.push(ratatui::text::Span::styled(
                status_icon(status, config.status_indicators),
                Style::default().fg(status_color(status, palette)),
            ));
            spans.push(ratatui::text::Span::raw(" "));
            spans.push(ratatui::text::Span::styled(label, label_style));
        }
        Paragraph::new(Line::from(spans)).render(
            Rect::new(
                legend_rect.x,
                legend_rect.y + row as u16,
                legend_rect.width,
                1,
            ),
            buffer,
        );
    }
}

pub(super) fn render_agent_list<T>(
    buffer: &mut Buffer,
    area: Rect,
    rows: &[T],
    empty_message: Option<&str>,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
    row_lines: impl Fn(&T) -> usize,
    mut render_row: impl FnMut(&mut Buffer, Rect, &T, &mut ShellHitMap),
) {
    // Reserve space for the status legend at the bottom, shrinking the list body
    // so rows never overlap it. `agent_legend_reserved_rows` returns 0 when the
    // pane is too short, leaving the list exactly as it was.
    let legend_reserved = agent_legend_reserved_rows(area);
    render_agent_legend(buffer, area, config);
    let body = Rect::new(
        area.x,
        area.y.saturating_add(AGENT_PANEL_HEADER_ROWS),
        area.width,
        area.height
            .saturating_sub(AGENT_PANEL_HEADER_ROWS)
            .saturating_sub(legend_reserved),
    );
    hits.agent_body = body;
    if body.is_empty() || rows.is_empty() {
        *agent_scroll = 0;
        if let Some(message) = empty_message.filter(|_| !body.is_empty()) {
            put_text(
                buffer,
                body.x,
                body.y,
                body.width,
                message,
                Style::default()
                    .fg(config.palette.overlay0)
                    .add_modifier(Modifier::DIM),
            );
        }
        return;
    }

    let row_heights = rows
        .iter()
        .map(|row| row_lines(row).max(1).min(u16::MAX as usize) as u16)
        .collect::<Vec<_>>();
    let gaps = rows
        .iter()
        .enumerate()
        .map(|(index, _)| {
            if index + 1 < rows.len() {
                config.agents.row_gap
            } else {
                0
            }
        })
        .collect::<Vec<_>>();
    let metrics =
        super::scroll::list_scroll_metrics(&row_heights, &gaps, body.height, *agent_scroll);
    hits.agent_max_scroll = metrics.max_offset_from_bottom;
    hits.agent_scroll_metrics = Some(metrics);
    *agent_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = body.y;
    for (index, row) in rows.iter().enumerate().skip(*agent_scroll) {
        let height = row_heights[index].min(body.height);
        if y.saturating_add(height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, height);
        render_row(buffer, rect, row, hits);
        y = y
            .saturating_add(height)
            .saturating_add(if index + 1 < rows.len() {
                config.agents.row_gap
            } else {
                0
            });
    }

    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.agent_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, &config.palette);
    }
}

pub(super) fn agent_rows(
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    machine: Option<&str>,
) -> Vec<AgentRow> {
    ordered_agent_pane_ids(snapshot, config.agent_panel_sort)
        .into_iter()
        .filter_map(|pane_id| {
            let agent = snapshot
                .agents
                .iter()
                .find(|agent| agent.pane_id == pane_id)?;
            let workspace = snapshot
                .workspaces
                .iter()
                .find(|workspace| workspace.workspace_id == agent.workspace_id)?;
            let tab = snapshot.tabs.iter().find(|tab| tab.tab_id == agent.tab_id);
            let pane = snapshot
                .panes
                .iter()
                .find(|pane| pane.pane_id == agent.pane_id);
            let tab_count = snapshot
                .tabs
                .iter()
                .filter(|candidate| candidate.workspace_id == agent.workspace_id)
                .count();
            let tab_label = tab
                .filter(|tab| tab_count > 1 || tab.custom_label)
                .map(|tab| tab.label.as_str());
            let agent_label = agent
                .display_agent
                .as_deref()
                .or(agent.name.as_deref())
                .or(agent.agent.as_deref())
                .or(agent.title.as_deref());
            let labels = agent
                .state_labels
                .iter()
                .cloned()
                .collect::<HashMap<_, _>>();
            let tokens = agent.tokens.iter().cloned().collect::<HashMap<_, _>>();
            let state_text = labels
                .get(status_text(agent.agent_status))
                .map(String::as_str)
                .unwrap_or_else(|| sidebar_status_text(agent.agent_status));
            let canonical_agent = agent
                .agent
                .as_deref()
                .and_then(crate::detect::parse_agent_label);
            let rows = crate::ui::sidebar_agent_rows(
                &config.agents,
                crate::ui::AgentTokenContext {
                    machine,
                    workspace: &workspace.label,
                    tab: tab_label,
                    pane: agent
                        .title
                        .as_deref()
                        .or_else(|| pane.and_then(|pane| pane.label.as_deref())),
                    agent_label,
                    terminal_title: agent.terminal_title.as_deref(),
                    terminal_title_stripped: agent.terminal_title_stripped.as_deref(),
                    canonical_agent,
                    tokens: &tokens,
                },
                state_text,
            );
            Some(AgentRow {
                pane_id: agent.pane_id.clone(),
                status: agent.agent_status,
                focused: agent.focused,
                rows,
            })
        })
        .collect()
}

pub(super) fn render_agent_row(
    buffer: &mut Buffer,
    rect: Rect,
    row: &AgentRow,
    config: &ClientShellConfig,
    hover: Option<(u16, u16)>,
    tooltips: &mut Vec<super::state::HoverTooltip>,
) {
    let palette = &config.palette;
    // The row background, reused for the row fill and every tooltip box so
    // hovering preserves it, matching the spaces section's behavior.
    let row_bg = if row.focused {
        palette.active_row_bg
    } else {
        palette.sidebar_bg
    };
    let row_style = Style::default().bg(row_bg);
    let name_style = if row.focused {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(palette.subtext0)
            .add_modifier(Modifier::BOLD)
    };
    let status_style = Style::default()
        .fg(status_color(row.status, palette))
        .add_modifier(if row.focused {
            Modifier::empty()
        } else {
            Modifier::DIM
        });
    // Match the spaces pane's secondary-line style (mauve when focused, else
    // overlay0, no DIM) so the agents pane reads the same in dark terminals.
    let secondary = Style::default().fg(if row.focused {
        palette.mauve
    } else {
        palette.overlay0
    });
    let icon = (
        status_icon(row.status, config.status_indicators),
        Style::default().fg(status_color(row.status, palette)),
    );
    let rows = if row.rows.is_empty() {
        vec![vec![crate::ui::ResolvedToken {
            kind: crate::ui::ResolvedTokenKind::StateIcon,
            style: Default::default(),
        }]]
    } else {
        row.rows.clone()
    };
    let over_entry = hover.is_some_and(|point| super::contains(rect, point));
    for (index, tokens) in rows.iter().take(rect.height as usize).enumerate() {
        let indent = if index == 0 { 1 } else { 3 };
        let max_width = rect.width.saturating_sub(indent as u16) as usize;
        let content = crate::ui::resolved_token_spans(
            tokens,
            icon,
            status_style,
            name_style,
            secondary,
            secondary,
            palette,
            max_width,
        );
        if over_entry {
            let full = crate::ui::resolved_token_spans(
                tokens,
                icon,
                status_style,
                name_style,
                secondary,
                secondary,
                palette,
                usize::MAX,
            );
            if spans_text(&full) != spans_text(&content) {
                tooltips.push(super::state::HoverTooltip {
                    spans: hover_tooltip_spans(&full),
                    bg: row_bg,
                    row: rect.y + index as u16,
                    col: rect.x + indent as u16,
                });
            }
        }
        let mut spans = vec![ratatui::text::Span::raw(" ".repeat(indent))];
        spans.extend(content);
        Paragraph::new(Line::from(spans)).style(row_style).render(
            Rect::new(rect.x, rect.y + index as u16, rect.width, 1),
            buffer,
        );
    }
}

fn put_text(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    for (offset, character) in text.chars().take(width as usize).enumerate() {
        if let Some(cell) = buffer.cell_mut((x + offset as u16, y)) {
            cell.set_char(character).set_style(style);
        }
    }
}

fn display_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

fn sidebar_status_text(status: crate::api::schema::AgentStatus) -> &'static str {
    use crate::api::schema::AgentStatus;
    match status {
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Working => "working",
        AgentStatus::Idle | AgentStatus::Unknown => "idle",
    }
}

#[cfg(test)]
mod legend_tests {
    use super::*;

    #[test]
    fn line_count_matches_packing() {
        // The no-alloc line counter must agree with the actual packing at every
        // width, since geometry (body reservation) relies on the former while
        // rendering uses the latter.
        for width in 0..=120u16 {
            assert_eq!(
                agent_legend_line_count(width),
                agent_legend_lines(width).len() as u16,
                "legend line count disagrees with packing at width {width}"
            );
        }
    }

    #[test]
    fn every_chip_appears_exactly_once() {
        // At a comfortable width all five status chips pack without loss.
        let packed: Vec<usize> = agent_legend_lines(80).into_iter().flatten().collect();
        assert_eq!(packed, (0..STATUS_LEGEND.len()).collect::<Vec<_>>());
    }

    #[test]
    fn legend_yields_the_list_when_the_pane_is_too_short() {
        // A pane with no room beyond the header for both a list row and the
        // legend reserves nothing, so short panes render exactly as before.
        let tiny = Rect::new(0, 0, 30, AGENT_PANEL_HEADER_ROWS + 1);
        assert_eq!(agent_legend_reserved_rows(tiny), 0);
        assert_eq!(agent_legend_rect(tiny), Rect::default());

        // A taller pane reserves the legend lines plus the spacer row.
        let tall = Rect::new(0, 0, 30, 20);
        let reserved = agent_legend_reserved_rows(tall);
        assert!(reserved > 0);
        assert_eq!(
            reserved,
            agent_legend_line_count(tall.width) + AGENT_LEGEND_SPACER_ROWS
        );
    }
}
