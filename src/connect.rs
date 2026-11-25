//! Verbindungs-Assistent: prüft Adapter, Interface, Bitrate, Rechte und ob
//! Steuergeräte antworten — mit Klartext-Hinweisen zur Abhilfe.

use crate::tr;
use crate::limits::Stufe;
use crate::transport::Backend;
use std::path::Path;

fn read(p: &str) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

/// Alle CAN-Interfaces des Systems (Name, aktiv?, Bitrate falls bekannt).
pub fn can_interfaces() -> Vec<(String, bool, Option<u32>)> {
    let mut v = Vec::new();
    for e in std::fs::read_dir("/sys/class/net").into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if read(&crate::platzhalter!("/sys/class/net/{name}/type", name = name)).as_deref() != Some("280") {
            continue; // 280 = ARPHRD_CAN
        }
        let up = read(&crate::platzhalter!("/sys/class/net/{name}/flags", name = name))
            .and_then(|f| u32::from_str_radix(f.trim_start_matches("0x"), 16).ok())
            .is_some_and(|f| f & 1 != 0);
        v.push((name.clone(), up, bitrate(&name)));
    }
    v.sort();
    v
}

/// Bitrate über `ip -details link show` (ohne Root lesbar).
pub fn bitrate(iface: &str) -> Option<u32> {
    let out = std::process::Command::new("ip").args(["-details", "link", "show", iface]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut it = text.split_whitespace();
    while let Some(t) = it.next() {
        if t == "bitrate" {
            return it.next()?.parse().ok();
        }
    }
    None
}

/// CAN-Zustand des Controllers, z. B. "ERROR-ACTIVE", "ERROR-PASSIVE", "BUS-OFF".
pub fn can_state(iface: &str) -> Option<String> {
    let out = std::process::Command::new("ip").args(["-details", "link", "show", iface]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut it = text.split_whitespace();
    while let Some(t) = it.next() {
        if t == "state" {
            let v = it.next()?;
            // erstes „state“ ist der Link-Zustand (UP/DOWN), das CAN-„state“ folgt
            if v.contains('-') || v == "STOPPED" || v == "SLEEPING" {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Hinweis zum CAN-Zustand, wenn nichts antwortet.
pub fn state_hint(iface: &str, state: &str) -> Option<(Stufe, String)> {
    let reset = crate::platzhalter!("sudo ip link set {iface} down && sudo ip link set {iface} up", iface = iface);
    match state {
        "BUS-OFF" => Some((
            Stufe::Warnung,
            tr!("Der Adapter hat sich abgeschaltet (Bus-Off): seine Nachrichten werden auf der Leitung verfälscht. Meist sind CAN-High und CAN-Low vertauscht (OBD: Pin 6 = High, Pin 14 = Low, Pin 5 = Masse) oder die Bitrate passt nicht. Nach dem Prüfen wieder einschalten: {reset}", reset = reset),
        )),
        "ERROR-PASSIVE" | "ERROR-WARNING" => Some((
            Stufe::Warnung,
            tr!("Niemand bestätigt die Nachrichten des Adapters: CAN-Leitungen nicht verbunden, Stecker nicht richtig in der OBD-Buchse oder Zündung aus.").to_string(),
        )),
        "ERROR-ACTIVE" => Some((
            Stufe::Hinweis,
            tr!("Die Leitung ist in Ordnung, aber kein Steuergerät antwortet: Zündung an? Nach längerer Standzeit schläft das Auto — Zündung kurz aus und wieder an.").to_string(),
        )),
        _ => None,
    }
}

/// STM32 im Update-Modus (USB 0483:df11)? Liefert die Seriennummer.
pub fn dfu_device() -> Option<String> {
    let rd = std::fs::read_dir("/sys/bus/usb/devices").ok()?;
    for e in rd.flatten() {
        let p = e.path();
        let r = |f: &str| std::fs::read_to_string(p.join(f)).ok().map(|s| s.trim().to_string());
        if r("idVendor").as_deref() == Some("0483") && r("idProduct").as_deref() == Some("df11") {
            return Some(r("serial").unwrap_or_default());
        }
    }
    None
}

/// Steht der Adapter im Prüfmodus (`loopback on`)? Dann hört er seine
/// eigenen Nachrichten — ohne Auto testbar.
pub fn hw_loopback(iface: &str) -> bool {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, bool>>> = std::sync::OnceLock::new();
    let m = CACHE.get_or_init(Default::default);
    if let Some(v) = m.lock().unwrap().get(iface) {
        return *v;
    }
    let v = std::process::Command::new("ip")
        .args(["-details", "link", "show", iface])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim_start().starts_with("can <") && l.contains("LOOPBACK")))
        .unwrap_or(false);
    m.lock().unwrap().insert(iface.to_string(), v);
    v
}

/// Im Prüfmodus liefert der Adapter jede Nachricht zweimal (Hardware-Echo +
/// Linux-Echo). Linux-Echo für diesen Socket abschalten, dann kommt sie einmal.
#[cfg(not(target_os = "linux"))]
pub fn fix_hw_loopback(_iface: &str, _sock: &crate::can::CanSocket) {}

#[cfg(target_os = "linux")]
pub fn fix_hw_loopback(iface: &str, sock: &crate::can::CanSocket) {
    if !hw_loopback(iface) {
        return;
    }
    use std::os::fd::AsRawFd;
    let off: libc::c_int = 0;
    // SOL_CAN_RAW = 101, CAN_RAW_LOOPBACK = 3
    unsafe {
        libc::setsockopt(sock.as_raw_fd(), 101, 3, &off as *const _ as *const libc::c_void, std::mem::size_of::<libc::c_int>() as libc::socklen_t);
    }
}

/// Prüfen und Hinweise liefern.
pub fn check(be: &Backend) -> Vec<(Stufe, String)> {
    let mut out = Vec::new();
    match be {
        Backend::Demo => out.push((Stufe::Gut, tr!("Demo-Fahrzeug — keine Hardware nötig."))),
        Backend::Doip { addr } => {
            let a = if addr.contains(':') { addr.clone() } else { format!("{addr}:{}", crate::doip::PORT) };
            match a.parse::<std::net::SocketAddr>().ok().map(|x| std::net::TcpStream::connect_timeout(&x, std::time::Duration::from_secs(2))) {
                Some(Ok(_)) => out.push((Stufe::Gut, tr!("DoIP-Gateway {a} erreichbar", a = a))),
                Some(Err(e)) => {
                    out.push((Stufe::Warnung, tr!("DoIP-Gateway {a} nicht erreichbar ({e}) — Ethernet-Kabel gesteckt, Zündung an, Rechner im selben Netz (oft 169.254.x.x)?", a = a, e = e)));
                    return out;
                }
                None => {
                    out.push((Stufe::Warnung, tr!("Adresse '{a}' ungültig (Format 169.254.1.10 oder 169.254.1.10:13400)", a = addr)));
                    return out;
                }
            }
        }
        Backend::SocketCan { iface } => {
            let all = can_interfaces();
            let virt = read(&crate::platzhalter!("/sys/class/net/{iface}/type", iface = iface)).is_some() && iface.starts_with("vcan");
            if !Path::new(&format!("/sys/class/net/{iface}")).exists() {
                if let Some(serial) = dfu_device() {
                    out.push((Stufe::Warnung, tr!("Ein Adapter steckt im Update-Modus (DFU, Seriennr. {serial}) — darum kein '{iface}'. Zurückholen: sudo dfu-util -a 0 -s 0x08000000:leave  (Meldung „Error during download get_status“ ist normal). Dauerhaft automatisch: ./packaging/install.sh --can --dfu {serial}", serial = serial, iface = iface)));
                } else if all.is_empty() {
                    out.push((Stufe::Warnung, tr!("Kein CAN-Adapter gefunden ('{iface}' fehlt). Adapter eingesteckt? USB-Kabel/Port prüfen. Für ELM327-Adapter oben „ELM327“ wählen.", iface = iface)));
                } else {
                    let names: Vec<String> = all.iter().map(|(n, _, _)| n.clone()).collect();
                    out.push((Stufe::Warnung, tr!("'{iface}' gibt es nicht — gefunden: {a}. Oben als Interface eintragen.", iface = iface, a = names.join(", "))));
                }
                return out;
            }
            out.push((Stufe::Gut, tr!("Interface '{iface}' vorhanden{a}", iface = iface, a = if virt { " (virtuell)" } else { "" })));
            if hw_loopback(iface) {
                out.push((Stufe::Warnung, tr!("'{iface}' steht im Prüfmodus (loopback) — nur zum Testen ohne Auto. Fürs Auto: sudo ip link set {iface} down && sudo ip link set {iface} type can bitrate 500000 loopback off && sudo ip link set {iface} up", iface = iface)));
            }
            let up = all.iter().find(|(n, _, _)| n == iface).map(|x| x.1).unwrap_or(true);
            let br = bitrate(iface);
            if !up {
                out.push((
                    Stufe::Warnung,
                    tr!("'{iface}' ist nicht aktiv. Einschalten mit:  sudo ip link set {iface} type can bitrate 500000 && sudo ip link set {iface} up   (dauerhaft: ./packaging/install.sh --can)", iface = iface),
                ));
                return out;
            }
            match br {
                Some(500_000) => out.push((Stufe::Gut, tr!("Bitrate 500 kbit/s (Antriebs-CAN, passt für die OBD-Buchse)"))),
                Some(b) => out.push((
                    Stufe::Hinweis,
                    tr!("Bitrate {a} kbit/s — an der OBD-Buchse meist 500 kbit/s. Ändern: sudo ip link set {iface} down && sudo ip link set {iface} type can bitrate 500000 && sudo ip link set {iface} up", a = b / 1000, iface = iface),
                )),
                None if !virt => out.push((Stufe::Hinweis, tr!("Bitrate nicht lesbar"))),
                None => {}
            }
            if can_state(iface).as_deref() == Some("BUS-OFF") {
                if let Some(h) = state_hint(iface, "BUS-OFF") {
                    out.push(h);
                }
                return out;
            }
            if let Some(state) = read(&crate::platzhalter!("/sys/class/net/{iface}/operstate", iface = iface)) {
                if state == "down" {
                    out.push((Stufe::Hinweis, tr!("Interface meldet 'down' — Adapter-Treiber oder Kabel prüfen")));
                }
            }
        }
        Backend::Elm327Serial { port, .. } => {
            if !Path::new(port).exists() {
                let cand: Vec<String> = std::fs::read_dir("/dev")
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|n| n.starts_with("ttyUSB") || n.starts_with("ttyACM") || n.starts_with("rfcomm"))
                    .map(|n| format!("/dev/{n}"))
                    .collect();
                out.push((
                    Stufe::Warnung,
                    if cand.is_empty() { tr!("{port} nicht gefunden — ELM327 eingesteckt / Bluetooth gekoppelt?", port = port) } else { tr!("{port} nicht gefunden — vorhanden: {a}", port = port, a = cand.join(", ")) },
                ));
                return out;
            }
            if std::fs::OpenOptions::new().read(true).write(true).open(port).is_err() {
                out.push((Stufe::Warnung, tr!("Keine Berechtigung für {port}. Einmalig:  sudo usermod -aG dialout $USER  — danach ab- und wieder anmelden.", port = port)));
                return out;
            }
            out.push((Stufe::Gut, tr!("{port} vorhanden und zugreifbar", port = port)));
        }
        Backend::Elm327Tcp { addr } => {
            match addr.parse::<std::net::SocketAddr>().ok().map(|a| std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_secs(2))) {
                Some(Ok(_)) => out.push((Stufe::Gut, tr!("WLAN-Adapter {addr} erreichbar", addr = addr))),
                Some(Err(e)) => {
                    out.push((Stufe::Warnung, tr!("WLAN-Adapter {addr} nicht erreichbar ({e}) — mit dem WLAN des Adapters verbunden?", addr = addr, e = e)));
                    return out;
                }
                None => {
                    out.push((Stufe::Warnung, tr!("Adresse '{addr}' ungültig (Format 192.168.0.10:35000)", addr = addr)));
                    return out;
                }
            }
        }
    }

    // Antworten Steuergeräte?
    let uds = be.open(0x7E0, 0x7E8).and_then(|tp| crate::uds::Uds::new(&*tp).diagnostic_session(crate::uds::SESSION_DEFAULT).map(|_| ())).is_ok();
    let obd = !uds
        && be
            .open(crate::obd::DEFAULT_TX, crate::obd::DEFAULT_RX)
            .and_then(|tp| crate::obd::Obd::new(&*tp).read_pid(0x00))
            .is_ok();
    let tp20 = be.supports_tp20() && be.open_tp20(0x01, std::time::Duration::from_millis(500)).is_ok();
    if uds || obd || tp20 {
        let mut w = Vec::new();
        if uds {
            w.push("UDS");
        }
        if obd {
            w.push("OBD-II");
        }
        if tp20 {
            w.push("TP 2.0");
        }
        out.push((Stufe::Gut, tr!("Motorsteuergerät antwortet ({a}) — alles bereit", a = w.join(", "))));
        if let Ok(tp) = be.open(crate::obd::DEFAULT_TX, crate::obd::DEFAULT_RX) {
            if let Ok(u) = crate::obd::Obd::new(&*tp).read_value(0x42) {
                if u < 11.8 {
                    out.push((Stufe::Hinweis, tr!("Bordspannung nur {u} V — Batterie schwach, Ladegerät anschließen (Diagnose kann abbrechen)", u = format!("{:.1}", u))));
                }
            }
        }
    } else {
        // Nach den Versuchen sagt der CAN-Zustand, woran es liegt
        let hint = match be {
            Backend::SocketCan { iface } if !iface.starts_with("vcan") => can_state(iface).and_then(|st| state_hint(iface, &st)),
            _ => None,
        };
        out.push((
            Stufe::Warnung,
            tr!("Kein Steuergerät antwortet. Zündung an? Stecker richtig in der OBD-Buchse? Bei sehr alten Fahrzeugen (vor ~2004, K-Leitung) geht es nur mit einem K-Line-Adapter."),
        ));
        if let Some(h) = hint {
            out.push(h);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_ok_and_missing_iface() {
        // nicht während des Batterietests (der senkt die Demo-Spannung kurz ab)
        let _g = crate::sim::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let r = check(&Backend::Demo);
        assert!(r.iter().all(|(s, _)| *s == Stufe::Gut), "{r:?}");
        let r = check(&Backend::SocketCan { iface: "gibtsnicht9".into() });
        assert_eq!(r[0].0, Stufe::Warnung);
    }
}
