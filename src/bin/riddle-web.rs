//! Local-first browser edition for HarmonyOS/Android tablets running Termux.

use riddle::memory::{MemoryStore, Strokes};
use riddle::oracle::{Event, Oracle, TurnContext};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const INDEX: &[u8] = include_bytes!("../../web/index.html");
const APP_JS: &[u8] = include_bytes!("../../web/app.js");
const STYLE: &[u8] = include_bytes!("../../web/style.css");
const FONT: &[u8] = include_bytes!("../../fonts/DancingScript.ttf");

struct Job {
    events: Vec<(String, String)>,
    created: Instant,
}

struct AppState {
    memory: Mutex<Option<MemoryStore>>,
    jobs: Mutex<HashMap<String, Job>>,
}

fn main() {
    load_env();
    if std::env::var_os("RIDDLE_MEMORY_DIR").is_none() {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        std::env::set_var(
            "RIDDLE_MEMORY_DIR",
            format!("{home}/.local/share/riddle/memories"),
        );
    }
    let bind = std::env::var("RIDDLE_WEB_BIND").unwrap_or_else(|_| "127.0.0.1:9314".into());
    let state = Arc::new(AppState {
        memory: Mutex::new(MemoryStore::open()),
        jobs: Mutex::new(HashMap::new()),
    });
    let listener = TcpListener::bind(&bind).unwrap_or_else(|e| {
        eprintln!("riddle-web: cannot listen on {bind}: {e}");
        std::process::exit(1);
    });
    println!("The diary is waiting at http://{bind}");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    if let Err(e) = handle(stream, state) {
                        if e.kind() != std::io::ErrorKind::BrokenPipe {
                            eprintln!("riddle-web: {e}");
                        }
                    }
                });
            }
            Err(e) => eprintln!("riddle-web: connection failed: {e}"),
        }
    }
}

fn handle(mut stream: TcpStream, state: Arc<AppState>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(15)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    let path = target.split('?').next().unwrap_or("/");
    let mut content_len = 0usize;
    let mut setup_header = false;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_len = v.trim().parse().unwrap_or(0);
        }
        if line.to_ascii_lowercase().starts_with("x-riddle-setup: 1") {
            setup_header = true;
        }
    }
    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => {
            asset(&mut stream, "text/html; charset=utf-8", INDEX)
        }
        ("GET", "/app.js") => asset(&mut stream, "text/javascript; charset=utf-8", APP_JS),
        ("GET", "/style.css") => asset(&mut stream, "text/css; charset=utf-8", STYLE),
        ("GET", "/DancingScript.ttf") => asset(&mut stream, "font/ttf", FONT),
        ("GET", "/api/health") => asset(&mut stream, "application/json", br#"{"ok":true}"#),
        ("GET", "/api/config") => {
            let ready = std::env::var("RIDDLE_OPENAI_KEY")
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false);
            asset(
                &mut stream,
                "application/json",
                if ready {
                    br#"{"ready":true}"#
                } else {
                    br#"{"ready":false}"#
                },
            )
        }
        ("GET", p) if p.starts_with("/api/job/") => {
            poll_job(&mut stream, p.trim_start_matches("/api/job/"), &state)
        }
        ("POST", "/api/config") if setup_header && content_len > 0 && content_len <= 1024 => {
            let mut body = vec![0u8; content_len];
            reader.read_exact(&mut body)?;
            save_config(&mut stream, &body)
        }
        ("POST", "/api/ask") if content_len > 0 && content_len <= 8 * 1024 * 1024 => {
            let mut png = vec![0u8; content_len];
            reader.read_exact(&mut png)?;
            start_job(&mut stream, png, state)
        }
        _ => response(&mut stream, "404 Not Found", "text/plain", b"Not found"),
    }
}

fn save_config(stream: &mut TcpStream, body: &[u8]) -> std::io::Result<()> {
    let key = String::from_utf8_lossy(body).trim().to_string();
    if key.len() < 12 || key.contains(['\r', '\n', '\0']) {
        return response(
            stream,
            "400 Bad Request",
            "application/json",
            br#"{"ok":false,"error":"invalid API key"}"#,
        );
    }
    let path = saved_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let contents = format!(
        "# Saved by riddle-web first-run setup.\nRIDDLE_OPENAI_KEY={key}\nRIDDLE_OPENAI_BASE=https://api.moonshot.cn/v1\nRIDDLE_OPENAI_MODEL=kimi-k2.6\nRIDDLE_OPENAI_MAX_TOKENS=800\nRIDDLE_OPENAI_THINKING=disabled\n"
    );
    std::fs::write(&path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    std::env::set_var("RIDDLE_OPENAI_KEY", key);
    std::env::set_var("RIDDLE_OPENAI_BASE", "https://api.moonshot.cn/v1");
    std::env::set_var("RIDDLE_OPENAI_MODEL", "kimi-k2.6");
    std::env::set_var("RIDDLE_OPENAI_MAX_TOKENS", "800");
    std::env::set_var("RIDDLE_OPENAI_THINKING", "disabled");
    response(stream, "200 OK", "application/json", br#"{"ok":true}"#)
}

fn asset(stream: &mut TcpStream, content_type: &str, body: &[u8]) -> std::io::Result<()> {
    response(stream, "200 OK", content_type, body)
}

fn response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len())?;
    stream.write_all(body)
}

fn start_job(stream: &mut TcpStream, png: Vec<u8>, state: Arc<AppState>) -> std::io::Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let job_id = format!("{}-{}", now.as_millis(), std::process::id());
    if let Ok(mut jobs) = state.jobs.lock() {
        jobs.retain(|_, job| job.created.elapsed() < Duration::from_secs(3600));
        jobs.insert(
            job_id.clone(),
            Job {
                events: Vec::new(),
                created: Instant::now(),
            },
        );
    }
    eprintln!("riddle-web: job {job_id} page={} bytes", png.len());
    let worker_id = job_id.clone();
    std::thread::spawn(move || run_job(worker_id, png, state));
    response(
        stream,
        "202 Accepted",
        "text/plain; charset=utf-8",
        job_id.as_bytes(),
    )
}

fn poll_job(stream: &mut TcpStream, job_id: &str, state: &Arc<AppState>) -> std::io::Result<()> {
    let jobs = state
        .jobs
        .lock()
        .map_err(|_| std::io::Error::other("job lock poisoned"))?;
    let Some(job) = jobs.get(job_id) else {
        return response(stream, "404 Not Found", "text/plain", b"job not found");
    };
    let mut body = String::new();
    for (kind, text) in &job.events {
        body.push_str(&format!(
            "{{\"type\":{},\"text\":{}}}\n",
            quote(kind),
            quote(text)
        ));
    }
    response(
        stream,
        "200 OK",
        "application/x-ndjson; charset=utf-8",
        body.as_bytes(),
    )
}

fn push_job(state: &Arc<AppState>, job_id: &str, kind: &str, text: &str) {
    if let Ok(mut jobs) = state.jobs.lock() {
        if let Some(job) = jobs.get_mut(job_id) {
            job.events.push((kind.to_string(), text.to_string()));
        }
    }
}

fn run_job(job_id: String, png: Vec<u8>, state: Arc<AppState>) {
    if std::env::var("RIDDLE_MOCK").as_deref() == Ok("1") {
        let _ = png;
        std::thread::sleep(Duration::from_millis(700));
        push_job(
            &state,
            &job_id,
            "ink",
            "Ah… so the diary has found a new writer.",
        );
        std::thread::sleep(Duration::from_millis(500));
        push_job(
            &state,
            &job_id,
            "ink",
            "Tell me, what secret brought you to these pages?",
        );
        push_job(&state, &job_id, "done", "");
        return;
    }
    let remember = state.memory.lock().map(|m| m.is_some()).unwrap_or(false);
    let oracle = match Oracle::spawn(remember) {
        Ok(o) => o,
        Err(e) => {
            push_job(
                &state,
                &job_id,
                "error",
                &format!("The diary remains silent: {e}"),
            );
            push_job(&state, &job_id, "done", "");
            return;
        }
    };
    let ctx = build_ctx(&state);
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut path = std::env::temp_dir();
    path.push(format!("riddle-web-{id}-{}.png", std::process::id()));
    if let Err(e) = std::fs::write(&path, png) {
        push_job(
            &state,
            &job_id,
            "error",
            &format!("The page could not be read: {e}"),
        );
        push_job(&state, &job_id, "done", "");
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    oracle.ask(path.to_string_lossy().as_ref(), &ctx, tx);
    let _ = std::fs::remove_file(&path);

    let mut reply = String::new();
    let mut transcript = String::new();
    let mut failed = false;
    for item in rx {
        match item {
            Ok(Event::Ink(text)) => {
                if !reply.is_empty() {
                    reply.push(' ');
                }
                reply.push_str(&text);
                push_job(&state, &job_id, "ink", &text);
            }
            Ok(Event::Transcript(text)) => transcript = text,
            Ok(Event::Show(id)) => {
                let recalled = state
                    .memory
                    .lock()
                    .ok()
                    .and_then(|m| m.as_ref().and_then(|s| s.get(id)).cloned());
                if let Some(old) = recalled {
                    let text = format!(
                        "You wrote: {}\n\nAnd I answered: {}",
                        old.transcript, old.reply
                    );
                    reply.push_str(&text);
                    push_job(&state, &job_id, "ink", &text);
                }
            }
            Err(e) => {
                failed = true;
                push_job(
                    &state,
                    &job_id,
                    "error",
                    &format!("The diary's ink has gone cold: {e}"),
                );
            }
        }
    }
    if !failed && !reply.trim().is_empty() {
        if let Ok(mut guard) = state.memory.lock() {
            if let Some(store) = guard.as_mut() {
                store.append(id, &transcript, reply.trim(), &Strokes::new());
            }
        }
    }
    push_job(&state, &job_id, "done", "");
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn build_ctx(state: &Arc<AppState>) -> TurnContext {
    let Ok(guard) = state.memory.lock() else {
        return TurnContext::default();
    };
    let Some(store) = guard.as_ref() else {
        return TurnContext::default();
    };
    let turns = std::env::var("RIDDLE_MEMORY_TURNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6);
    let (catalog_lines, catalog_ids) = store.catalog(40);
    TurnContext {
        history: store.recent_dialogue(turns),
        catalog_lines,
        catalog_ids,
    }
}

fn load_env() {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("riddle-web"));
    let candidates = [
        exe.parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("oracle.env"),
        PathBuf::from("oracle.env"),
        saved_config_path(),
    ];
    for path in candidates {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for raw in text.lines() {
            let line = raw.trim().strip_prefix("export ").unwrap_or(raw.trim());
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                if std::env::var_os(key.trim()).is_none() {
                    std::env::set_var(key.trim(), value.trim().trim_matches(['\'', '"']));
                }
            }
        }
        break;
    }
}

fn saved_config_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config/riddle/oracle.env")
}
