//! Handy-Anzeige: kleiner Webserver im lokalen Netz, der die Live-Werte als
//! Seite zeigt (Browser auf dem Handy öffnen, gleiche WLAN).
//!
//! Nur lesend, keine Steuerung. Läuft in einem eigenen Thread, bis `stop`.
//! Die Adresse enthält einen zufälligen Schlüssel (`/<schlüssel>/`): wer ihn
//! nicht kennt (andere Geräte im WLAN), bekommt nur „404“.

use crate::tr;
use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::net::{TcpListener, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Ein angezeigter Wert.
#[derive(Debug, Clone, Default)]
pub struct Tile {
    pub name: String,
    pub unit: String,
    pub value: Option<f64>,
    /// "gut" | "hinweis" | "warnung" | ""
    pub state: String,
}

#[derive(Debug, Clone, Default)]
pub struct LiveData {
    pub title: String,
    pub tiles: Vec<Tile>,
    pub alarm: String,
}

pub type Shared = Arc<Mutex<LiveData>>;

pub struct Server {
    pub url: String,
    stop: Arc<AtomicBool>,
}

impl Server {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Eigene IP im lokalen Netz (ohne etwas zu senden).
pub fn local_ip() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("192.168.0.1:9")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn json(d: &LiveData) -> String {
    let tiles: Vec<String> = d
        .tiles
        .iter()
        .map(|t| {
            let v = t.value.map(|v| format!("{v:.1}")).unwrap_or_else(|| "null".into());
            format!("{{\"name\":\"{}\",\"unit\":\"{}\",\"value\":{v},\"state\":\"{}\"}}", esc(&t.name), esc(&t.unit), esc(&t.state))
        })
        .collect();
    format!("{{\"title\":\"{}\",\"alarm\":\"{}\",\"tiles\":[{}]}}", esc(&d.title), esc(&d.alarm), tiles.join(","))
}

const PAGE: &str = r#"<!doctype html><html lang="de"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>ECUWerkbank live</title>
<style>
body{margin:0;font-family:system-ui,sans-serif;background:#111;color:#eee}
h1{font-size:16px;font-weight:600;margin:12px 16px;color:#9cf}
#alarm{display:none;margin:0 16px 8px;padding:10px;border-radius:8px;background:#b22;font-weight:700}
#g{display:grid;grid-template-columns:repeat(auto-fill,minmax(150px,1fr));gap:10px;padding:0 16px 16px}
.t{background:#1d1f24;border-radius:12px;padding:12px;border-left:6px solid #444}
.t.gut{border-color:#2a8a45}.t.hinweis{border-color:#d98c0d}.t.warnung{border-color:#c0271f;background:#2a1414}
.n{font-size:13px;color:#aaa}.v{font-size:34px;font-weight:700;margin-top:4px}.u{font-size:14px;color:#aaa;margin-left:4px}
#s{font-size:11px;color:#666;margin:0 16px 16px}
</style></head><body><h1 id="h">ECUWerkbank</h1><div id="alarm"></div><div id="g"></div><div id="s">verbinde…</div>
<script>
async function tick(){try{const r=await fetch('api',{cache:'no-store'});const d=await r.json();
document.getElementById('h').textContent=d.title||'ecu-werkbank';
const a=document.getElementById('alarm');a.style.display=d.alarm?'block':'none';a.textContent=d.alarm;
document.getElementById('g').innerHTML=d.tiles.map(t=>`<div class="t ${t.state}"><div class="n">${t.name}</div><div class="v">${t.value===null?'—':t.value.toLocaleString('de-DE')}<span class="u">${t.unit}</span></div></div>`).join('');
document.getElementById('s').textContent='aktualisiert '+new Date().toLocaleTimeString('de-DE');}catch(e){document.getElementById('s').textContent='keine Verbindung';}}
setInterval(tick,500);tick();
</script></body></html>"#;

/// Server auf `port` starten (0 = beliebig). Liefert die Adresse fürs Handy.
pub fn serve(port: u16, data: Shared) -> Result<Server> {
    let listener = TcpListener::bind(("0.0.0.0", port)).with_context(|| tr!("Port {port} belegt?", port = port))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let schluessel = zufall_schluessel();
    let pfad = format!("/{schluessel}/");
    std::thread::spawn(move || {
        while !s2.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut sock, _)) => {
                    let _ = sock.set_nonblocking(false);
                    let _ = sock.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                    // Anfrage bis zum Kopfende lesen (kann in mehreren Stücken kommen)
                    let mut buf = Vec::with_capacity(1024);
                    let mut chunk = [0u8; 1024];
                    while buf.len() < 8192 && !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match sock.read(&mut chunk) {
                            Ok(0) => break,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                            Err(_) => break,
                        }
                    }
                    let req = String::from_utf8_lossy(&buf);
                    let path = req.split_whitespace().nth(1).unwrap_or("/");
                    let (status, ctype, body) = match path.strip_prefix(pfad.as_str()) {
                        Some(r) if r.starts_with("api") => ("200 OK", "application/json", json(&data.lock().unwrap_or_else(|e| e.into_inner()))),
                        Some(_) => ("200 OK", "text/html; charset=utf-8", PAGE.to_string()),
                        None => ("404 Not Found", "text/plain", "404".to_string()),
                    };
                    let _ = write!(
                        sock,
                        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(30)),
            }
        }
    });
    Ok(Server { url: crate::platzhalter!("http://{a}:{port}/{s}/", a = local_ip(), port = port, s = schluessel), stop })
}

/// Zufälliger Schlüssel (16 Hex-Zeichen) aus dem Zufall des Betriebssystems
/// (über die zufällig gesetzten Startwerte von `RandomState`).
fn zufall_schluessel() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    format!("{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_page_and_json() {
        let data: Shared = Arc::new(Mutex::new(LiveData {
            title: "B XY 123".into(),
            tiles: vec![Tile { name: "Drehzahl".into(), unit: "1/min".into(), value: Some(812.5), state: "gut".into() }],
            alarm: String::new(),
        }));
        let srv = serve(0, data).unwrap();
        let hinten = srv.url.rsplit(':').next().unwrap();
        let (port, schluessel) = hinten.trim_end_matches('/').split_once('/').unwrap();
        let port: u16 = port.parse().unwrap();
        let get = |p: &str| {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(s, "GET {p} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };
        assert!(get("/").contains("404"), "ohne Schlüssel nichts");
        assert!(get("/api").contains("404"));
        assert!(get(&format!("/{schluessel}/")).contains("ECUWerkbank live"));
        let j = get(&format!("/{schluessel}/api"));
        assert!(j.contains("\"value\":812.5") && j.contains("\"title\":\"B XY 123\""), "{j}");
        srv.stop();
    }
}
