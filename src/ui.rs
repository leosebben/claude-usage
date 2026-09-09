//! Desenho da tela. Layout, de cima para baixo: cabeçalho com filtros, quatro
//! tiles de resumo, gráfico por dia, painéis por modelo e por projeto lado a
//! lado, tabela de sessões e rodapé com as teclas.

use crate::app::{App, Pane};
use crate::data::{
    fmt_date, fmt_date_short, fmt_date_time, fmt_metric, fmt_tokens, fmt_usd, project_name, short_model, short_path,
    Bucket, DayBucket, Metric, PERIODS,
};
use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Bar, BarChart, BarGroup, Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Row as TableRow,
        Table, TableState, Wrap,
    },
    Frame,
};

const PALETTE: [Color; 8] = [
    Color::Cyan,
    Color::Magenta,
    Color::Yellow,
    Color::Green,
    Color::Blue,
    Color::Red,
    Color::LightCyan,
    Color::LightMagenta,
];

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;

fn model_color(app: &App, model: &str) -> Color {
    PALETTE[app.color_index(model) % PALETTE.len()]
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [header, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());

    draw_header(frame, app, header);
    draw_footer(frame, app, footer);

    if app.rows.is_none() {
        let msg = match &app.error {
            Some(e) => Line::from(e.as_str()).red(),
            None => Line::from("Lendo transcritos…").fg(DIM),
        };
        let [row] = Layout::vertical([Constraint::Length(1)]).flex(Flex::Center).areas(body);
        frame.render_widget(Paragraph::new(msg).centered(), row);
        return;
    }

    // Gráfico mais baixo em terminais pequenos para sobrar espaço às listas.
    let chart_h = match body.height {
        0..=24 => 7,
        25..=34 => 9,
        _ => 12,
    };
    let [tiles, chart, middle, sessions] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(chart_h),
        Constraint::Fill(3),
        Constraint::Fill(3),
    ])
    .areas(body);

    draw_tiles(frame, app, tiles);
    draw_chart(frame, app, chart);
    let [models, projects] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(middle);
    draw_models(frame, app, models);
    draw_projects(frame, app, projects);
    draw_sessions(frame, app, sessions);

    if app.show_help {
        draw_help(frame, frame.area());
    }
}

// ---------------------------------------------------------------------------

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans: Vec<Span> = vec![Span::from(" claude-usage ").bold().fg(Color::Black).bg(ACCENT), Span::from("  ")];
    for (i, p) in PERIODS.iter().enumerate() {
        let label = format!(" ({}) {} ", i + 1, p.label());
        spans.push(if *p == app.filters.period {
            Span::from(label).bold().fg(ACCENT).add_modifier(Modifier::UNDERLINED)
        } else {
            Span::from(label).fg(DIM)
        });
    }
    spans.push(Span::from("  │  "));
    for m in [Metric::Cost, Metric::Tokens] {
        let label = format!(" {} ", m.label());
        spans.push(if m == app.metric {
            Span::from(label).bold().fg(ACCENT).add_modifier(Modifier::UNDERLINED)
        } else {
            Span::from(label).fg(DIM)
        });
    }
    if let Some(d) = &app.filters.day {
        spans.push(Span::from("  │  dia: ").fg(DIM));
        spans.push(Span::from(fmt_date(d)).bold().yellow());
    }
    if let Some(p) = &app.filters.project {
        spans.push(Span::from("  │  projeto: ").fg(DIM));
        spans.push(Span::from(project_name(p)).bold().yellow());
    }
    if let Some(m) = &app.filters.model {
        spans.push(Span::from("  │  modelo: ").fg(DIM));
        spans.push(Span::styled(short_model(m), Style::new().bold().fg(model_color(app, m))));
    }
    if app.has_filter() {
        spans.push(Span::from("  Esc limpa").fg(DIM));
    }

    let status = if app.loading {
        Span::from("⟳ lendo… ").fg(Color::Yellow)
    } else if let Some(rows) = &app.rows {
        Span::from(format!("{} respostas ", rows.len())).fg(DIM)
    } else {
        Span::from("")
    };
    let status_w = status.width() as u16;
    let [left, right] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(status_w)]).areas(area);
    frame.render_widget(Paragraph::new(Line::from(spans)), left);
    frame.render_widget(Paragraph::new(Line::from(status)).right_aligned(), right);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let key = |k: &str, what: &str| {
        vec![Span::from(format!(" {k} ")).fg(Color::Black).bg(DIM), Span::from(format!(" {what}  ")).fg(DIM)]
    };
    let mut spans = Vec::new();
    for (k, w) in [
        ("1-4", "período"),
        ("t", "métrica"),
        ("Tab", "painel"),
        ("↑↓", "mover"),
        ("⏎", "filtrar"),
        ("Esc", "limpar"),
        ("r", "recarregar"),
        ("?", "ajuda"),
        ("q", "sair"),
    ] {
        spans.extend(key(k, w));
    }
    let note = if app.view.summary.unpriced > 0 {
        format!("{} respostas sem preço conhecido fora do custo ", app.view.summary.unpriced)
    } else {
        String::new()
    };
    let [left, right] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(note.len() as u16)]).areas(area);
    frame.render_widget(Paragraph::new(Line::from(spans)), left);
    frame.render_widget(Paragraph::new(Line::from(note).fg(Color::Yellow)).right_aligned(), right);
}

// ---------------------------------------------------------------------------

/// Tile de resumo. O badge e o subtexto só aparecem quando cabem, para o
/// título e o valor nunca serem cortados.
fn tile(frame: &mut Frame, area: Rect, label: &str, badge: String, value: String, sub: String) {
    let w = area.width as usize;
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(DIM))
        .title(Span::from(format!(" {label} ")).bold());
    if w >= label.chars().count() + badge.chars().count() + 8 {
        block = block.title_top(Line::from(Span::from(format!(" {badge} ")).fg(DIM)).right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut spans = vec![Span::from(" "), Span::from(value.clone()).bold().fg(ACCENT)];
    if inner.width as usize >= value.chars().count() + sub.chars().count() + 4 {
        spans.push(Span::from("  "));
        spans.push(Span::from(sub).fg(DIM));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

fn draw_tiles(frame: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::horizontal([Constraint::Percentage(25); 4]).split(area);
    let s = &app.view.summary;
    let t = &app.view.today;
    let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    tile(
        frame,
        cols[0],
        "Custo",
        format!("{} respostas", s.messages),
        fmt_usd(s.cost),
        match &app.filters.day {
            Some(d) => fmt_date(d),
            None => app.filters.period.label().to_string(),
        },
    );
    tile(
        frame,
        cols[1],
        "Hoje",
        format!("{} respostas", t.messages),
        fmt_usd(t.cost),
        format!("{} tokens", fmt_tokens(t.tokens)),
    );
    tile(
        frame,
        cols[2],
        "Tokens",
        format!("{} saída", fmt_tokens(s.output)),
        fmt_tokens(s.tokens),
        format!("{} lidos do cache", fmt_tokens(s.cache_read)),
    );
    tile(
        frame,
        cols[3],
        "Sessões",
        plural(app.view.projects.len(), "projeto", "projetos"),
        s.sessions.to_string(),
        plural(app.view.models.len(), "modelo", "modelos"),
    );
}

// ---------------------------------------------------------------------------

fn dominant_model(d: &DayBucket) -> Option<&str> {
    d.by_model.iter().max_by(|a, b| a.1.total_cmp(b.1)).map(|(m, _)| m.as_str())
}

fn draw_chart(frame: &mut Frame, app: &App, area: Rect) {
    let days = &app.view.days;
    let max = days.iter().map(|d| d.total).fold(0.0, f64::max);

    let mut legend: Vec<Span> = Vec::new();
    for m in &app.order {
        if app.view.models.iter().any(|b| &b.key == m) {
            legend.push(Span::styled(" ● ", Style::new().fg(model_color(app, m))));
            legend.push(Span::from(short_model(m)).fg(DIM));
        }
    }
    legend.push(Span::from(" ← → ⏎ filtra dia ").fg(DIM));

    let focused = app.pane == Pane::Days;
    let cursor = app.selected_in(Pane::Days);
    // Com o painel em foco, o título mostra o dia sob o cursor e o valor dele;
    // fora de foco, o dia filtrado ou o máximo do período.
    let title = match (focused.then(|| days.get(cursor)).flatten(), &app.filters.day) {
        (Some(d), _) => format!(" Por dia › {} · {} ", fmt_date(&d.date), fmt_metric(d.total, app.metric)),
        (None, Some(d)) => format!(" Por dia · {} ", fmt_date(d)),
        (None, None) if max > 0.0 => format!(" Por dia · máx {} ", fmt_metric(max, app.metric)),
        (None, None) => " Por dia ".to_string(),
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if focused { ACCENT } else { DIM }))
        .title(Span::from(title).bold())
        .title_top(Line::from(legend).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if days.is_empty() || max <= 0.0 {
        frame.render_widget(Paragraph::new("Sem uso no período.").fg(DIM).centered(), inner);
        return;
    }

    // Largura das barras: quantas cabem com espaço de 1 entre elas. Quando
    // não cabem todas, a janela termina no último dia mas segue o cursor.
    let w = inner.width as usize;
    let n = days.len();
    let bar_w = ((w + 1) / n).saturating_sub(1).clamp(1, 12);
    let fit = ((w + 1) / (bar_w + 1)).max(1);
    let start = if fit >= n { 0 } else { (n - fit).min(cursor) };
    let shown = &days[start..(start + fit).min(n)];

    let scale = match app.metric {
        Metric::Cost => 100.0,
        Metric::Tokens => 1.0,
    };
    let bars: Vec<Bar> = shown
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let label = if bar_w >= 5 {
                fmt_date_short(&d.date)
            } else if bar_w >= 2 {
                fmt_date(&d.date)[..2].to_string()
            } else {
                String::new()
            };
            let text = if bar_w >= 6 && d.total > 0.0 { short_value(d.total, app.metric) } else { String::new() };
            let is_cursor = focused && start + i == cursor;
            let is_filtered = app.filters.day.as_deref() == Some(&d.date);
            let color =
                if is_cursor { Color::White } else { dominant_model(d).map(|m| model_color(app, m)).unwrap_or(DIM) };
            let label_style = if is_filtered {
                Style::new().bold().yellow()
            } else if is_cursor {
                Style::new().bold().fg(Color::White)
            } else {
                Style::new().fg(DIM)
            };
            // Dias vazios ainda precisam de uma barra mínima para o cursor aparecer.
            let value = (d.total * scale).round() as u64;
            Bar::default()
                .value(if is_cursor && value == 0 { 1 } else { value })
                .label(Line::from(label).style(label_style))
                .text_value(text)
                .style(Style::new().fg(color))
                .value_style(Style::new().fg(Color::Black).bg(color))
        })
        .collect();

    let chart = BarChart::default()
        .data(BarGroup::default().bars(&bars))
        .bar_width(bar_w as u16)
        .bar_gap(1)
        .max((max * scale).round().max(1.0) as u64);
    frame.render_widget(chart, inner);
}

/// Valor curto para caber em cima da barra: `12` (US$) ou `1,2M`.
fn short_value(v: f64, metric: Metric) -> String {
    match metric {
        Metric::Cost => {
            if v >= 100.0 {
                format!("{}", v.round() as u64)
            } else if v >= 10.0 {
                format!("{v:.0}")
            } else {
                format!("{v:.1}").replace('.', ",")
            }
        }
        Metric::Tokens => fmt_tokens(v.round() as u64).replace(' ', ""),
    }
}

// ---------------------------------------------------------------------------

fn pane_block(title: &str, focused: bool, hint: &str) -> Block<'static> {
    let border = if focused { ACCENT } else { DIM };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(border))
        .title(Span::from(format!(" {title} ")).bold())
        .title_top(Line::from(Span::from(format!(" {hint} ")).fg(DIM)).right_aligned())
}

struct BarItem {
    dot: Option<Color>,
    label: String,
    value: f64,
    text: String,
    sub: String,
    active: bool,
}

/// Linha de lista com barra proporcional: `● rótulo ████░░░ valor  detalhe`.
fn bar_lines(items: &[BarItem], width: usize) -> Vec<ListItem<'static>> {
    let max = items.iter().map(|i| i.value).fold(0.0, f64::max);
    let label_w = items.iter().map(|i| i.label.chars().count()).max().unwrap_or(0).clamp(4, 22);
    let text_w = items.iter().map(|i| i.text.chars().count()).max().unwrap_or(0);
    let sub_w = items.iter().map(|i| i.sub.chars().count()).max().unwrap_or(0);
    let has_dot = items.iter().any(|i| i.dot.is_some());
    let fixed = 2 + if has_dot { 2 } else { 0 } + label_w + 1 + text_w + 1;
    let mut bar_w = width.saturating_sub(fixed + sub_w + 2);
    let show_sub = bar_w >= 6;
    if !show_sub {
        bar_w = width.saturating_sub(fixed);
    }
    let bar_w = bar_w.min(40);

    items
        .iter()
        .map(|i| {
            let mut spans: Vec<Span> = Vec::new();
            if let Some(c) = i.dot {
                spans.push(Span::styled("● ", Style::new().fg(c)));
            } else if has_dot {
                spans.push(Span::from("  "));
            }
            let label: String = truncate(&i.label, label_w);
            let label_style = if i.active { Style::new().bold().yellow() } else { Style::new() };
            spans.push(Span::styled(format!("{label:<label_w$} "), label_style));
            if bar_w >= 3 {
                let filled = if max > 0.0 { ((i.value / max) * bar_w as f64).round() as usize } else { 0 };
                let color = i.dot.unwrap_or(ACCENT);
                spans.push(Span::styled("█".repeat(filled), Style::new().fg(color)));
                spans.push(Span::styled("░".repeat(bar_w - filled), Style::new().fg(DIM)));
                spans.push(Span::from(" "));
            }
            spans.push(Span::from(format!("{:>text_w$}", i.text)).bold());
            if show_sub && !i.sub.is_empty() {
                spans.push(Span::from(format!("  {}", i.sub)).fg(DIM));
            }
            ListItem::new(Line::from(spans))
        })
        .collect()
}

fn truncate(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w {
        s.to_string()
    } else if w <= 1 {
        "…".to_string()
    } else {
        let head: String = s.chars().take(w - 1).collect();
        format!("{head}…")
    }
}

fn draw_bar_pane(frame: &mut Frame, app: &App, area: Rect, pane: Pane, title: &str, items: Vec<BarItem>) {
    let focused = app.pane == pane;
    let hint = match pane {
        Pane::Models => "⏎ filtra modelo",
        Pane::Projects => "⏎ filtra projeto",
        Pane::Days | Pane::Sessions => "",
    };
    let block = pane_block(title, focused, hint);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if items.is_empty() {
        frame.render_widget(Paragraph::new("Sem uso no período.").fg(DIM).centered(), inner);
        return;
    }
    let lines = bar_lines(&items, inner.width.saturating_sub(2) as usize);
    let list = List::new(lines).highlight_symbol(if focused { "› " } else { "  " }).highlight_style(if focused {
        Style::new().bg(Color::Rgb(45, 50, 60))
    } else {
        Style::new()
    });
    let mut state = ListState::default().with_selected(Some(app.selected_in(pane)));
    frame.render_stateful_widget(list, inner, &mut state);
}

fn draw_models(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<BarItem> = app
        .view
        .models
        .iter()
        .map(|b| BarItem {
            dot: Some(model_color(app, &b.key)),
            label: short_model(&b.key),
            value: b.value(app.metric),
            text: bucket_text(b, app.metric),
            sub: format!("{} resp · {} tok", b.messages, fmt_tokens(b.tokens)),
            active: app.filters.model.as_deref() == Some(&b.key),
        })
        .collect();
    draw_bar_pane(frame, app, area, Pane::Models, "Por modelo", items);
}

fn draw_projects(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<BarItem> = app
        .view
        .projects
        .iter()
        .map(|b| BarItem {
            dot: None,
            label: project_name(&b.key),
            value: b.value(app.metric),
            text: bucket_text(b, app.metric),
            sub: short_path(&b.key),
            active: app.filters.project.as_deref() == Some(&b.key),
        })
        .collect();
    draw_bar_pane(frame, app, area, Pane::Projects, "Por projeto", items);
}

fn bucket_text(b: &Bucket, metric: Metric) -> String {
    match metric {
        Metric::Cost => b.fmt_cost(),
        Metric::Tokens => fmt_tokens(b.tokens),
    }
}

// ---------------------------------------------------------------------------

fn draw_sessions(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.pane == Pane::Sessions;
    let n = app.view.sessions.len();
    let block = pane_block("Sessões", focused, &format!("{n} no período"));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if n == 0 {
        frame.render_widget(Paragraph::new("Sem sessões no período.").fg(DIM).centered(), inner);
        return;
    }

    let rows: Vec<TableRow> = app
        .view
        .sessions
        .iter()
        .map(|s| {
            let start = chrono::DateTime::parse_from_rfc3339(&s.first)
                .map(|d| fmt_date_time(&d.with_timezone(&chrono::Local)))
                .unwrap_or_else(|_| s.first.clone());
            let mut models: Vec<Span> = Vec::new();
            for m in &s.models {
                models.push(Span::styled("● ", Style::new().fg(model_color(app, m))));
                models.push(Span::from(format!("{} ", short_model(m))));
            }
            TableRow::new(vec![
                Line::from(start).fg(DIM),
                Line::from(s.projects.iter().map(|p| project_name(p)).collect::<Vec<_>>().join(", ")),
                Line::from(models),
                Line::from(s.messages.to_string()).right_aligned(),
                Line::from(fmt_tokens(s.tokens)).right_aligned(),
                Line::from(s.fmt_cost()).right_aligned().bold(),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Fill(3),
            Constraint::Fill(2),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(13),
        ],
    )
    .header(
        TableRow::new(vec![
            Line::from("Início"),
            Line::from("Projeto"),
            Line::from("Modelos"),
            Line::from("Respostas").right_aligned(),
            Line::from("Tokens").right_aligned(),
            Line::from("Custo").right_aligned(),
        ])
        .fg(DIM)
        .underlined(),
    )
    .column_spacing(1)
    .highlight_symbol(if focused { "› " } else { "  " })
    .row_highlight_style(if focused { Style::new().bg(Color::Rgb(45, 50, 60)) } else { Style::new() });
    let mut state = TableState::default().with_selected(Some(app.selected_in(Pane::Sessions)));
    frame.render_stateful_widget(table, inner, &mut state);
}

// ---------------------------------------------------------------------------

fn draw_help(frame: &mut Frame, area: Rect) {
    let lines = [
        ("1 2 3 4", "período: hoje, 7 dias, 30 dias, tudo"),
        ("t / m", "alterna entre custo e tokens"),
        ("Tab / ← → / h l", "painel anterior / seguinte (no gráfico, ← → andam entre os dias)"),
        ("↑ ↓ / j k", "move a seleção"),
        ("PgUp PgDn / u d", "move 10 linhas"),
        ("Home End / g G", "início / fim da lista"),
        ("Enter / espaço", "liga ou desliga o filtro do item (dia, modelo ou projeto)"),
        ("Esc / x", "limpa os filtros de dia, modelo e projeto"),
        ("r", "recarrega os transcritos (automático a cada 30 s)"),
        ("q / Ctrl-C", "sai"),
    ];
    let key_w = lines.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);
    let mut text: Vec<Line> = lines
        .iter()
        .map(|(k, what)| Line::from(vec![Span::from(format!(" {k:<key_w$}  ")).bold().fg(ACCENT), Span::from(*what)]))
        .collect();
    text.push(Line::from(""));
    text.push(
        Line::from(" Custo estimado com os preços da API da Anthropic; em planos por assinatura é só referência.")
            .fg(DIM),
    );
    text.push(Line::from(" Qualquer tecla fecha esta ajuda.").fg(DIM));

    let height = (text.len() + 2) as u16;
    let width = 90u16.min(area.width.saturating_sub(4));
    let [v] = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center).areas(area);
    let [popup] = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center).areas(v);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: false }).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(ACCENT))
                .title(Span::from(" Teclas ").bold()),
        ),
        popup,
    );
}

#[cfg(test)]
mod real_home {
    use super::*;
    use ratatui::{backend::TestBackend, crossterm::event::KeyCode, Terminal};

    /// `cargo test real_home -- --ignored --nocapture` imprime a tela com os
    /// dados reais de `~/.claude`, em vários tamanhos.
    #[test]
    #[ignore]
    fn render() {
        let mut app = App::new(crate::usage::claude_home().unwrap());
        while !app.poll_load() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(app.rows.is_some(), "{:?}", app.error);
        for (w, h) in [(140, 45), (100, 32), (80, 24)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| draw(f, &mut app)).unwrap();
            println!("--- {w}x{h}\n{}", t.backend());
        }
        // Painel de dias em foco, cursor dois dias para trás e filtro ligado.
        app.pane = Pane::Days;
        app.on_key(KeyCode::Left.into());
        app.on_key(KeyCode::Left.into());
        app.on_key(KeyCode::Enter.into());
        let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
        t.draw(|f| draw(f, &mut app)).unwrap();
        println!("--- dia filtrado\n{}", t.backend());
        app.on_key(KeyCode::Esc.into());
        app.pane = Pane::Models;

        app.show_help = true;
        let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
        t.draw(|f| draw(f, &mut app)).unwrap();
        println!("--- ajuda\n{}", t.backend());
    }
}
