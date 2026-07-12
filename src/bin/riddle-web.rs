//! Local-first browser edition for HarmonyOS/Android tablets running Termux.

use riddle::memory::{MemoryStore, Strokes};
use riddle::oracle::{Event, Oracle, TurnContext};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const INDEX: &[u8] = include_bytes!("../../web/index.html");
const APP_JS: &[u8] = include_bytes!("../../web/app.js");
const STYLE: &[u8] = include_bytes!("../../web/style.css");
const FONT: &[u8] = include_bytes!("../../fonts/DancingScript.ttf");

fn main() {
    load_env();
    if std::env::var_os("RIDDLE_MEMORY_DIR").is_none() {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        std::env::set_var(
            "RIDDLE_MEMORY_DIR",
            format!("{home}/.local/share/riddle/memories"),
        );
    }
    let bind = std::env::var("RIDDLE_WEB_BIND").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let memory = Arc::new(Mutex::new(MemoryStore::open()));
    let listener = TcpListener::bind(&bind).unwrap_or_else(|e| {
        eprintln!("riddle-web: cannot listen on {bind}: {e}");
        std::process::exit(1);
    });
    println!("The diary is waiting at http://{bind}");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let memory = Arc::clone(&memory);
                std::thread::spawn(move || {
                    if let Err(e) = handle(stream, memory) {
                        eprintln!("riddle-web: {e}");
                    }
                });
            }
            Err(e) => eprintln!("riddle-web: connection failed: {e}"),
        }
    }
}

fn handle(mut stream: TcpStream, memory: Arc<Mutex<Option<MemoryStore>>>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(15)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/").split('?').next().unwrap_or("/");
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
        ("POST", "/api/config") if setup_header && content_len > 0 && content_len <= 1024 => {
            let mut body = vec![0u8; content_len];
            reader.read_exact(&mut body)?;
            save_config(&mut stream, &body)
        }
        ("POST", "/api/ask") if content_len > 0 && content_len <= 8 * 1024 * 1024 => {
            let mut png = vec![0u8; content_len];
            reader.read_exact(&mut png)?;
            ask(stream, png, memory)
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
        "# Saved by riddle-web first-run setup.\nRIDDLE_OPENAI_KEY={key}\nRIDDLE_OPENAI_BASE=https://api.moonshot.cn/v1\nRIDDLE_OPENAI_MODEL=kimi-k2.6\nRIDDLE_OPENAI_MAX_TOKENS=800\n"
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

fn ask(
    mut stream: TcpStream,
    png: Vec<u8>,
    memory: Arc<Mutex<Option<MemoryStore>>>,
) -> std::io::Result<()> {
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson; charset=utf-8\r\nCache-Control: no-store, no-transform\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n")?;
    stream.flush()?;

    let remember = memory.lock().map(|m| m.is_some()).unwrap_or(false);
    let oracle = match Oracle::spawn(remember) {
        Ok(o) => o,
        Err(e) => {
            return event(
                &mut stream,
                "error",
                &format!("The diary remains silent: {e}"),
            )
        }
    };
    let ctx = build_ctx(&memory);
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut path = std::env::temp_dir();
    path.push(format!("riddle-web-{id}-{}.png", std::process::id()));
    std::fs::write(&path, png)?;
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
                event(&mut stream, "ink", &text)?;
            }
            Ok(Event::Transcript(text)) => transcript = text,
            Ok(Event::Show(id)) => {
                let recalled = memory
                    .lock()
                    .ok()
                    .and_then(|m| m.as_ref().and_then(|s| s.get(id)).cloned());
                if let Some(old) = recalled {
                    let text = format!(
                        "You wrote: {}\n\nAnd I answered: {}",
                        old.transcript, old.reply
                    );
                    reply.push_str(&text);
                    event(&mut stream, "ink", &text)?;
                }
            }
            Err(e) => {
                failed = true;
                event(
                    &mut stream,
                    "error",
                    &format!("The diary's ink has gone cold: {e}"),
                )?;
            }
        }
    }
    if !failed && !reply.trim().is_empty() {
        if let Ok(mut guard) = memory.lock() {
            if let Some(store) = guard.as_mut() {
                store.append(id, &transcript, reply.trim(), &Strokes::new());
            }
        }
    }
    event(&mut stream, "done", "")
}

fn event(stream: &mut TcpStream, kind: &str, text: &str) -> std::io::Result<()> {
    writeln!(
        stream,
        "{{\"type\":{},\"text\":{}}}",
        quote(kind),
        quote(text)
    )?;
    stream.flush()
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

fn build_ctx(memory: &Arc<Mutex<Option<MemoryStore>>>) -> TurnContext {
    let Ok(guard) = memory.lock() else {
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
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config/riddle/oracle.env")
}
