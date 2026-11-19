//! CLI: Zielbereich, Stellfunktion, Prüfungen, Kaufcheck, Nachrüstung, TP-2.0-Schreiben.
//! Aus main.rs ausgelagert (Roadmap 27); Aufruf über `enum Command` in main.rs.

use super::*;

// ---------------------------------------------------------------------------
// Zielbereich / Stellfunktion
// ---------------------------------------------------------------------------

pub(super) static STOP_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(super) extern "C" fn on_sigint(_: libc::c_int) {
    STOP_REQUESTED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Strg-C abfangen, damit wir sauber aufräumen können.
pub(super) fn catch_ctrl_c() {
    // SAFETY: Der Handler setzt nur ein AtomicBool (async-signal-sicher).
    unsafe {
        libc::signal(libc::SIGINT, on_sigint as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
}

pub(super) fn stop_requested() -> bool {
    STOP_REQUESTED.load(std::sync::atomic::Ordering::SeqCst)
}

/// Balken: Ziel in der Mitte, ● = Istwert.
pub(super) fn band_bar(ist: f64, ziel: f64, tol: f64) -> String {
    let width = 31i64;
    let span = tol * 4.0;
    let pos = (((ist - ziel) / span + 0.5) * (width - 1) as f64).round().clamp(0.0, (width - 1) as f64) as i64;
    let lo = (((-tol) / span + 0.5) * (width - 1) as f64).round() as i64;
    let hi = (((tol) / span + 0.5) * (width - 1) as f64).round() as i64;
    (0..width)
        .map(|i| if i == pos { '●' } else if i >= lo && i <= hi { '=' } else { '-' })
        .collect()
}

pub(super) fn ziel(be: &Backend, pid: u8, muster: &str, toleranz: f64, seconds: u64) -> Result<()> {
    use std::io::Write;
    use ecu_werkbank::control::{bewerten, Muster, Richtung};
    let m = Muster::parse(muster)?;
    let (name, unit) = ecu_werkbank::obd::pid_info(pid).unwrap_or(("Wert", ""));
    catch_ctrl_c();
    println!("{}", tr!("Fahrer-Assistent: {name} {a} (±{b} {unit}). Strg-C beendet.\n", name = name, a = m.text(), b = ecu_werkbank::csvlog::num(toleranz), unit = unit));
    let tp = be.open(ecu_werkbank::obd::DEFAULT_TX, ecu_werkbank::obd::DEFAULT_RX)?;
    let o = ecu_werkbank::obd::Obd::new(&*tp);
    let start = Instant::now();
    let (mut n, mut inside) = (0u32, 0u32);
    while !stop_requested() && (seconds == 0 || start.elapsed() < Duration::from_secs(seconds)) {
        let t = start.elapsed().as_secs_f64();
        let soll = m.ziel(t);
        if let Ok(ist) = o.read_value(pid) {
            let (r, _) = bewerten(ist, soll, toleranz);
            n += 1;
            if r == Richtung::Passt {
                inside += 1;
            }
            let hint = match r {
                Richtung::Mehr => ecu_werkbank::trs!("▲ mehr Gas"),
                Richtung::Weniger => ecu_werkbank::trs!("▼ weniger Gas"),
                Richtung::Passt => ecu_werkbank::trs!("✓ im Bereich"),
            };
            print!("\r  {ist:7.0} {unit:<5} [{}]  {hint:<14} {}   ", band_bar(ist, soll, toleranz), tr!("Ziel {a}", a = format!("{soll:.0}")));
            let _ = std::io::stdout().flush();
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    println!("{}", tr!("\n\nIm Bereich: {a} % der Zeit ({n} Messungen).", a = format!("{:.0}", if n > 0 { f64::from(inside) * 100.0 / f64::from(n) } else { 0.0 }), n = n));
    Ok(())
}

pub(super) fn stellen(be: &Backend, db: &LabelDb, funktion: &str, muster: &str, seconds: u64, ja: bool) -> Result<()> {
    use ecu_werkbank::control::{demo_stellfunktion, Muster, Regler};
    let m = Muster::parse(muster)?;
    let f = if funktion.eq_ignore_ascii_case("demo") {
        if !matches!(be, Backend::Demo) {
            anyhow::bail!(tr!("Die Demo-Stellfunktion gibt es nur mit -t demo"));
        }
        demo_stellfunktion()
    } else {
        db.stellfunktionen()
            .iter()
            .find(|f| f.name.to_lowercase().contains(&funktion.to_lowercase()))
            .cloned()
            .with_context(|| {
                let names: Vec<&str> = db.stellfunktionen().iter().map(|f| f.name.as_str()).collect();
                tr!("Stellfunktion '{funktion}' nicht in den Labels (vorhanden: {a})", funktion = funktion, a = if names.is_empty() { tr!("keine") } else { names.join(", ") })
            })?
    };
    println!("{}", tr!("Stellfunktion: {a}", a = f.name));
    println!("{}", tr!("  Steuergerät {a}/{b}, {c} {d}", a = format!("{:#05x}", f.tx), b = format!("{:#05x}", f.rx), c = f.art, d = format!("{:#06x}", f.id)));
    println!("{}", tr!("  Muster:      {a}  (begrenzt auf {b}–{c} {d})", a = m.text(), b = f.min, c = f.max, d = f.einheit));
    println!("{}", tr!("  Dauer:       {seconds} s (Zeitlimit Steuergerät-Funktion: {a} s)", seconds = seconds, a = f.max_dauer_s));
    println!("{}", tr!("  Sicherheit:  nur im Stand; Abbruch bei Fahrt, Überschreitung, Fehlern, Strg-C"));
    if !ja {
        println!("{}", tr!("\nNichts ausgeführt. Fahrzeug steht, Handbremse angezogen, Gang raus/P? Dann mit --ja starten."));
        return Ok(());
    }
    catch_ctrl_c();
    let mut r = Regler::start(be, f)?;
    println!("{}", tr!("\nLäuft — Strg-C bricht ab und gibt die Kontrolle zurück.\n"));
    let result = (|| -> Result<()> {
        while !stop_requested() && r.laufzeit() < seconds as f64 {
            let soll = m.ziel(r.laufzeit());
            match r.schritt(soll) {
                Ok(x) => println!("{}", tr!("  {a} s  Soll {b}  Ist {c}", a = format!("{:5.1}", r.laufzeit()), b = format!("{:6.0}", x.soll), c = format!("{:>6}", x.ist.map(|v| format!("{v:.0}")).unwrap_or_else(|| "—".into())))),
                Err(e) if ecu_werkbank::control::ist_abbruch(&e) => return Err(e),
                Err(e) => println!("{}", tr!("  (Fehler, wird wiederholt: {e})", e = e)),
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Ok(())
    })();
    r.stop();
    println!("{}", tr!("\nKontrolle ans Steuergerät zurückgegeben."));
    result
}

pub(super) fn obd_tests(be: &Backend) -> Result<()> {
    use ecu_werkbank::obd::{self, Obd};
    let tp = be.open(obd::DEFAULT_TX, obd::DEFAULT_RX)?;
    let tests = Obd::new(&*tp).test_results()?;
    if tests.is_empty() {
        println!("{}", tr!("Keine Prüfergebnisse (Mode 06) verfügbar."));
        return Ok(());
    }
    println!("{}", tr!("{a} {b} {c} {d}  Ergebnis", a = format!("{:<52}", "Test"), b = format!("{:>10}", "Wert"), c = format!("{:>10}", "min"), d = format!("{:>10}", "max")));
    for t in &tests {
        let res = if !t.has_limits() {
            "—".to_string()
        } else {
            format!("{}{}", t.stufe().text(), t.reserve().map(|r| tr!(" (Reserve {r} %)", r = format!("{:.0}", r))).unwrap_or_default())
        };
        println!("{:<52} {:>10.2} {:>10.2} {:>10.2}  {res}  {}", t.name(), t.value, t.min, t.max, t.unit);
    }
    let sum = obd::test_summary(&tests);
    println!();
    if sum.is_empty() {
        println!("{}", tr!("Alle Tests ohne Auffälligkeit."));
    }
    for (st, t) in sum {
        println!("  [{}] {t}", st.text());
    }
    Ok(())
}

pub(super) fn gemisch(be: &Backend, seconds: u64, fahrzeug: Option<&str>) -> Result<()> {
    use std::io::Write;
    use ecu_werkbank::mixture;
    let demo = matches!(be, Backend::Demo);
    let tp = be.open(ecu_werkbank::obd::DEFAULT_TX, ecu_werkbank::obd::DEFAULT_RX)?;
    let o = ecu_werkbank::obd::Obd::new(&*tp);
    let veh = fahrzeug.and_then(ecu_werkbank::vehicle::find);
    let run_phase = |title: &str, target: Option<f64>| -> Result<Vec<mixture::Sample>> {
        println!("\n{title}");
        if demo {
            let rpm = target.unwrap_or(800.0);
            ecu_werkbank::sim::set_muster(ecu_werkbank::sim::Param::Drehzahl, Some(ecu_werkbank::control::Muster::Halten(rpm)));
        }
        let start = Instant::now();
        let mut v = Vec::new();
        while start.elapsed() < Duration::from_secs(seconds) {
            let s = mixture::sample(&o)?;
            let hint = match target {
                Some(t) if s.rpm < t - 250.0 => "▲ mehr Gas",
                Some(t) if s.rpm > t + 250.0 => "▼ weniger Gas",
                Some(_) => "✓ halten",
                None => "",
            };
            print!("\r  {:4.0} s  {:5.0} 1/min  kurz {:+5.1} %  lang {:+5.1} %  {hint}      ", start.elapsed().as_secs_f64(), s.rpm, s.stft, s.ltft);
            let _ = std::io::stdout().flush();
            if target.is_none_or(|t| (s.rpm - t).abs() < 400.0) {
                v.push(s);
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        println!();
        Ok(v)
    };
    println!("{}", tr!("Gemisch-Assistent — Motor betriebswarm, alle Verbraucher aus, Fahrzeug steht."));
    let idle = run_phase(&tr!("Phase 1: Leerlauf ({seconds} s) — nichts tun.", seconds = seconds), None)?;
    let high = run_phase(&tr!("Phase 2: Drehzahl ca. 2500 1/min halten ({seconds} s).", seconds = seconds), Some(2500.0))?;
    if demo {
        ecu_werkbank::sim::clear_muster();
    }
    if idle.is_empty() || high.is_empty() {
        anyhow::bail!(tr!("Zu wenige Messwerte — Phase 2 bei ca. 2500 1/min wiederholen."));
    }
    let (a, b) = (mixture::summarize(&idle), mixture::summarize(&high));
    println!("{}", tr!("\nErgebnis:"));
    for (st, t) in mixture::analyze(&a, &b, veh.as_ref().and_then(|v| v.hubraum_l)) {
        println!("  [{}] {t}", st.text());
    }
    if let Some(v) = veh {
        let mut tr = vec![("Gemisch Leerlauf".to_string(), a.total(), "%".to_string()), ("Gemisch 2500".to_string(), b.total(), "%".to_string())];
        if let Some(m) = a.maf {
            tr.push(("Luftmasse Leerlauf".into(), m, "g/s".into()));
        }
        ecu_werkbank::trend::add(&v.key, v.km, &tr)?;
        println!("{}", tr!("\n(In den Trends von {a} gespeichert.)", a = v.label()));
    }
    Ok(())
}

pub(super) fn kaufcheck(be: &Backend, db: &LabelDb, out: Option<&std::path::Path>, note: &str, fahrzeug: Option<&str>) -> Result<()> {
    let active = active_vehicle(fahrzeug)?;
    println!("{}", tr!("Gebrauchtwagen-Check über {a} …\n", a = be.describe()));
    let mut e = ecu_werkbank::usedcar::run(be, db, |_| {})?;
    e.report.note = note.to_string();
    let v = e.report.assign_vehicle(active.as_deref())?;
    for x in &e.punkte {
        println!("  [{:<7}] {:<20} {}", x.stufe.text(), x.bereich, x.text);
    }
    println!("\n→ {}", e.urteil());
    let out = match out {
        Some(p) => p.to_path_buf(),
        None => v.sub("berichte")?.join(ecu_werkbank::vehicle::stamped("kaufcheck", "pdf")),
    };
    ecu_werkbank::ablage::schreiben(&out, e.to_pdf())?;
    let _ = ecu_werkbank::history::save(&e.report);
    println!("{}", tr!("PDF: {a}", a = out.display()));
    Ok(())
}

/// Gebrauchtwagen-Check ohne Auto aus dem letzten gespeicherten Scan mit Abgasdaten.
pub(super) fn kaufcheck_aus_scan(out: Option<&std::path::Path>, note: &str, fahrzeug: Option<&str>) -> Result<()> {
    // ohne --fahrzeug: das in der GUI gewählte Fahrzeug
    let gewaehlt = || ecu_werkbank::ablage::lesen(ecu_werkbank::history::data_dir().join("aktives-fahrzeug")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let key = active_vehicle(fahrzeug)?.or_else(gewaehlt).context(tr!("Kein Fahrzeug gewählt — mit --fahrzeug angeben"))?;
    let v = ecu_werkbank::vehicle::load(&key).context(tr!("Fahrzeug nicht gefunden"))?;
    let r = ecu_werkbank::usedcar::letzter_scan(&v.key).context(tr!("Für dieses Fahrzeug ist noch kein Scan gespeichert"))?;
    let mut e = ecu_werkbank::usedcar::aus_scan(r.clone());
    if !note.is_empty() {
        e.report.note = note.to_string();
    }
    println!("{}", tr!("Gebrauchtwagen-Check aus dem Scan vom {a} …\n", a = r.created));
    for x in &e.punkte {
        println!("  [{:<7}] {:<20} {}", x.stufe.text(), x.bereich, x.text);
    }
    println!("\n→ {}", e.urteil());
    let out = match out {
        Some(p) => p.to_path_buf(),
        None => v.sub("berichte")?.join(ecu_werkbank::vehicle::stamped("kaufcheck", "pdf")),
    };
    ecu_werkbank::ablage::schreiben(&out, e.to_pdf())?;
    println!("{}", tr!("PDF: {a}", a = out.display()));
    Ok(())
}

pub(super) fn reparaturbericht(be: &Backend, db: &LabelDb, arbeiten: &str, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::history;
    let active = active_vehicle(fahrzeug)?;
    let mut after = ecu_werkbank::report::scan(be, db, true, |_| {})?;
    let v = after.assign_vehicle(active.as_deref())?;
    let before = history::list(&v.key).pop().map(|(_, r)| r).context(tr!("Kein früherer Scan als „vorher“ — vor der Reparatur `report` oder `check` ausführen"))?;
    after.reparatur = arbeiten.trim().to_string();
    after.comparison = history::compare_with_previous(&after);
    let offen: Vec<String> = be
        .open(ecu_werkbank::obd::DEFAULT_TX, ecu_werkbank::obd::DEFAULT_RX)
        .ok()
        .and_then(|tp| ecu_werkbank::obd::Obd::new(&*tp).readiness().ok())
        .map(|r| r.monitors.iter().filter(|m| m.available && !m.complete).map(|m| m.name.to_string()).collect())
        .unwrap_or_default();
    let out = v.sub("berichte")?.join(ecu_werkbank::vehicle::stamped("reparaturbericht", "pdf"));
    ecu_werkbank::ablage::schreiben(&out, ecu_werkbank::repair::bericht_pdf(&before, &after, arbeiten, &offen))?;
    history::save(&after)?;
    let d = history::diff(&before, &after, &[]);
    println!("{}", tr!("Vorher {a} Fehler ({b}), nachher {c} Fehler.", a = before.total_dtcs(), b = before.created, c = after.total_dtcs()));
    println!("{}", tr!("Behoben: {a}, noch da: {b}, neu: {c}", a = d.gone.len(), b = d.still.len(), c = d.new.len()));
    if !offen.is_empty() {
        println!("{}", tr!("Readiness offen: {a}", a = offen.join(", ")));
    }
    println!("{}", tr!("PDF: {a}", a = out.display()));
    Ok(())
}

pub(super) fn software(be: &Backend, db: &LabelDb, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::swinfo;
    let st = swinfo::read(be, db, true, |_| {})?;
    println!("{:<10} {:<22} {:<14} {:<8} {:<14} {:<6} Seriennr.", "Adresse", "System", "Teilenummer", "SW", "HW-Nr.", "HW");
    for m in &st.module {
        let sys = if m.system.is_empty() { &m.name } else { &m.system };
        println!("{:<10} {:<22} {:<14} {:<8} {:<14} {:<6} {}", m.adresse, sys, m.teilenummer, m.software, m.hardware_nr, m.hardware_ver, m.seriennummer);
    }
    let active = active_vehicle(fahrzeug)?;
    let v = ecu_werkbank::vehicle::resolve(st.vin.as_deref(), active.as_deref())?;
    if let Some(prev) = swinfo::list(&v.key).pop() {
        let d = swinfo::compare(&prev, &st);
        println!("{}", tr!("\nSeit {a}: {b}", a = prev.created, b = if d.is_empty() { tr!("keine Änderung") } else { tr!("{n} Änderung(en)", n = d.len()) }));
        for (m, t) in d {
            println!("  {m}: {t}");
        }
    }
    let p = swinfo::save(&v.key, &st)?;
    println!("{}", tr!("\nGespeichert: {a}", a = p.display()));
    Ok(())
}

pub(super) fn nachruestung(be: &Backend, db: &LabelDb, name: &str, phase: &str, notiz: &str, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::retrofit;
    let v = need_vehicle(fahrzeug)?;
    let dir = v.sub("nachruestung")?.join(ecu_werkbank::vehicle::plate_key(name).to_lowercase());
    std::fs::create_dir_all(&dir)?;
    match phase {
        "vorher" => {
            let st = retrofit::aufnehmen(be, db, |_| {})?;
            ecu_werkbank::ablage::schreiben(dir.join("vorher-scan.toml"), toml::to_string(&st.scan)?)?;
            ecu_werkbank::ablage::schreiben(dir.join("vorher-codierung.toml"), toml::to_string(&st.codierung)?)?;
            println!("{}", tr!("Vorher-Stand gesichert ({a} Steuergeräte, {b} Fehler). Jetzt einbauen, dann --phase nachher.", a = st.scan.modules.len(), b = st.scan.total_dtcs()));
        }
        "nachher" => {
            let vorher = retrofit::Stand {
                scan: ecu_werkbank::history::load(&dir.join("vorher-scan.toml")).context(tr!("Erst --phase vorher ausführen"))?,
                codierung: ecu_werkbank::backup::load(&dir.join("vorher-codierung.toml"))?,
            };
            let mut nachher = retrofit::aufnehmen(be, db, |_| {})?;
            nachher.scan.assign_vehicle(Some(&v.key))?;
            nachher.scan.reparatur = tr!("Nachrüstung {name}{a}", name = name, a = if notiz.is_empty() { String::new() } else { format!(": {notiz}") });
            let r = retrofit::vergleich(&vorher, &nachher, db);
            for (st, t) in &r {
                println!("  [{}] {t}", st.text());
            }
            let out = v.sub("berichte")?.join(ecu_werkbank::vehicle::stamped("nachruestung", "pdf"));
            ecu_werkbank::ablage::schreiben(&out, retrofit::pdf(name, &vorher, &nachher, &r, notiz))?;
            ecu_werkbank::history::save(&nachher.scan)?;
            nachher.codierung.vehicle = v.key.clone();
            ecu_werkbank::backup::save(&nachher.codierung)?;
            println!("{}", tr!("PDF: {a}", a = out.display()));
        }
        x => anyhow::bail!(tr!("--phase {x}: erlaubt sind vorher | nachher", x = x)),
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn codier_bit(be: &Backend, tx: u16, rx: u16, did: u16, byte: usize, bit: u8, wert: &str, ja: bool, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::backup;
    let on = match wert.to_lowercase().as_str() {
        "ein" | "1" | "an" => true,
        "aus" | "0" => false,
        x => anyhow::bail!(tr!("--wert {x}: ein oder aus", x = x)),
    };
    let tp = be.open(tx, rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(ecu_werkbank::uds::SESSION_DEFAULT)?;
    let now = uds.read_data_by_id(did)?;
    drop(tp);
    let mut new = now.clone();
    if !ecu_werkbank::set_coding_bit(&mut new, byte, bit, on) {
        anyhow::bail!(tr!("Byte {byte} gibt es nicht (Codierung hat {a} Byte)", byte = byte, a = now.len()));
    }
    if let Some(n) = ecu_werkbank::codiertexte::Texte::laden().eigen(tx, byte, Some(bit)) {
        println!("{}", tr!("Bit: {a}", a = n));
    }
    println!("{}", tr!("Codierung {did} an {tx}:\n  jetzt: {a}\n  neu:   {b}", did = format!("{:#06x}", did), tx = format!("{:#05x}", tx), a = backup::hex(&now), b = backup::hex(&new)));
    if now == new {
        println!("{}", tr!("Bit ist bereits so gesetzt."));
        return Ok(());
    }
    if !ja {
        println!("{}", tr!("Nichts geschrieben. Mit --ja ausführen (vorher wird automatisch gesichert)."));
        return Ok(());
    }
    // Sicherung des Ist-Stands
    let active = active_vehicle(fahrzeug)?;
    let mk = |data: &[u8]| backup::CodingEntry { protocol: "UDS".into(), address: None, name: String::new(), tx, rx, did: Some(did), part: String::new(), data_hex: backup::hex(data), note: String::new() };
    let b = backup::CodingBackup {
        created: ecu_werkbank::report::now_local(),
        timestamp: ecu_werkbank::report::unix_now(),
        vin: None,
        transport: be.describe(),
        vehicle: ecu_werkbank::vehicle::resolve(None, active.as_deref())?.key,
        entries: vec![mk(&now)],
    };
    let p = backup::save(&b)?;
    println!("{}", tr!("Sicherung: {a}", a = p.display()));
    let login = ecu_werkbank::login::fuer(&b.vehicle, tx);
    backup::restore(be, &mk(&new), login.as_ref())?;
    println!("{}", tr!("Geschrieben und kontrolliert ✓  (Rückgängig: coding-restore --file {a} --tx {tx} --ja)", a = p.display(), tx = format!("{:#05x}", tx)));
    Ok(())
}


pub(super) fn ersatzteilpass_neu(be: &Backend, fahrzeug: Option<&str>) -> Result<()> {
    let v = fahrzeug_fuer(be, fahrzeug)?;
    let pdf = ecu_werkbank::notfall::pass_neu(&v)?;
    println!("{}", tr!("Ersatzteil-Pass neu erzeugt: {a}", a = pdf.display()));
    Ok(())
}

pub(super) fn notfall_sicherung(be: &Backend, db: &LabelDb, fahrzeug: Option<&str>) -> Result<()> {
    let v = fahrzeug_fuer(be, fahrzeug)?;
    println!("{}", tr!("Komplett-Sicherung für {a} über {b} — nur lesen, dauert etwa 10–20 Minuten. Zündung an lassen, Ladegerät an die Batterie.", a = v.label(), b = be.describe()));
    let mut zuletzt = String::new();
    let e = ecu_werkbank::notfall::sichern(be, db, &v, |t| {
        if t != zuletzt {
            println!("  {t}");
            zuletzt = t.to_string();
        }
    })?;
    let n: usize = e.anpassung.geraete.iter().map(|g| g.kennungen.len()).sum();
    println!("{}", tr!("Fertig: {a} Steuergeräte, {b} Codierungen, {c} Anpassungswerte.", a = e.software.module.len(), b = e.codierung.entries.len(), c = n));
    for d in &e.dateien {
        println!("  {}", d.display());
    }
    println!("{}", tr!("Ersatzteil-Pass: {a}", a = e.pdf.display()));
    Ok(())
}

/// Verbauliste aus dem Gateway anzeigen (nur lesen).
pub(super) fn gateway_verbauliste(be: &Backend) -> Result<()> {
    println!("{}", tr!("Verbauliste aus dem Gateway über {a} (nur lesen) …", a = be.describe()));
    let l = kwp::gateway_verbauliste(be)?;
    let verbaut: Vec<_> = l.iter().filter(|e| e.verbaut()).collect();
    // mit dem letzten gespeicherten Scan des aktiven Fahrzeugs abgleichen
    let scan = load_active().and_then(|k| ecu_werkbank::history::list(&k).pop()).map(|(_, r)| r);
    let aeste = ecu_werkbank::verbauliste::baum(&l, scan.as_ref());
    print!("{}", ecu_werkbank::verbauliste::baum_text(&aeste, scan.is_some()));
    let fehler = verbaut.iter().filter(|e| e.fehler()).count();
    println!("{}", tr!("\n{a} verbaut, davon {b} mit Fehler; {c} weitere Adressen kennt das Gateway als nicht verbaut.", a = verbaut.len(), b = fehler, c = l.len() - verbaut.len()));
    Ok(())
}

/// Anpassungskanäle eines TP-2.0-Steuergeräts anzeigen (nur lesen).
pub(super) fn kwp_anpassung(be: &Backend, address: u8, kanaele: &[u8]) -> Result<()> {
    println!("{}", tr!("Anpassung Steuergerät {a} (nur lesen) …", a = format!("{address:02X}")));
    for r in kwp::anpassung_lesen(be, address, kanaele)? {
        match r {
            Ok(k) => {
                let felder: Vec<String> = k.felder.iter().filter(|v| !v.is_leer()).map(|v| v.display()).collect();
                println!("{}", tr!("  Kanal {a}: Wert {b}   [{c}]", a = format!("{:03}", k.kanal), b = k.wert, c = felder.join("  |  ")));
            }
            Err(e) => println!("  {e}"),
        }
    }
    Ok(())
}

/// Stellglieddiagnose im Terminal: Enter = nächstes Bauteil, q = Ende.
pub(super) fn kwp_stellglied(be: &Backend, address: u8, ja: bool) -> Result<()> {
    println!("{}", tr!("Stellglieddiagnose Steuergerät {a}: Bauteile werden dabei WIRKLICH angesteuert (Fensterheber, Verriegelung, Lampen …).", a = format!("{address:02X}")));
    if !ja {
        println!("{}", tr!("Nichts gestartet. Fahrzeug sichern (Hände weg von Fenstern/Türen), dann mit --ja starten."));
        return Ok(());
    }
    println!("{}", tr!("Enter = nächstes Bauteil, q + Enter = beenden."));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut zeile = String::new();
        while std::io::stdin().read_line(&mut zeile).is_ok() {
            let ende = zeile.trim().eq_ignore_ascii_case("q") || zeile.is_empty();
            if tx.send(if ende { kwp::StellBefehl::Ende } else { kwp::StellBefehl::Weiter }).is_err() || ende {
                break;
            }
            zeile.clear();
        }
    });
    kwp::stellglieddiagnose(be, address, |s| {
        if let Some(s) = s {
            println!("  {}", s.text());
        }
        rx.try_recv().unwrap_or(kwp::StellBefehl::Bleiben)
    })?;
    println!("{}", tr!("Stellglieddiagnose beendet."));
    Ok(())
}

/// Kurzcodierung schreiben (TP 2.0) — erst Vorschau, mit --ja wirklich.
pub(super) fn kwp_codierung_schreiben(be: &Backend, address: u8, wert: u32, ja: bool) -> Result<()> {
    let mut ch = kwp_open(be, address)?;
    let raw = Kwp::new(&mut ch).read_ecu_id(kwp::ECU_ID_VAG)?;
    drop(ch);
    let alt = match kwp::kurzcodierung_9b(&raw) {
        Some(kwp::TpCodierung::Kurz { wert, .. }) => wert,
        _ => anyhow::bail!(tr!("Steuergerät {a} hat keine Kurzcodierung (Aufbau 03 xx xx xx).", a = format!("{address:02X}"))),
    };
    println!("{}", tr!("Steuergerät {a}: Kurzcodierung jetzt {b}, neu {c}.", a = format!("{address:02X}"), b = format!("{alt:07}"), c = format!("{wert:07}")));
    if !ja {
        println!("{}", tr!("Nichts geschrieben. Den alten Wert notieren, dann mit --ja schreiben."));
        return Ok(());
    }
    let (vorher, danach) = kwp::kurzcodierung_schreiben(be, address, wert)?;
    if danach == wert {
        println!("{}", tr!("Geschrieben und kontrolliert: {a} → {b}.", a = format!("{vorher:07}"), b = format!("{danach:07}")));
    } else {
        println!("{}", tr!("Achtung: danach gelesen {a}, erwartet {b}. Alter Wert war {c}.", a = format!("{danach:07}"), b = format!("{wert:07}"), c = format!("{vorher:07}")));
    }
    Ok(())
}

/// Ergebnis eines TP-2.0-Schreibversuchs ausgeben.
pub(super) fn schreib_ergebnis(r: &kwp::Schreiben, zeige: impl Fn(&[u8]) -> String) {
    match r {
        kwp::Schreiben::Probelauf { anfrage, alt } => {
            println!("{}", tr!("PROBELAUF — nichts gesendet. Dieser Schreibweg ist am Auto noch nicht mit einem Mitschnitt bestätigt."));
            println!("{}", tr!("Jetzt: {a}", a = zeige(alt)));
            println!("{}", tr!("Würde senden: {a}", a = ecu_werkbank::backup::hex(anfrage)));
        }
        kwp::Schreiben::Geschrieben { alt, danach } => {
            println!("{}", tr!("Geschrieben. Vorher {a}, danach gelesen {b}.", a = zeige(alt), b = zeige(danach)));
        }
    }
}

pub(super) fn kwp_langcodierung(be: &Backend, address: u8, neu: Option<&str>, ja: bool, db: &LabelDb) -> Result<()> {
    let mut ch = kwp_open(be, address)?;
    let (art, alt) = kwp::langcodierung_lesen(&mut Kwp::new(&mut ch)).ok_or_else(|| anyhow::anyhow!(tr!("Steuergerät {a} hat keine lange Codierung.", a = format!("{address:02X}"))))?;
    drop(ch);
    let wo = match art {
        kwp::LangArt::Kennung0600 => "0x0600",
        kwp::LangArt::Gateway9A(_) => "0x9A",
    };
    println!("{}", tr!("Steuergerät {a}: lange Codierung ({b} Byte, {c})", a = format!("{address:02X}"), b = alt.len(), c = wo));
    println!("  {}", ecu_werkbank::backup::hex(&alt));
    let Some(neu) = neu else { return Ok(()) };
    let neu: Vec<u8> = parse_hex_bytes(neu)?;
    let anfrage = kwp::langcodierung_anfrage(&art, &alt, &neu)?;
    for (i, (a, b)) in alt.iter().zip(&neu).enumerate() {
        for bit in 0..8 {
            if (a ^ b) >> bit & 1 == 1 {
                println!("  {}", tr!("Byte {i} Bit {bit}: {a} → {b}", i = i, bit = bit, a = a >> bit & 1, b = b >> bit & 1));
            }
        }
    }
    for h in ecu_werkbank::codiertexte::regeln_pruefen(db.coding(u16::from(address)).unwrap_or(&[]), &neu) {
        println!("  ⚠ {h}");
    }
    if !ja {
        println!("{}", tr!("Nichts geschrieben (Anfrage wäre {a}). Den alten Stand notieren, dann mit --ja schreiben.", a = ecu_werkbank::backup::hex(&anfrage)));
        return Ok(());
    }
    let r = kwp::langcodierung_schreiben(be, address, &neu)?;
    schreib_ergebnis(&r, ecu_werkbank::backup::hex);
    if let kwp::Schreiben::Geschrieben { danach, .. } = &r {
        if *danach != neu {
            println!("{}", tr!("Achtung: danach gelesen ist nicht der neue Stand."));
        }
    }
    Ok(())
}

pub(super) fn kwp_anpassung_schreiben(be: &Backend, address: u8, kanal: u8, wert: u16, ja: bool) -> Result<()> {
    let wert_text = |b: &[u8]| if b.len() == 2 { u16::from_be_bytes([b[0], b[1]]).to_string() } else { ecu_werkbank::backup::hex(b) };
    if !ja {
        let alt = match kwp::anpassung_lesen(be, address, &[kanal])?.pop() {
            Some(Ok(a)) => a.wert,
            Some(Err(e)) => anyhow::bail!(e),
            None => anyhow::bail!(tr!("Kanal {a}: keine Antwort", a = kanal)),
        };
        println!("{}", tr!("Steuergerät {a}, Kanal {b}: jetzt {c}, neu {d}.", a = format!("{address:02X}"), b = format!("{kanal:03}"), c = alt, d = wert));
        println!("{}", tr!("Nichts geschrieben. Den alten Wert notieren, dann mit --ja schreiben."));
        return Ok(());
    }
    let r = kwp::anpassung_schreiben(be, address, kanal, wert)?;
    schreib_ergebnis(&r, wert_text);
    if let kwp::Schreiben::Geschrieben { danach, .. } = &r {
        if *danach != wert.to_be_bytes() {
            println!("{}", tr!("Achtung: danach gelesen ist nicht der neue Wert."));
        }
    }
    Ok(())
}
