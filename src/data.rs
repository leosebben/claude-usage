//! Agregações puras sobre as entradas de uso e formatação pt-BR. Sem nada de
//! terminal, para poder ser testado direto.

use crate::usage::UsageEntry;
use chrono::{DateTime, Datelike, Local, NaiveDate, Timelike};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Entrada de uso com o instante já convertido para o fuso local.
#[derive(Clone, Debug)]
pub struct Row {
    pub e: UsageEntry,
    /// Data local `YYYY-MM-DD`.
    pub date: String,
}

impl Row {
    pub fn from_entry(e: UsageEntry) -> Row {
        let local = DateTime::parse_from_rfc3339(&e.timestamp)
            .map(|d| d.with_timezone(&Local))
            .unwrap_or_else(|_| DateTime::<Local>::from(std::time::UNIX_EPOCH));
        Row { date: local_date(&local), e }
    }
}

pub fn local_date(d: &DateTime<Local>) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Today,
    Days7,
    Days30,
    All,
}

pub const PERIODS: [Period; 4] = [Period::Today, Period::Days7, Period::Days30, Period::All];

impl Period {
    pub fn label(self) -> &'static str {
        match self {
            Period::Today => "Hoje",
            Period::Days7 => "7 dias",
            Period::Days30 => "30 dias",
            Period::All => "Tudo",
        }
    }

    /// Primeira data (local) incluída no período, ou `None` para "tudo".
    pub fn start(self, today: NaiveDate) -> Option<String> {
        let days = match self {
            Period::Today => 0,
            Period::Days7 => 6,
            Period::Days30 => 29,
            Period::All => return None,
        };
        let d = today - chrono::Duration::days(days);
        Some(d.format("%Y-%m-%d").to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    Cost,
    Tokens,
}

impl Metric {
    pub fn label(self) -> &'static str {
        match self {
            Metric::Cost => "Custo",
            Metric::Tokens => "Tokens",
        }
    }

    pub fn of(self, e: &UsageEntry) -> f64 {
        match self {
            Metric::Cost => e.cost_usd.unwrap_or(0.0),
            Metric::Tokens => e.total_tokens() as f64,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filters {
    pub period: Period,
    pub project: Option<String>,
    pub model: Option<String>,
    /// Data local `YYYY-MM-DD`; restringe tudo a um único dia dentro do período.
    pub day: Option<String>,
}

impl Default for Filters {
    fn default() -> Self {
        Filters { period: Period::Days7, project: None, model: None, day: None }
    }
}

pub fn apply_filters<'a>(rows: &'a [Row], f: &Filters, today: NaiveDate) -> Vec<&'a Row> {
    let start = f.period.start(today);
    rows.iter()
        .filter(|r| {
            start.as_deref().is_none_or(|s| r.date.as_str() >= s)
                && f.day.as_deref().is_none_or(|d| r.date == d)
                && f.project.as_deref().is_none_or(|p| r.e.project_path == p)
                && f.model.as_deref().is_none_or(|m| r.e.model == m)
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub cost: f64,
    pub tokens: u64,
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub messages: usize,
    pub sessions: usize,
    /// Mensagens de modelos sem preço conhecido.
    pub unpriced: usize,
}

pub fn summarize(rows: &[&Row]) -> Summary {
    let mut s = Summary { messages: rows.len(), ..Default::default() };
    let mut sessions = HashSet::new();
    for r in rows {
        let e = &r.e;
        s.cost += e.cost_usd.unwrap_or(0.0);
        if e.cost_usd.is_none() {
            s.unpriced += 1;
        }
        s.tokens += e.total_tokens();
        s.input += e.input_tokens;
        s.output += e.output_tokens;
        s.cache_write += e.cache_write_tokens;
        s.cache_read += e.cache_read_tokens;
        sessions.insert(e.session_id.as_str());
    }
    s.sessions = sessions.len();
    s
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bucket {
    pub key: String,
    pub cost: f64,
    pub tokens: u64,
    pub messages: usize,
    /// Respostas de modelos sem preço conhecido.
    pub unpriced: usize,
    pub first: String,
    pub last: String,
    pub models: Vec<String>,
    pub projects: Vec<String>,
}

impl Bucket {
    pub fn value(&self, metric: Metric) -> f64 {
        match metric {
            Metric::Cost => self.cost,
            Metric::Tokens => self.tokens as f64,
        }
    }

    /// Custo do grupo: "—" quando nenhuma resposta tinha preço conhecido.
    pub fn fmt_cost(&self) -> String {
        if self.unpriced == self.messages {
            "—".to_string()
        } else {
            fmt_usd(self.cost)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupKey {
    Model,
    Project,
    Session,
}

/// Agrupa por chave, ordenado pela métrica em ordem decrescente.
pub fn group_by(rows: &[&Row], key: GroupKey, metric: Metric) -> Vec<Bucket> {
    struct Acc {
        b: Bucket,
        models: HashSet<String>,
        projects: HashSet<String>,
    }
    let mut map: HashMap<&str, Acc> = HashMap::new();
    let mut order: Vec<&str> = Vec::new();
    for r in rows {
        let e = &r.e;
        let k = match key {
            GroupKey::Model => e.model.as_str(),
            GroupKey::Project => e.project_path.as_str(),
            GroupKey::Session => e.session_id.as_str(),
        };
        let a = map.entry(k).or_insert_with(|| {
            order.push(k);
            Acc {
                b: Bucket {
                    key: k.to_string(),
                    cost: 0.0,
                    tokens: 0,
                    messages: 0,
                    unpriced: 0,
                    first: e.timestamp.clone(),
                    last: e.timestamp.clone(),
                    models: vec![],
                    projects: vec![],
                },
                models: HashSet::new(),
                projects: HashSet::new(),
            }
        });
        a.b.cost += e.cost_usd.unwrap_or(0.0);
        if e.cost_usd.is_none() {
            a.b.unpriced += 1;
        }
        a.b.tokens += e.total_tokens();
        a.b.messages += 1;
        if e.timestamp < a.b.first {
            a.b.first = e.timestamp.clone();
        }
        if e.timestamp > a.b.last {
            a.b.last = e.timestamp.clone();
        }
        a.models.insert(e.model.clone());
        a.projects.insert(e.project_path.clone());
    }
    let mut out: Vec<Bucket> = order
        .into_iter()
        .filter_map(|k| map.remove(k))
        .map(|a| {
            let mut models: Vec<_> = a.models.into_iter().collect();
            models.sort();
            let mut projects: Vec<_> = a.projects.into_iter().collect();
            projects.sort();
            Bucket { models, projects, ..a.b }
        })
        .collect();
    out.sort_by(|a, b| b.value(metric).total_cmp(&a.value(metric)));
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct DayBucket {
    pub date: String,
    pub total: f64,
    /// Valor da métrica por modelo, só para modelos presentes no dia.
    pub by_model: BTreeMap<String, f64>,
}

/// Série diária contínua (dias sem uso entram zerados) entre a primeira e a
/// última data do conjunto, ou entre `start` e hoje quando `start` é dado.
pub fn by_day(rows: &[&Row], metric: Metric, start: Option<&str>, today: NaiveDate) -> Vec<DayBucket> {
    let mut map: BTreeMap<String, DayBucket> = BTreeMap::new();
    for r in rows {
        let d = map.entry(r.date.clone()).or_insert_with(|| DayBucket {
            date: r.date.clone(),
            total: 0.0,
            by_model: BTreeMap::new(),
        });
        let v = metric.of(&r.e);
        d.total += v;
        *d.by_model.entry(r.e.model.clone()).or_insert(0.0) += v;
    }
    if map.is_empty() && start.is_none() {
        return vec![];
    }
    let from = start.map(str::to_string).unwrap_or_else(|| map.keys().next().cloned().unwrap());
    let to =
        if start.is_some() { today.format("%Y-%m-%d").to_string() } else { map.keys().next_back().cloned().unwrap() };
    let Ok(mut cursor) = NaiveDate::parse_from_str(&from, "%Y-%m-%d") else { return vec![] };
    let mut out = Vec::new();
    for _ in 0..400 {
        let date = cursor.format("%Y-%m-%d").to_string();
        if date > to {
            break;
        }
        out.push(map.remove(&date).unwrap_or_else(|| DayBucket {
            date: date.clone(),
            total: 0.0,
            by_model: BTreeMap::new(),
        }));
        cursor += chrono::Duration::days(1);
    }
    out
}

/// Ordem fixa de cores por modelo: pelo custo total no conjunto completo.
pub fn model_order(rows: &[Row]) -> Vec<String> {
    let all: Vec<&Row> = rows.iter().collect();
    group_by(&all, GroupKey::Model, Metric::Cost).into_iter().map(|b| b.key).collect()
}

// ---------------------------------------------------------------------------
// Formatação (pt-BR: ponto de milhar, vírgula decimal)

fn group_thousands(int: u64) -> String {
    let s = int.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// Número com no máximo `max_dec` casas, sem zeros à direita.
fn fmt_num(v: f64, max_dec: usize) -> String {
    let s = format!("{v:.max_dec$}");
    let (int, dec) = s.split_once('.').unwrap_or((&s, ""));
    let dec = dec.trim_end_matches('0');
    let int = group_thousands(int.parse().unwrap_or(0));
    if dec.is_empty() {
        int
    } else {
        format!("{int},{dec}")
    }
}

pub fn fmt_usd(v: f64) -> String {
    let s = format!("{v:.2}");
    let (int, dec) = s.split_once('.').unwrap_or((&s, "00"));
    format!("US$ {},{}", group_thousands(int.parse().unwrap_or(0)), dec)
}

pub fn fmt_tokens(v: u64) -> String {
    let f = v as f64;
    if f >= 1e9 {
        format!("{} B", fmt_num((f / 1e7).round() / 100.0, 2))
    } else if f >= 1e6 {
        format!("{} M", fmt_num((f / 1e4).round() / 100.0, 2))
    } else if f >= 1e3 {
        format!("{} k", group_thousands((f / 1e3).round() as u64))
    } else {
        group_thousands(v)
    }
}

pub fn fmt_metric(v: f64, metric: Metric) -> String {
    match metric {
        Metric::Cost => fmt_usd(v),
        Metric::Tokens => fmt_tokens(v.round() as u64),
    }
}

/// `claude-fable-5-1` -> `fable 5.1`.
pub fn short_model(model: &str) -> String {
    let rest = match model.strip_prefix("claude-") {
        Some(r) => r,
        None => return model.to_string(),
    };
    let mut parts = rest.split('-');
    let Some(family) = parts.next().filter(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_lowercase())) else {
        return model.to_string();
    };
    let Some(major) = parts.next().filter(|m| !m.is_empty() && m.chars().all(|c| c.is_ascii_digit())) else {
        return model.to_string();
    };
    match parts.next().filter(|m| !m.is_empty() && m.chars().all(|c| c.is_ascii_digit())) {
        Some(minor) => format!("{family} {major}.{minor}"),
        None => format!("{family} {major}"),
    }
}

pub fn short_path(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("/Users/") {
        match rest.split_once('/') {
            Some((_, tail)) => format!("~/{tail}"),
            None => "~".to_string(),
        }
    } else {
        p.to_string()
    }
}

pub fn project_name(p: &str) -> String {
    let s = short_path(p);
    if s == "~" {
        return s;
    }
    s.split('/').rfind(|x| !x.is_empty()).map(str::to_string).unwrap_or(s)
}

/// `2026-09-03` -> `03/09/26`.
pub fn fmt_date(date: &str) -> String {
    let mut it = date.split('-');
    match (it.next(), it.next(), it.next()) {
        (Some(y), Some(m), Some(d)) => format!("{d}/{m}/{}", &y[y.len().saturating_sub(2)..]),
        _ => date.to_string(),
    }
}

/// `03/09/26` (sem o ano quando `with_year` é falso: `03/09`).
pub fn fmt_date_short(date: &str) -> String {
    fmt_date(date)[..5].to_string()
}

pub fn fmt_date_time(d: &DateTime<Local>) -> String {
    format!("{} {:02}:{:02}", fmt_date(&local_date(d)), d.hour(), d.minute())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ts: &str, model: &str, project: &str, session: &str, cost: Option<f64>) -> Row {
        Row::from_entry(UsageEntry {
            timestamp: ts.to_string(),
            model: model.to_string(),
            slug: "s".into(),
            project_path: project.to_string(),
            session_id: session.to_string(),
            input_tokens: 10,
            output_tokens: 20,
            cache_write_tokens: 30,
            cache_read_tokens: 40,
            cost_usd: cost,
        })
    }

    fn sample() -> Vec<Row> {
        vec![
            row("2026-09-01T12:00:00Z", "claude-opus-5", "/Users/x/a", "s1", Some(1.0)),
            row("2026-09-02T12:00:00Z", "claude-sonnet-5", "/Users/x/a", "s1", Some(0.5)),
            row("2026-09-03T12:00:00Z", "claude-opus-5", "/Users/x/b", "s2", Some(2.0)),
            row("2026-09-03T13:00:00Z", "grok-9", "/Users/x/b", "s3", None),
        ]
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 3).unwrap()
    }

    #[test]
    fn period_start() {
        assert_eq!(Period::Today.start(today()).as_deref(), Some("2026-09-03"));
        assert_eq!(Period::Days7.start(today()).as_deref(), Some("2026-08-28"));
        assert_eq!(Period::All.start(today()), None);
    }

    #[test]
    fn filters_and_summary() {
        let rows = sample();
        let f = Filters { period: Period::Today, ..Default::default() };
        let filtered = apply_filters(&rows, &f, today());
        assert_eq!(filtered.len(), 2);
        let s = summarize(&filtered);
        assert_eq!(s.messages, 2);
        assert_eq!(s.sessions, 2);
        assert_eq!(s.unpriced, 1);
        assert_eq!(s.tokens, 200);
        assert!((s.cost - 2.0).abs() < 1e-9);

        let f = Filters {
            period: Period::All,
            project: Some("/Users/x/a".into()),
            model: Some("claude-opus-5".into()),
            day: None,
        };
        assert_eq!(apply_filters(&rows, &f, today()).len(), 1);

        let f = Filters { period: Period::All, day: Some("2026-09-02".into()), ..Default::default() };
        assert_eq!(apply_filters(&rows, &f, today()).len(), 1);
    }

    #[test]
    fn groups_sorted_by_metric() {
        let rows = sample();
        let all: Vec<&Row> = rows.iter().collect();
        let g = group_by(&all, GroupKey::Model, Metric::Cost);
        assert_eq!(
            g.iter().map(|b| b.key.as_str()).collect::<Vec<_>>(),
            ["claude-opus-5", "claude-sonnet-5", "grok-9"]
        );
        assert_eq!(g[0].messages, 2);
        assert_eq!(g[0].projects, ["/Users/x/a", "/Users/x/b"]);
        assert_eq!(g[2].fmt_cost(), "—");
        let p = group_by(&all, GroupKey::Project, Metric::Tokens);
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn daily_series_is_continuous() {
        let rows = sample();
        let all: Vec<&Row> = rows.iter().collect();
        let d = by_day(&all, Metric::Cost, Some("2026-08-30"), today());
        assert_eq!(d.len(), 5);
        assert_eq!(d[0].total, 0.0);
        assert_eq!(d[4].date, "2026-09-03");
        assert!((d[4].total - 2.0).abs() < 1e-9);
        assert_eq!(d[4].by_model.len(), 2);

        let d = by_day(&all, Metric::Tokens, None, today());
        assert_eq!(d.len(), 3);
        assert_eq!(by_day(&[], Metric::Tokens, None, today()).len(), 0);
    }

    #[test]
    fn formatting() {
        assert_eq!(fmt_usd(1234.5), "US$ 1.234,50");
        assert_eq!(fmt_usd(0.0), "US$ 0,00");
        assert_eq!(fmt_tokens(999), "999");
        assert_eq!(fmt_tokens(1_500), "2 k");
        assert_eq!(fmt_tokens(12_345), "12 k");
        assert_eq!(fmt_tokens(1_234_567), "1,23 M");
        assert_eq!(fmt_tokens(2_000_000), "2 M");
        assert_eq!(fmt_tokens(1_500_000_000), "1,5 B");
        assert_eq!(short_model("claude-fable-5-1"), "fable 5.1");
        assert_eq!(short_model("claude-opus-5"), "opus 5");
        assert_eq!(short_model("claude-haiku-4-5-20251001"), "haiku 4.5");
        assert_eq!(short_model("gpt-5"), "gpt-5");
        assert_eq!(short_path("/Users/x/Developer/app"), "~/Developer/app");
        assert_eq!(short_path("/Users/x"), "~");
        assert_eq!(short_path("/tmp/x"), "/tmp/x");
        assert_eq!(project_name("/Users/x/Developer/app"), "app");
        assert_eq!(project_name("/Users/x"), "~");
        assert_eq!(fmt_date("2026-09-03"), "03/09/26");
        assert_eq!(fmt_date_short("2026-09-03"), "03/09");
    }
}
