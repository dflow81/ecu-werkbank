//! CLI: Mitschnitte aufzeichnen und auswerten.
//! Aus main.rs ausgelagert (Roadmap 27); Aufruf über `enum Command` in main.rs.

use super::*;

// ---------------------------------------------------------------------------
// Mitschnitte
// ---------------------------------------------------------------------------

pub(super) fn decode_log(file: &std::path::Path, report_only: bool) -> Result<()> {
    let text = ecu_werkbank::ablage::lesen(file).with_context(|| tr!("Log lesen: {a}", a = file.display()))?;
    let d = ecu_werkbank::canlog::Decoder::decode_text(&text);
    if !report_only {
        for l in d.lines() {
            println!("{l}");
        }
        println!();
    }
    print!("{}", d.pruefbericht());
    Ok(())
}

pub(super) fn record(be: &Backend, iface: &str, out: &std::path::Path, seconds: u64) -> Result<()> {
    use std::io::Write;
    if let Backend::Demo = be {
        ecu_werkbank::ablage::schreiben(out, ecu_werkbank::canlog::demo_log())?;
        println!("{}", tr!("Beispiel-Mitschnitt aus dem Demo-Fahrzeug: {a}", a = out.display()));
        println!("{}", tr!("Ansehen mit:  ecu-werkbank decode-log {a}", a = out.display()));
        return Ok(());
    }
    let Backend::SocketCan { .. } = be else {
        anyhow::bail!(tr!("Mitschneiden geht nur mit SocketCAN (oder -t demo für ein Beispiel)"));
    };
    let sock = CanSocket::open(iface).with_context(|| tr!("CAN-Interface '{iface}' öffnen", iface = iface))?;
    sock.set_read_timeout(Duration::from_millis(200))?;
    let mut w = std::io::BufWriter::new(std::fs::File::create(out)?);
    println!("{}", tr!("Schneide {seconds}s auf {iface} mit → {a} (Strg-C bricht ab)…", seconds = seconds, iface = iface, a = out.display()));
    let start = Instant::now();
    let mut n = 0usize;
    while start.elapsed() < Duration::from_secs(seconds) {
        let Ok(frame) = sock.read_frame() else { continue };
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let id = match frame.id() {
            Id::Standard(s) => u32::from(s.as_raw()),
            Id::Extended(e) => e.as_raw(),
        };
        writeln!(w, "{}", ecu_werkbank::canlog::format_line(t, iface, id, frame.data()))?;
        n += 1;
        if n.is_multiple_of(100) {
            w.flush()?;
        }
    }
    w.flush()?;
    println!("{}", tr!("{n} Frames gespeichert. Auswerten mit:  ecu-werkbank decode-log {a}", n = n, a = out.display()));
    Ok(())
}
