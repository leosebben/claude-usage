//! Estado da TUI: dados carregados, filtros, painel em foco e seleção. A
//! leitura dos transcritos roda numa thread separada para não travar a
//! interface; o resultado chega por um canal e é consumido em `poll_load`.

use crate::data::{
    apply_filters, by_day, group_by, model_order, summarize, Bucket, DayBucket, Filters, GroupKey, Metric, Period, Row,
    Summary, PERIODS,
};
use crate::usage::{collect_usage, UsageEntry};
use chrono::{Local, NaiveDate};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

/// Intervalo de recarga automática dos transcritos.
pub const AUTO_RELOAD: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Days,
    Models,
    Projects,
    Sessions,
}

impl Pane {
    const ALL: [Pane; 4] = [Pane::Days, Pane::Models, Pane::Projects, Pane::Sessions];

    fn idx(self) -> usize {
        Pane::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }

    fn next(self) -> Pane {
        Pane::ALL[(self.idx() + 1) % Pane::ALL.len()]
    }

    fn prev(self) -> Pane {
        Pane::ALL[(self.idx() + Pane::ALL.len() - 1) % Pane::ALL.len()]
    }
}

/// Tudo que a tela mostra, já agregado para os filtros atuais.
#[derive(Default)]
pub struct View {
    pub summary: Summary,
    pub today: Summary,
    pub days: Vec<DayBucket>,
    pub models: Vec<Bucket>,
    pub projects: Vec<Bucket>,
    pub sessions: Vec<Bucket>,
}

type LoadResult = Result<Vec<UsageEntry>, String>;

pub struct App {
    home: PathBuf,
    pub rows: Option<Vec<Row>>,
    pub error: Option<String>,
    pub loading: bool,
    pub filters: Filters,
    pub metric: Metric,
    pub pane: Pane,
    /// Índice selecionado em cada painel, na ordem de `Pane::ALL`.
    pub selected: [usize; 4],
    /// Leva o cursor de dias para o último dia na próxima agregação (troca de
    /// período ou primeira carga).
    reset_day_cursor: bool,
    pub show_help: bool,
    pub quit: bool,
    /// Modelos na ordem de cor (custo total decrescente no conjunto completo).
    pub order: Vec<String>,
    pub view: View,
    pub last_load: Option<Instant>,
    tx: Sender<LoadResult>,
    rx: Receiver<LoadResult>,
}

impl App {
    pub fn new(home: PathBuf) -> App {
        let (tx, rx) = channel();
        let mut app = App {
            home,
            rows: None,
            error: None,
            loading: false,
            filters: Filters::default(),
            metric: Metric::Cost,
            pane: Pane::Models,
            selected: [0; 4],
            reset_day_cursor: true,
            show_help: false,
            quit: false,
            order: vec![],
            view: View::default(),
            last_load: None,
            tx,
            rx,
        };
        app.reload();
        app
    }

    pub fn today() -> NaiveDate {
        Local::now().date_naive()
    }

    /// Dispara a leitura em segundo plano (ignorada se já houver uma em curso).
    pub fn reload(&mut self) {
        if self.loading {
            return;
        }
        self.loading = true;
        let tx = self.tx.clone();
        let home = self.home.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| collect_usage(&home))
                .map_err(|_| "falha ao ler os transcritos".to_string());
            let _ = tx.send(result);
        });
    }

    /// Consome o resultado de uma leitura, se já tiver chegado. Retorna `true`
    /// quando os dados mudaram.
    pub fn poll_load(&mut self) -> bool {
        let Ok(result) = self.rx.try_recv() else { return false };
        self.loading = false;
        self.last_load = Some(Instant::now());
        match result {
            Ok(entries) => {
                let rows: Vec<Row> = entries.into_iter().map(Row::from_entry).collect();
                self.order = model_order(&rows);
                self.rows = Some(rows);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        self.recompute();
        true
    }

    /// Recarrega sozinho de tempos em tempos.
    pub fn tick(&mut self) {
        if !self.loading && self.last_load.is_some_and(|t| t.elapsed() >= AUTO_RELOAD) {
            self.reload();
        }
    }

    pub fn recompute(&mut self) {
        let Some(rows) = &self.rows else {
            self.view = View::default();
            return;
        };
        let today = App::today();
        let filtered = apply_filters(rows, &self.filters, today);
        let today_rows =
            apply_filters(rows, &Filters { period: Period::Today, day: None, ..self.filters.clone() }, today);
        // O gráfico mostra o período inteiro mesmo com um dia filtrado, para
        // dar para andar entre os dias.
        let period_rows = apply_filters(rows, &Filters { day: None, ..self.filters.clone() }, today);
        let mut sessions = group_by(&filtered, GroupKey::Session, self.metric);
        sessions.sort_by(|a, b| b.last.cmp(&a.last));
        self.view = View {
            summary: summarize(&filtered),
            today: summarize(&today_rows),
            days: by_day(&period_rows, self.metric, self.filters.period.start(today).as_deref(), today),
            models: group_by(&filtered, GroupKey::Model, self.metric),
            projects: group_by(&filtered, GroupKey::Project, self.metric),
            sessions,
        };
        for p in Pane::ALL {
            let len = self.list_len(p);
            let s = &mut self.selected[p.idx()];
            *s = if len == 0 { 0 } else { (*s).min(len - 1) };
        }
        if self.reset_day_cursor {
            self.selected[Pane::Days.idx()] = self.view.days.len().saturating_sub(1);
            self.reset_day_cursor = false;
        }
    }

    pub fn list_len(&self, pane: Pane) -> usize {
        match pane {
            Pane::Days => self.view.days.len(),
            Pane::Models => self.view.models.len(),
            Pane::Projects => self.view.projects.len(),
            Pane::Sessions => self.view.sessions.len(),
        }
    }

    pub fn selected_in(&self, pane: Pane) -> usize {
        self.selected[pane.idx()]
    }

    /// Cor (índice) de um modelo, estável entre filtros.
    pub fn color_index(&self, model: &str) -> usize {
        self.order.iter().position(|m| m == model).unwrap_or(0)
    }

    pub fn has_filter(&self) -> bool {
        self.filters.project.is_some() || self.filters.model.is_some() || self.filters.day.is_some()
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.list_len(self.pane);
        if len == 0 {
            return;
        }
        let s = &mut self.selected[self.pane.idx()];
        *s = (*s as isize + delta).clamp(0, len as isize - 1) as usize;
    }

    fn set_period(&mut self, period: Period) {
        if self.filters.period != period {
            self.filters.period = period;
            self.filters.day = None;
            self.reset_day_cursor = true;
            self.recompute();
        }
    }

    /// No painel de dias, `h`/`l` andam entre os dias em vez de trocar de painel.
    fn move_horizontal(&mut self, delta: isize) {
        if self.pane == Pane::Days {
            self.move_selection(delta);
        } else if delta > 0 {
            self.pane = self.pane.next();
        } else {
            self.pane = self.pane.prev();
        }
    }

    /// Enter/espaço: liga ou desliga o filtro do item selecionado.
    fn toggle_selected(&mut self) {
        let i = self.selected_in(self.pane);
        match self.pane {
            Pane::Days => {
                let Some(d) = self.view.days.get(i) else { return };
                let key = d.date.clone();
                self.filters.day = if self.filters.day.as_deref() == Some(&key) { None } else { Some(key) };
            }
            Pane::Models => {
                let Some(b) = self.view.models.get(i) else { return };
                let key = b.key.clone();
                self.filters.model = if self.filters.model.as_deref() == Some(&key) { None } else { Some(key) };
            }
            Pane::Projects => {
                let Some(b) = self.view.projects.get(i) else { return };
                let key = b.key.clone();
                self.filters.project = if self.filters.project.as_deref() == Some(&key) { None } else { Some(key) };
            }
            Pane::Sessions => return,
        }
        self.recompute();
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.show_help {
            // Qualquer tecla fecha a ajuda; `q` também sai.
            self.show_help = false;
            if matches!(key.code, KeyCode::Char('q')) || (ctrl && matches!(key.code, KeyCode::Char('c'))) {
                self.quit = true;
            }
            return;
        }
        match key.code {
            KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Esc | KeyCode::Char('x') => {
                if self.has_filter() {
                    self.filters.model = None;
                    self.filters.project = None;
                    self.filters.day = None;
                    self.recompute();
                }
            }
            KeyCode::Char('r') => self.reload(),
            KeyCode::Char('t') | KeyCode::Char('m') => {
                self.metric = match self.metric {
                    Metric::Cost => Metric::Tokens,
                    Metric::Tokens => Metric::Cost,
                };
                self.recompute();
            }
            KeyCode::Char(c @ '1'..='4') => self.set_period(PERIODS[(c as u8 - b'1') as usize]),
            KeyCode::Tab => self.pane = self.pane.next(),
            KeyCode::BackTab => self.pane = self.pane.prev(),
            KeyCode::Right | KeyCode::Char('l') => self.move_horizontal(1),
            KeyCode::Left | KeyCode::Char('h') => self.move_horizontal(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageDown | KeyCode::Char('d') => self.move_selection(10),
            KeyCode::PageUp | KeyCode::Char('u') => self.move_selection(-10),
            KeyCode::Home | KeyCode::Char('g') => self.move_selection(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.move_selection(isize::MAX / 2),
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle_selected(),
            _ => {}
        }
    }
}
