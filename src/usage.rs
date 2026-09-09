//! Leitura do uso de tokens do Claude Code a partir dos transcritos em
//! `~/.claude/projects/<slug>/<sessão>.jsonl`.
//!
//! Cada linha é um evento JSON. As linhas com `type: "assistant"` trazem
//! `message.usage` com os tokens da resposta, o modelo, o `cwd` da sessão e o
//! `sessionId`. Uma mesma mensagem aparece em várias linhas (uma por bloco de
//! conteúdo, ver `apiBlockIndex`), então o `message.id` é usado para deduplicar.
//!
//! O custo é estimado com a tabela de preços da API da Anthropic; assinaturas
//! (Pro/Max) não são cobradas por token, então o valor serve de referência.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq)]
pub struct UsageEntry {
    /// ISO-8601 em UTC, como gravado no transcrito.
    pub timestamp: String,
    pub model: String,
    pub slug: String,
    /// Diretório de trabalho da sessão (`cwd`), ou o caminho aproximado do slug.
    pub project_path: String,
    pub session_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_read_tokens: u64,
    /// Custo estimado em dólares; `None` quando o modelo não tem preço na tabela.
    pub cost_usd: Option<f64>,
}

impl UsageEntry {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_write_tokens + self.cache_read_tokens
    }
}

/// Preço por milhão de tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pricing {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
}

/// Tabela de preços (US$/MTok). Cache write = 1,25× input; cache read = 0,1×
/// input, exceto onde a documentação diz outra coisa.
pub fn pricing_for(model: &str) -> Option<Pricing> {
    let m = model.to_lowercase();
    let p = |input: f64, output: f64| Pricing { input, output, cache_write: input * 1.25, cache_read: input * 0.1 };
    if m.contains("fable-5-1") || m.contains("mythos-5-1") {
        return Some(Pricing { cache_read: 0.25, ..p(10.0, 50.0) });
    }
    if m.contains("fable-5") || m.contains("mythos-5") {
        return Some(p(10.0, 50.0));
    }
    if m.contains("opus-5") || m.contains("opus-4-8") || m.contains("opus-4-7") || m.contains("opus-4-6") {
        return Some(p(5.0, 25.0));
    }
    if m.contains("opus-4-5") || m.contains("opus-4-1") || m.contains("opus-4") {
        return Some(p(15.0, 75.0));
    }
    if m.contains("sonnet-5") {
        return Some(p(2.0, 10.0));
    }
    if m.contains("sonnet-4") || m.contains("sonnet-3") {
        return Some(p(3.0, 15.0));
    }
    if m.contains("haiku-4") {
        return Some(p(1.0, 5.0));
    }
    if m.contains("haiku-3") {
        return Some(p(0.8, 4.0));
    }
    None
}

fn cost(model: &str, input: u64, output: u64, cache_write: u64, cache_read: u64) -> Option<f64> {
    let p = pricing_for(model)?;
    let per = |tokens: u64, price: f64| tokens as f64 * price / 1_000_000.0;
    Some(per(input, p.input) + per(output, p.output) + per(cache_write, p.cache_write) + per(cache_read, p.cache_read))
}

/// Interpreta uma linha do transcrito. Retorna `None` para linhas que não são
/// respostas do assistente com `usage`, e o id da mensagem para deduplicação.
fn parse_line(line: &str, slug: &str, guessed_path: &str) -> Option<(String, UsageEntry)> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let message = v.get("message")?;
    let usage = message.get("usage")?;
    let model = message.get("model")?.as_str()?.to_string();
    if model.starts_with('<') {
        // "<synthetic>": mensagens geradas localmente, sem custo.
        return None;
    }
    let u = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    let input_tokens = u("input_tokens");
    let output_tokens = u("output_tokens");
    let cache_write_tokens = u("cache_creation_input_tokens");
    let cache_read_tokens = u("cache_read_input_tokens");
    if input_tokens + output_tokens + cache_write_tokens + cache_read_tokens == 0 {
        return None;
    }

    let str_field = |obj: &serde_json::Value, k: &str| obj.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let id = str_field(message, "id").or_else(|| str_field(&v, "requestId")).or_else(|| str_field(&v, "uuid"))?;
    let session_id = str_field(&v, "sessionId").or_else(|| str_field(&v, "session_id")).unwrap_or_default();
    let project_path = str_field(&v, "cwd").unwrap_or_else(|| guessed_path.to_string());

    Some((
        id,
        UsageEntry {
            timestamp: str_field(&v, "timestamp").unwrap_or_default(),
            model: model.clone(),
            slug: slug.to_string(),
            project_path,
            session_id,
            input_tokens,
            output_tokens,
            cache_write_tokens,
            cache_read_tokens,
            cost_usd: cost(&model, input_tokens, output_tokens, cache_write_tokens, cache_read_tokens),
        },
    ))
}

/// Entradas de um transcrito, com o id de cada mensagem para deduplicar
/// entre arquivos (a mesma mensagem pode aparecer em sessões retomadas).
fn read_transcript(path: &Path, slug: &str, guessed_path: &str) -> Vec<(String, UsageEntry)> {
    let mut out = Vec::new();
    let Ok(file) = fs::File::open(path) else { return out };
    let reader = BufReader::new(file);
    let file_session = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut seen = HashSet::new();
    for line in reader.lines().map_while(Result::ok) {
        // Filtro barato antes do parse completo: só respostas do assistente com usage.
        if !line.contains("\"usage\"") || !line.contains("\"assistant\"") {
            continue;
        }
        if let Some((id, mut entry)) = parse_line(&line, slug, guessed_path) {
            if !seen.insert(id.clone()) {
                continue;
            }
            if entry.session_id.is_empty() {
                entry.session_id = file_session.clone();
            }
            out.push((id, entry));
        }
    }
    out
}

/// Assinatura de um arquivo para invalidar o cache: mudou de tamanho ou de mtime.
type FileStamp = (Option<SystemTime>, u64);

struct CachedFile {
    stamp: FileStamp,
    entries: Vec<(String, UsageEntry)>,
}

/// Cache por arquivo. Transcritos antigos nunca mudam, então só os arquivos
/// novos ou modificados desde a última leitura são reinterpretados.
fn cache() -> &'static Mutex<HashMap<PathBuf, CachedFile>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedFile>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn stamp_of(path: &Path) -> Option<FileStamp> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok(), meta.len()))
}

#[derive(Clone)]
struct Transcript {
    path: PathBuf,
    slug: String,
    guessed_path: String,
    stamp: FileStamp,
}

fn list_transcripts(claude_home: &Path) -> Vec<Transcript> {
    let mut out = Vec::new();
    let Ok(dirs) = fs::read_dir(claude_home.join("projects")) else { return out };
    for dir in dirs.flatten() {
        let dir_path = dir.path();
        if !dir_path.is_dir() {
            continue;
        }
        let slug = dir.file_name().to_string_lossy().into_owned();
        let guessed_path = format!("/{}", slug.trim_start_matches('-').replace('-', "/"));
        let Ok(files) = fs::read_dir(&dir_path) else { continue };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(stamp) = stamp_of(&path) else { continue };
            out.push(Transcript { path, slug: slug.clone(), guessed_path: guessed_path.clone(), stamp });
        }
    }
    out
}

/// Interpreta vários transcritos em paralelo.
fn read_transcripts(files: &[Transcript]) -> Vec<(Transcript, Vec<(String, UsageEntry)>)> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(files.len().max(1));
    let chunk = files.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = files
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|t| (t.clone(), read_transcript(&t.path, &t.slug, &t.guessed_path)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    })
}

/// Lê todos os transcritos em `<claude_home>/projects`, usando o cache para
/// pular arquivos que não mudaram.
pub fn collect_usage(claude_home: &Path) -> Vec<UsageEntry> {
    let files = list_transcripts(claude_home);
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());

    let stale: Vec<Transcript> =
        files.iter().filter(|t| cache.get(&t.path).map(|c| c.stamp != t.stamp).unwrap_or(true)).cloned().collect();
    for (t, entries) in read_transcripts(&stale) {
        cache.insert(t.path, CachedFile { stamp: t.stamp, entries });
    }
    // Arquivos apagados saem do cache.
    let present: HashSet<&PathBuf> = files.iter().map(|t| &t.path).collect();
    cache.retain(|p, _| present.contains(p));

    let mut seen = HashSet::new();
    let mut out: Vec<UsageEntry> = Vec::new();
    for t in &files {
        if let Some(c) = cache.get(&t.path) {
            for (id, entry) in &c.entries {
                if seen.insert(id.as_str()) {
                    out.push(entry.clone());
                }
            }
        }
    }
    out.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    out
}

/// `~/.claude`, ou `$CLAUDE_HOME` quando definido.
pub fn claude_home() -> Result<PathBuf, String> {
    if let Some(h) = std::env::var_os("CLAUDE_HOME") {
        return Ok(PathBuf::from(h));
    }
    dirs::home_dir().map(|h| h.join(".claude")).ok_or_else(|| "não foi possível localizar o diretório home".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, model: &str, output: u64) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"2026-09-03T10:33:57.565Z","cwd":"/Users/x/app","sessionId":"s1","requestId":"req_1","message":{{"id":"{id}","model":"{model}","usage":{{"input_tokens":2,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":{output}}}}}}}"#
        )
    }

    #[test]
    fn parses_assistant_line_with_cost() {
        let (id, e) = parse_line(&line("msg_1", "claude-opus-5", 50), "-Users-x-app", "/Users/x/app").unwrap();
        assert_eq!(id, "msg_1");
        assert_eq!(e.model, "claude-opus-5");
        assert_eq!(e.project_path, "/Users/x/app");
        assert_eq!(e.session_id, "s1");
        assert_eq!((e.input_tokens, e.output_tokens, e.cache_write_tokens, e.cache_read_tokens), (2, 50, 100, 1000));
        // 2*5 + 50*25 + 100*6.25 + 1000*0.5 = 2385 / 1e6
        let c = e.cost_usd.unwrap();
        assert!((c - 0.002385).abs() < 1e-9, "{c}");
    }

    #[test]
    fn ignores_non_assistant_and_synthetic() {
        assert!(parse_line(r#"{"type":"user","message":{"usage":{}}}"#, "s", "/p").is_none());
        assert!(parse_line(&line("m", "<synthetic>", 1), "s", "/p").is_none());
        assert!(parse_line("não é json", "s", "/p").is_none());
    }

    #[test]
    fn unknown_model_has_no_cost() {
        let (_, e) = parse_line(&line("m", "grok-9", 1), "s", "/p").unwrap();
        assert_eq!(e.cost_usd, None);
    }

    #[test]
    fn dedupes_by_message_id_across_blocks() {
        let dir = std::env::temp_dir().join(format!("usage-test-{}", std::process::id()));
        let proj = dir.join("projects").join("-Users-x-app");
        fs::create_dir_all(&proj).unwrap();
        let content = [
            line("msg_a", "claude-sonnet-5", 10),
            line("msg_a", "claude-sonnet-5", 10),
            line("msg_b", "claude-sonnet-5", 20),
        ]
        .join("\n");
        fs::write(proj.join("sess.jsonl"), content).unwrap();

        let entries = collect_usage(&dir);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].slug, "-Users-x-app");
        assert_eq!(entries.iter().map(|e| e.output_tokens).sum::<u64>(), 30);
    }

    #[test]
    fn pricing_table() {
        assert_eq!(pricing_for("claude-fable-5-1").unwrap().cache_read, 0.25);
        assert_eq!(pricing_for("claude-opus-4-8").unwrap().input, 5.0);
        assert_eq!(pricing_for("claude-haiku-4-5-20251001").unwrap().output, 5.0);
        assert!(pricing_for("gpt-5.6-sol").is_none());
    }
}
