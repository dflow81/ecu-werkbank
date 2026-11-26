//! Mock-Steuergeräte für PC-Tests ohne Hardware.
//!
//! Stellt das eingebaute Demo-Fahrzeug ([`ecu_werkbank::sim`]) auf einem
//! (virtuellen) CAN-Interface bereit: UDS-Steuergeräte über ISO-TP (inkl.
//! OBD-II am Motor) und TP-2.0/KWP2000-Steuergeräte. So lassen sich alle
//! Befehle end-to-end über echte CAN-Frames testen.
//!
//! Ohne CAN-Frames geht es einfacher mit `ecu-werkbank -t demo …`.
//! Golf-6-Nachbau statt Demo-Fahrzeug: `VAG_DEMO=golf6 cargo run --bin mockecu`.
//! Mit `VAG_MOCK_BMW=1` antwortet der Motor zusätzlich wie ein BMW-Steuergerät
//! (erweiterte Adressierung: Anfrage 0x6F1 mit Adresse 0x12, Antwort 0x612) —
//! zum Prüfen von `fremd` ohne BMW.
//!
//! Start:
//!   sudo modprobe vcan; sudo ip link add dev vcan0 type vcan 2>/dev/null
//!   sudo ip link set up vcan0
//!   cargo run --bin mockecu -- vcan0

use std::thread;
use ecu_werkbank::isotp::IsoTp;
use ecu_werkbank::sim;
use ecu_werkbank::tp20::{CanIo, EcuSim, SocketIo};

fn main() -> anyhow::Result<()> {
    let iface = std::env::args().nth(1).unwrap_or_else(|| "vcan0".into());
    eprintln!("Mock-ECUs auf {iface} ({}):", sim::modell().name());
    let mut handles = Vec::new();
    for (addr, tx, rx, name) in sim::uds_modules() {
        eprintln!("  UDS    {addr:#04x} {name:<14} req={tx:#05x} resp={rx:#05x}");
        let iface = iface.clone();
        handles.push(thread::spawn(move || serve_uds(&iface, tx, rx)));
    }
    if std::env::var("VAG_MOCK_BMW").is_ok_and(|v| v == "1") {
        eprintln!("  BMW    0x12 Motor (wie DME)  req=0x6f1:12 resp=0x612:f1");
        let iface = iface.clone();
        handles.push(thread::spawn(move || serve_bmw(&iface, 0x12)));
    }
    let addrs = sim::tp20_addresses();
    eprintln!(
        "  TP 2.0 {}",
        addrs.iter().map(|a| format!("{a:#04x}")).collect::<Vec<_>>().join(", ")
    );
    {
        let iface = iface.clone();
        handles.push(thread::spawn(move || serve_tp20(&iface)));
    }
    eprintln!("Strg-C zum Beenden.");
    for h in handles {
        let _ = h.join();
    }
    Ok(())
}

fn serve_uds(iface: &str, tx: u16, rx: u16) {
    let tp = match IsoTp::open(iface, u32::from(rx), u32::from(tx)) {
        Ok(tp) => tp,
        Err(e) => {
            eprintln!("[{tx:#05x}] Fehler: {e}");
            return;
        }
    };
    loop {
        let req = match tp.recv() {
            Ok(r) if !r.is_empty() => r,
            _ => continue,
        };
        if let Some(resp) = sim::uds_response(tx, &req) {
            let _ = tp.send(&resp);
        }
    }
}

/// Motor des Demo-Fahrzeugs mit BMW-Adressierung (Adressbyte im ersten Datenbyte).
fn serve_bmw(iface: &str, adresse: u8) {
    use ecu_werkbank::isotp::mit_adresse;
    let tp = match IsoTp::open(iface, mit_adresse(0x600 + u16::from(adresse), 0xF1), mit_adresse(0x6F1, adresse)) {
        Ok(tp) => tp,
        Err(e) => {
            eprintln!("[BMW {adresse:#04x}] Fehler: {e}");
            return;
        }
    };
    loop {
        let req = match tp.recv() {
            Ok(r) if !r.is_empty() => r,
            _ => continue,
        };
        if let Some(resp) = sim::uds_response(0x7E0, &req) {
            let _ = tp.send(&resp);
        }
    }
}

fn serve_tp20(iface: &str) {
    let io = match SocketIo::open(iface, std::time::Duration::from_millis(500)) {
        Ok(io) => io,
        Err(e) => {
            eprintln!("[TP2.0] Fehler: {e}");
            return;
        }
    };
    let mut ecus: Vec<EcuSim> = sim::tp20_ecus();
    loop {
        let Ok((id, frame)) = io.read_any() else { continue };
        for ecu in ecus.iter_mut() {
            for (out_id, data) in ecu.on_frame(id, &frame) {
                let _ = io.write(out_id, &data);
            }
        }
    }
}
