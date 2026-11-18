//! CLI: Codierungs-Sicherung.
//! Aus main.rs ausgelagert (Roadmap 27); Aufruf über `enum Command` in main.rs.

use super::*;

// ---------------------------------------------------------------------------
// Codierungs-Sicherung
// ---------------------------------------------------------------------------

pub(super) fn coding_backup(be: &Backend, db: &LabelDb, with_tp20: bool, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::backup;
    let active = active_vehicle(fahrzeug)?;
    let mut b = backup::backup(be, db, with_tp20, |_| {})?;
    b.vehicle = ecu_werkbank::vehicle::resolve(b.vin.as_deref(), active.as_deref())?.key;
    if b.entries.is_empty() {
        anyhow::bail!(tr!("Keine Steuergeräte erreichbar — nichts gesichert."));
    }
    for e in &b.entries {
        let data = if e.data_hex.is_empty() { e.note.clone() } else { e.data_hex.clone() };
        println!("  {:<34} {data}", e.title());
    }
    let p = backup::save(&b)?;
    println!("{}", tr!("\n{a} Steuergerät(e) gesichert: {b}", a = b.entries.len(), b = p.display()));
    Ok(())
}

pub(super) fn coding_list() -> Result<()> {
    let all = ecu_werkbank::backup::list_all();
    if all.is_empty() {
        println!("{}", tr!("Noch keine Sicherungen. Anlegen mit:  ecu-werkbank coding-backup"));
    }
    for (p, b) in all {
        println!("{}", tr!("{a}  {b} {c} Steuergerät(e)  {d}", a = b.created, b = format!("{:<18}", b.vin.as_deref().unwrap_or(ecu_werkbank::trs!("ohne VIN"))), c = format!("{:>2}", b.entries.len()), d = p.display()));
    }
    Ok(())
}

pub(super) fn coding_compare(old: Option<&std::path::Path>, new: Option<&std::path::Path>, db: &LabelDb) -> Result<()> {
    use ecu_werkbank::backup;
    let (a, b) = match (old, new) {
        (Some(o), Some(n)) => (backup::load(o)?, backup::load(n)?),
        _ => {
            let all = backup::list_all();
            if all.len() < 2 {
                anyhow::bail!(tr!("Zum Vergleich braucht es zwei Sicherungen (vorhanden: {a}).", a = all.len()));
            }
            (all[1].1.clone(), all[0].1.clone())
        }
    };
    println!("{}", tr!("Sicherung vom {a}  →  {b}\n", a = a.created, b = b.created));
    let changes = backup::compare(&a, &b, db);
    if changes.is_empty() {
        println!("{}", tr!("Keine Codierung geändert."));
    }
    for c in changes {
        println!("{}", c.module);
        let zeig = |h: &str| if h.trim().is_empty() { tr!("(nicht lesbar)") } else { h.to_string() };
        println!("  {}", tr!("vorher: {a}", a = zeig(&c.old_hex)));
        println!("  {}", tr!("jetzt:  {a}", a = zeig(&c.new_hex)));
        for (byte, bit, x, y, name) in c.bits {
            let name = if name.is_empty() { String::new() } else { format!("  {name}") };
            println!("{}", tr!("    Byte {byte} Bit {bit}: {a} → {b}{name}", byte = byte, bit = bit, a = if x { tr!("ein") } else { tr!("aus") }, b = if y { tr!("ein") } else { tr!("aus") }, name = name));
        }
    }
    Ok(())
}

pub(super) fn coding_restore(be: &Backend, file: &std::path::Path, tx: u16, ja: bool) -> Result<()> {
    use ecu_werkbank::backup;
    let b = backup::load(file)?;
    let e = b
        .entries
        .iter()
        .find(|e| e.tx == tx)
        .with_context(|| tr!("Steuergerät {tx} ist nicht in dieser Sicherung", tx = format!("{:#05x}", tx)))?;
    if !e.can_restore() {
        anyhow::bail!(tr!("{a}: Zurückschreiben nicht möglich ({b})", a = e.title(), b = e.note));
    }
    // Aktuellen Stand zeigen
    let tp = be.open(e.tx, e.rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(ecu_werkbank::uds::SESSION_DEFAULT)?;
    let now = uds.read_data_by_id(e.did.expect("geprüft"))?;
    drop(tp);
    println!("{}", e.title());
    println!("  aktuell:      {}", backup::hex(&now));
    println!("{}", tr!("  Sicherung:    {a}  (vom {b})", a = e.data_hex, b = b.created));
    if now == e.bytes() {
        println!("{}", tr!("\nIdentisch — nichts zu tun."));
        return Ok(());
    }
    if !ja {
        println!("{}", tr!("\nNichts geschrieben. Zum Zurückschreiben denselben Befehl mit --ja ausführen."));
        return Ok(());
    }
    let login = ecu_werkbank::login::fuer(&b.vehicle, e.tx);
    backup::restore(be, e, login.as_ref())?;
    println!("{}", tr!("\nZurückgeschrieben und kontrolliert ✓"));
    Ok(())
}
