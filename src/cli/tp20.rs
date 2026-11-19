//! CLI: TP 2.0 / KWP2000: lesen (Identifikation, Fehler, Messwerte, Anpassung, Login).
//! Aus main.rs ausgelagert (Roadmap 27); Aufruf über `enum Command` in main.rs.

use super::*;

// ---------------------------------------------------------------------------
// TP 2.0 / KWP2000
// ---------------------------------------------------------------------------

pub(super) fn kwp_open(be: &Backend, address: u8) -> Result<tp20::Channel> {
    kwp::open_session(be, address)
}

pub(super) fn kwp_dtc_line(d: &kwp::KwpDtc, address: u8, db: &LabelDb) -> String {
    let obd = d.obd_code(Some(address));
    let text = obd
        .as_deref()
        .and_then(|c| db.dtc_description(c))
        .or_else(|| db.dtc_description(&d.vag_code()))
        .unwrap_or("");
    let obd_s = obd.map(|c| format!(" ({c})")).unwrap_or_default();
    format!("{}{obd_s}  {text}  ({})", d.vag_code(), d.status_text())
}

pub(super) fn kwp_ident(be: &Backend, address: u8) -> Result<()> {
    let mut ch = kwp_open(be, address)?;
    let (tx, rx) = (ch.tx_id(), ch.rx_id());
    let mut k = Kwp::new(&mut ch);
    let (data, lines) = match k.read_ecu_id(kwp::ECU_ID_VAG) {
        Ok(d) => {
            // fester VW-Aufbau (Teilenummer, SW, Name ab Byte 26), sonst Textstücke
            let l = kwp::ident_9b(&d).unwrap_or_else(|| kwp::ident_strings(&d));
            (d, l)
        }
        // neuere Steuergeräte: 0x22 statt 0x1A
        Err(e) => {
            let l = kwp::ident_22(&mut k);
            if l.is_empty() {
                return Err(e);
            }
            (Vec::new(), l)
        }
    };
    let name = kwp::tp20_name(address, &lines);
    println!("{}", tr!("Steuergerät {address} {name} (tx={tx} rx={rx}):", address = format!("{:#04x}", address), name = name, tx = format!("{:#05x}", tx), rx = format!("{:#05x}", rx)));
    for s in &lines {
        println!("  {s}");
    }
    if data.is_empty() {
        return Ok(());
    }
    let hex: Vec<String> = data.iter().map(|b| format!("{b:02X}")).collect();
    println!("{}", tr!("  Rohdaten: {a}", a = hex.join(" ")));
    Ok(())
}

pub(super) fn kwp_dtc(be: &Backend, address: u8, db: &LabelDb) -> Result<()> {
    let mut ch = kwp_open(be, address)?;
    let dtcs = Kwp::new(&mut ch).read_dtcs_mit_umgebung()?;
    if dtcs.is_empty() {
        println!("{}", tr!("Keine Fehlercodes gespeichert."));
    } else {
        println!("{}", tr!("{a} Fehlercode(s):", a = dtcs.len()));
        for (d, env) in &dtcs {
            println!("  {}", kwp_dtc_line(d, address, db));
            if let Some(e) = env {
                println!("{}", tr!("      Umgebungsdaten (roh): {a}", a = kwp::umgebung_hex(e)));
            }
        }
    }
    Ok(())
}

pub(super) fn kwp_clear(be: &Backend, address: u8) -> Result<()> {
    let mut ch = kwp_open(be, address)?;
    Kwp::new(&mut ch).clear_dtcs()?;
    println!("{}", tr!("Fehlerspeicher von {address} gelöscht.", address = format!("{:#04x}", address)));
    Ok(())
}

/// In der GUI zuletzt gewähltes Fahrzeug (<Datenordner>/aktives-fahrzeug).
pub(super) fn load_active() -> Option<String> {
    let key = ecu_werkbank::ablage::lesen(ecu_werkbank::history::data_dir().join("aktives-fahrzeug")).ok()?;
    ecu_werkbank::vehicle::load(key.trim()).map(|v| v.key)
}

/// Fahrzeug für Motor-Checks: --fahrzeug, sonst über die VIN (OBD), sonst das aktive.
/// Optionaler Preis „45,90“ → Zahl (Fehler bei Unsinn).
pub(super) fn preis_lesen(p: Option<&str>) -> Result<Option<f64>> {
    match p.map(str::trim).filter(|p| !p.is_empty()) {
        None => Ok(None),
        Some(p) => ecu_werkbank::kosten::zahl_lesen(p).map(Some).with_context(|| tr!("Preis bitte als Zahl eingeben, z. B. 45,90")),
    }
}

pub(super) fn fehler_geruest(be: &Backend, db: &LabelDb, out: Option<PathBuf>, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::fehlergeruest;
    println!("{}", tr!("Mögliche Fehlercodes lesen über {a} (nur lesen) …", a = be.describe()));
    let g = fehlergeruest::lesen(be, db, |t| {
        use std::io::Write;
        eprint!("\r  {t} …");
        let _ = std::io::stderr().flush();
    })?;
    eprintln!("\r          ");
    if g.is_empty() {
        anyhow::bail!(tr!("Kein UDS-Steuergerät hat seine Fehlercodes gemeldet. (TP 2.0 / KWP2000 kennt diese Abfrage nicht.)"));
    }
    let v = fahrzeug_fuer(be, fahrzeug).ok();
    let name = v.as_ref().map(|v| if v.bezeichnung.is_empty() { v.key.clone() } else { v.bezeichnung.clone() }).unwrap_or_default();
    let jetzt = ecu_werkbank::report::now_local();
    let text = fehlergeruest::geruest(&g, db, &name, &jetzt);
    let datei = format!("fehlertexte-geruest-{}.toml", ecu_werkbank::report::unix_now());
    let pfad = out.unwrap_or_else(|| match &v {
        Some(v) => ecu_werkbank::vehicle::dir(&v.key).join(&datei),
        None => PathBuf::from(&datei),
    });
    if let Some(p) = pfad.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(p)?;
    }
    ecu_werkbank::ablage::schreiben(&pfad, &text).with_context(|| tr!("Schreiben: {a}", a = pfad.display()))?;
    let n: usize = g.iter().map(|x| x.codes.len()).sum();
    for x in &g {
        println!("{}", tr!("  {a} {b}  {c} mögliche Fehler", a = format!("{:<28}", x.name), b = format!("{:#05x}", x.tx), c = x.codes.len()));
    }
    println!("{}", tr!("{a} Steuergerät(e), {b} Fehlercodes → {c}", a = g.len(), b = n, c = pfad.display()));
    println!("{}", tr!("Klartexte eintragen, dann: ecu-werkbank fehlertexte-uebernehmen \"{a}\"", a = pfad.display()));
    Ok(())
}

pub(super) fn fahrzeug_fuer(be: &Backend, fahrzeug: Option<&str>) -> Result<ecu_werkbank::vehicle::Vehicle> {
    let active = active_vehicle(fahrzeug)?;
    if active.is_some() {
        return ecu_werkbank::vehicle::resolve(None, active.as_deref());
    }
    let vin = ecu_werkbank::profil::quick_vin(be);
    ecu_werkbank::vehicle::resolve(vin.as_deref(), load_active().as_deref())
}

pub(super) fn kette(be: &Backend, feld: Option<&str>, proben: usize, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::motorcheck::{self, Feld};
    let v = fahrzeug_fuer(be, fahrzeug)?;
    let mut e = motorcheck::laden(&v.key);
    if let Some(f) = feld {
        e.kette = Some(Feld::parse(f).with_context(|| tr!("--feld {a}: Block/Feld, z. B. 093/2", a = f))?);
        motorcheck::speichern(&v.key, &e)?;
    }
    let Some(feld) = e.kette else {
        println!("{}", tr!("Noch kein Feld für die Phasenlage festgelegt. Suche mit:"));
        println!("  ecu-werkbank kwp-blockscan --address 01 --von 80 --bis 100");
        println!("{}", tr!("Felder mit Art „~Winkel“ oder „~Zündwinkel“ sind Kandidaten. Dann:  ecu-werkbank kette --feld 093/2"));
        return Ok(());
    };
    println!("{}", tr!("Steuerketten-Check ({a}, {b}) — Motor betriebswarm im Leerlauf, nichts tun …", a = v.label(), b = feld.text()));
    motorcheck::demo_vorbereiten(be, false);
    let m = motorcheck::kette_messen(be, feld, proben, |_| {});
    motorcheck::demo_ende(be);
    let m = m?;
    let frueher: Vec<f64> = ecu_werkbank::trend::series(&v.key).get(motorcheck::TREND_KETTE).map(|p| p.iter().map(|x| x.wert).collect()).unwrap_or_default();
    println!("{}", tr!("Phasenlage {a} {u} (Spanne {b}, {c} Proben, {d} 1/min{e})", a = format!("{:.1}", m.wert), u = m.einheit, b = format!("{:.1}", m.streuung), c = m.proben, d = format!("{:.0}", m.drehzahl), e = m.kuehlmittel.map(|t| format!(", {t:.0} °C")).unwrap_or_default()));
    for (st, t) in motorcheck::kette_bewerten(&m, &frueher) {
        println!("  [{}] {t}", st.text());
    }
    if m.kuehlmittel.is_none_or(|t| t >= 80.0) {
        ecu_werkbank::trend::add(&v.key, v.km, &[(motorcheck::TREND_KETTE.to_string(), m.wert, m.einheit.clone())])?;
        println!("{}", tr!("(In den Trends von {a} gespeichert.)", a = v.label()));
    }
    Ok(())
}

pub(super) fn druckpruefung(be: &Backend, art: &str, paar: Option<(&str, &str)>, sekunden: u64, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::motorcheck::{self, Druck, Feld};
    let art = match art.to_lowercase().as_str() {
        "ladedruck" | "lade" | "turbo" => Druck::Ladedruck,
        "kraftstoff" | "kraftstoffdruck" | "rail" => Druck::Kraftstoff,
        x => anyhow::bail!(tr!("--art {a}: ladedruck oder kraftstoff", a = x)),
    };
    let v = fahrzeug_fuer(be, fahrzeug)?;
    let mut e = motorcheck::laden(&v.key);
    if let Some((s, i)) = paar {
        let p = [Feld::parse(s).with_context(|| tr!("--soll {a}: Block/Feld, z. B. 115/3", a = s))?, Feld::parse(i).with_context(|| tr!("--ist {a}: Block/Feld, z. B. 115/4", a = i))?];
        match art {
            Druck::Ladedruck => e.ladedruck = Some(p),
            Druck::Kraftstoff => e.kraftstoff = Some(p),
        }
        motorcheck::speichern(&v.key, &e)?;
    }
    let paar = match art {
        Druck::Ladedruck => e.ladedruck.or(Some([Feld { block: 115, feld: 3 }, Feld { block: 115, feld: 4 }])),
        Druck::Kraftstoff => e.kraftstoff,
    };
    let Some(paar) = paar else {
        println!("{}", tr!("Noch keine Felder für Soll/Ist festgelegt. Suche mit `kwp-blockscan --address 01`, dann z. B.:  ecu-werkbank druckpruefung --art kraftstoff --soll 103/3 --ist 103/4"));
        return Ok(());
    };
    println!("{}", tr!("{a}-Prüfung ({b} / {c}): Motor warm, 3. Gang, ab ca. 1500 1/min voll beschleunigen. Nur wo es sicher und erlaubt ist! Aufzeichnung {d} s läuft …", a = art.name(), b = paar[0].text(), c = paar[1].text(), d = sekunden));
    motorcheck::demo_vorbereiten(be, true);
    let r = motorcheck::druck_aufzeichnen(be, paar, sekunden as f64, |_| true);
    motorcheck::demo_ende(be);
    let (proben, einheit) = r?;
    let (erg, bew) = motorcheck::druck_bewerten(art, &proben, &einheit);
    for (st, t) in bew {
        println!("  [{}] {t}", st.text());
    }
    if let Some(r) = erg {
        let name = tr!("{a} größte Lücke", a = art.name());
        ecu_werkbank::trend::add(&v.key, v.km, &[(name, r.luecke, einheit)])?;
        println!("{}", tr!("(In den Trends von {a} gespeichert.)", a = v.label()));
    }
    Ok(())
}

pub(super) fn rekorder(be: &Backend, bloecke: &[u8], grenzen: &[String], vorlauf: u64, nachlauf: u64, fahrzeug: Option<&str>) -> Result<()> {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use ecu_werkbank::rekorder::{self, Grenze, Meldung};
    let g: Vec<Grenze> = grenzen.iter().map(|x| Grenze::parse(x).with_context(|| tr!("--grenze {a}: z. B. \"003/4>40\"", a = x))).collect::<Result<_>>()?;
    let v = fahrzeug_fuer(be, fahrzeug)?;
    let ordner = v.sub("rekorder")?;
    let stop = AtomicBool::new(false);
    let merken = Arc::new(AtomicBool::new(false));
    {
        let m = merken.clone();
        std::thread::spawn(move || {
            let mut l = String::new();
            while std::io::stdin().read_line(&mut l).is_ok_and(|n| n > 0) {
                m.store(true, std::sync::atomic::Ordering::Relaxed);
                l.clear();
            }
        });
    }
    println!("{}", tr!("Fahrt-Rekorder läuft (Blöcke {a}) — Enter = Moment merken, Strg-C = Ende. Ablage: {b}", a = bloecke.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","), b = ordner.display()));
    rekorder::laufen(be, bloecke, &g, vorlauf as f64, nachlauf as f64, &ordner, &stop, &merken, |m| match m {
        Meldung::Laeuft { spalten: Some(s), .. } => println!("{}", tr!("Spalten: {a}", a = s.join(" | "))),
        Meldung::Ausgeloest(t) => println!("{}", tr!("⏺ Ausgelöst: {a} — nehme noch {b} s auf …", a = t, b = nachlauf)),
        Meldung::Gespeichert(p, t) => println!("{}", tr!("✓ Gespeichert ({a}): {b}", a = t, b = p.display())),
        _ => {}
    })
}

pub(super) fn anpassung_sichern(be: &Backend, db: &LabelDb, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::anpassung;
    println!("{}", tr!("Anpassungen sichern über {a} (nur lesen, je Steuergerät etwa 30–60 s) …", a = be.describe()));
    let mut zuletzt = String::new();
    let mut s = anpassung::sichern(be, db, true, |g, _| {
        if g != zuletzt {
            println!("  {g}");
            zuletzt = g.to_string();
        }
    })?;
    s.vehicle = fahrzeug_fuer(be, fahrzeug)?.key;
    let p = anpassung::speichern(&s)?;
    let n: usize = s.geraete.iter().map(|g| g.kennungen.len()).sum();
    println!("{}", tr!("{a} Steuergerät(e), {b} Kennungen gesichert: {c}", a = s.geraete.len(), b = n, c = p.display()));
    Ok(())
}

pub(super) fn anpassung_vergleich(alt: Option<&std::path::Path>, neu: Option<&std::path::Path>, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::anpassung;
    let (a, b) = match (alt, neu) {
        (Some(a), Some(b)) => (anpassung::laden(a)?, anpassung::laden(b)?),
        _ => {
            // ohne --fahrzeug: das Fahrzeug mit der neuesten Sicherung
            let l = match active_vehicle(fahrzeug)? {
                Some(k) => anpassung::liste(&k),
                None => ecu_werkbank::vehicle::list().iter().map(|v| anpassung::liste(&v.key)).max_by_key(|l| l.first().map(|x| x.1.timestamp)).unwrap_or_default(),
            };
            // nur komplette Sicherungen (nicht die Werte vor einem Zurückschreiben)
            let l: Vec<_> = l.into_iter().filter(|x| x.1.vollstaendig()).collect();
            if l.len() < 2 {
                anyhow::bail!(tr!("Zum Vergleich braucht es zwei Sicherungen (vorhanden: {a}).", a = l.len()));
            }
            (l[1].1.clone(), l[0].1.clone())
        }
    };
    println!("{}", tr!("Anpassungen vom {a}  →  {b}\n", a = a.created, b = b.created));
    let d = anpassung::vergleichen(&a, &b);
    if d.is_empty() {
        println!("{}", tr!("Keine Kennung geändert."));
    }
    for (geraet, aend) in d {
        println!("{geraet}");
        for x in aend {
            println!("  {}", x.zeile());
        }
    }
    Ok(())
}

pub(super) fn grundeinstellung_cmd(system: Option<&str>, suche: Option<&str>) -> Result<()> {
    use ecu_werkbank::grundeinstellung as ge;
    let treffer = ge::filtern(system, suche);
    if treffer.is_empty() {
        println!("{}", tr!("Nichts gefunden. Systeme: {a}", a = ge::systeme().join(", ")));
        return Ok(());
    }
    println!("{}", tr!("Grundeinstellungen/Werkstattfunktionen (Referenz, {a}):", a = treffer.len()));
    let mut letztes = "";
    for e in treffer {
        if e.system != letztes {
            println!("\n{}", e.system);
            letztes = e.system;
        }
        println!("  {a} {b}  [{c}]", a = e.stufe.ampel(), b = e.name, c = e.stufe.text());
        println!("      {}", e.zweck);
        println!("      ⚠ {}", e.hinweis);
    }
    println!("{}", tr!("\nReines Nachschlagen — ausgeführt wird nichts. Service-Rückstellung: `service`. Stellglieder: `stellen` (nur im Stand)."));
    Ok(())
}



pub(super) fn ident_cmd(be: &Backend, geraet: u16, rx: Option<u16>) -> Result<()> {
    use ecu_werkbank::swinfo;
    let rx = rx.unwrap_or_else(|| ecu_werkbank::uds::antwort_id(geraet));
    println!("{}", tr!("Erweiterte Identifikation {a} über {b} (nur lesen) …", a = format!("{geraet:#05x}"), b = be.describe()));
    let tp = be.open(geraet, rx)?;
    let uds = Uds::new(&*tp);
    uds.diagnostic_session(ecu_werkbank::uds::SESSION_DEFAULT).context(tr!("Steuergerät antwortet nicht (Zündung an?)"))?;
    let mut gelesen = 0;
    for (did, name) in swinfo::STD_DIDS {
        match uds.read_data_by_id(*did) {
            Ok(d) if !d.is_empty() => {
                println!("  {a:<32} {b}", a = name, b = swinfo::ident_wert(*did, &d));
                gelesen += 1;
            }
            _ => {}
        }
    }
    // lange Codierung (0x0600), falls vorhanden
    if let Ok(d) = uds.read_data_by_id(ecu_werkbank::backup::VAG_CODING_DID) {
        if !d.is_empty() {
            println!("  {a:<32} {b}", a = tr!("Codierung (lang)"), b = ecu_werkbank::backup::hex(&d));
            gelesen += 1;
        }
    }
    if gelesen == 0 {
        println!("{}", tr!("Keine Kennfelder gelesen — spricht das Gerät UDS? (TP 2.0: kwp-ident nutzen)"));
    }
    Ok(())
}

pub(super) fn anpassung_schreibtest(be: &Backend, geraet: u16, kennung: Option<u16>, ja: bool, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::anpassung;
    let l = match active_vehicle(fahrzeug)? {
        Some(k) => anpassung::liste(&k),
        None => ecu_werkbank::vehicle::list().iter().map(|v| anpassung::liste(&v.key)).max_by_key(|l| l.first().map(|x| x.1.timestamp)).unwrap_or_default(),
    };
    let s = l.into_iter().map(|x| x.1).find(|s| s.vollstaendig()).context(tr!("Keine Anpassungs-Sicherung gefunden (erst: anpassung-sichern oder notfall-sicherung)."))?;
    let g = anpassung::geraet_finden(&s, geraet).with_context(|| tr!("Steuergerät {a} ist nicht in dieser Sicherung", a = format!("{geraet:#x}")))?;
    let did = match kennung {
        Some(d) => d,
        None => g.kennungen.keys().next().and_then(|k| u16::from_str_radix(k, 16).ok()).context(tr!("Dieses Steuergerät hat keine Kennungen."))?,
    };
    println!("{}", tr!("Schreibtest an {a}, Kennung {b} — es wird der jetzige Inhalt unverändert zurückgeschrieben.", a = g.titel(), b = format!("{did:#06X}")));
    if !ja {
        let r = anpassung::schreibtest(be, g, &s.vehicle, did, false)?;
        println!("{}", tr!("Jetziger Inhalt: {a}", a = ecu_werkbank::backup::hex(&r.vorher)));
        println!("{}", tr!("Nichts geschrieben. Zum echten Test denselben Befehl mit --ja ausführen (Zündung an)."));
        return Ok(());
    }
    let r = anpassung::schreibtest(be, g, &s.vehicle, did, true)?;
    println!("{}", tr!("Jetziger Inhalt: {a}", a = ecu_werkbank::backup::hex(&r.vorher)));
    match &r.ergebnis {
        Ok(()) => println!("{}", tr!("Schreiben und Kontroll-Lesen erfolgreich ✓ — der Schreibweg funktioniert (nichts verändert).")),
        Err(t) => println!("{}", tr!("Nicht geschrieben: {a}", a = t)),
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn login_cmd(be: &Backend, geraet: Option<u16>, sa2: Option<String>, key: Option<String>, kwp: bool, index: u8, level: u8, test: bool, loeschen: bool, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::login::{self, Methode};
    let v = fahrzeug_fuer(be, fahrzeug)?;
    let Some(id) = geraet else {
        let l = login::laden(&v.key);
        if l.geraete.is_empty() {
            println!("{}", tr!("Keine Logins hinterlegt. Beispiel: ecu-werkbank login --geraet 714 --sa2 <hex> --level 3"));
        }
        for (k, m) in &l.geraete {
            println!("  {k}  {}", m.beschreibung());
        }
        return Ok(());
    };
    // TP 2.0 = Adresse < 0x100; UDS = CAN-Anfrage-ID
    let ist_tp20 = id < 0x100;
    if loeschen {
        let mut l = login::laden(&v.key);
        if l.geraete.remove(&format!("{id:#05x}")).is_some() {
            let d = ecu_werkbank::vehicle::dir(&v.key).join("logins.toml");
            ecu_werkbank::ablage::schreiben(&d, toml::to_string(&l)?)?;
            println!("{}", tr!("Login für {a} gelöscht.", a = format!("{id:#05x}")));
        }
        return Ok(());
    }
    if kwp {
        login::setzen(&v.key, id, Methode::KwpXor { ecu_index: index })?;
        println!("{}", tr!("KWP-Login (XorKey, Index {b}) für {a} hinterlegt.", a = format!("{id:#05x}"), b = index));
    } else if let Some(bc) = sa2 {
        let roh = ecu_werkbank::backup::parse_hex(&bc).context(tr!("SA2-Bytecode ist kein gültiges Hex"))?;
        login::setzen(&v.key, id, Methode::Sa2 { level, bytecode: ecu_werkbank::backup::hex(&roh) })?;
        println!("{}", tr!("SA2-Login für {a} hinterlegt (Level {b}, {c} Byte).", a = format!("{id:#05x}"), b = level, c = roh.len()));
    } else if let Some(k) = key {
        let roh = ecu_werkbank::backup::parse_hex(&k).context(tr!("Key ist kein gültiges Hex"))?;
        login::setzen(&v.key, id, Methode::FesterKey { level, key: ecu_werkbank::backup::hex(&roh) })?;
        println!("{}", tr!("Fester-Key-Login für {a} hinterlegt (Level {b}).", a = format!("{id:#05x}"), b = level));
    }
    if test {
        let m = login::fuer(&v.key, id).with_context(|| tr!("Für {a} ist kein Login hinterlegt.", a = format!("{id:#05x}")))?;
        println!("{}", tr!("Teste Login an {a} ({b}) — es wird nur angemeldet, nichts geschrieben …", a = format!("{id:#05x}"), b = m.beschreibung()));
        let erg = if ist_tp20 {
            let mut ch = kwp_open(be, id as u8)?;
            login::anmelden_kwp(&mut ecu_werkbank::kwp::Kwp::new(&mut ch), &m)
        } else {
            let tp = be.open(id, ecu_werkbank::uds::antwort_id(id))?;
            let uds = ecu_werkbank::uds::Uds::new(&*tp);
            uds.diagnostic_session(ecu_werkbank::uds::SESSION_EXTENDED).context(tr!("erweiterte Diagnosesitzung (0x03) abgelehnt"))?;
            login::anmelden_uds(&uds, &m)
        };
        match erg {
            Ok(()) => println!("{}", tr!("Login erfolgreich ✓ — das Steuergerät erlaubt jetzt das Schreiben.")),
            Err(e) => println!("{}", tr!("Login fehlgeschlagen: {e}", e = format!("{e:#}"))),
        }
    }
    Ok(())
}

pub(super) fn anpassung_zurueck(be: &Backend, datei: Option<&std::path::Path>, geraet: Option<u16>, kennung: &[u16], alle: bool, ja: bool, fahrzeug: Option<&str>) -> Result<()> {
    use ecu_werkbank::anpassung::{self, Zustand};
    let s = match datei {
        Some(p) => anpassung::laden(p)?,
        None => {
            let l = match active_vehicle(fahrzeug)? {
                Some(k) => anpassung::liste(&k),
                None => ecu_werkbank::vehicle::list().iter().map(|v| anpassung::liste(&v.key)).max_by_key(|l| l.first().map(|x| x.1.timestamp)).unwrap_or_default(),
            };
            // die neueste komplette (nicht die Werte vor einem Zurückschreiben)
            l.into_iter().map(|x| x.1).find(|s| s.vollstaendig()).context(tr!("Keine Anpassungs-Sicherung gefunden (erst: anpassung-sichern oder notfall-sicherung)."))?
        }
    };
    println!("{}", tr!("Sicherung vom {a} ({b})", a = s.created, b = s.vehicle));
    if !s.notiz.is_empty() {
        println!("  {}", s.notiz);
    }
    let Some(id) = geraet else {
        println!("{}", tr!("Steuergeräte in der Sicherung (Auswahl mit --geraet):"));
        for g in &s.geraete {
            let id = g.tp20.map(|a| format!("{a:02x}")).unwrap_or_else(|| format!("{:03x}", g.tx));
            println!("{}", tr!("  --geraet {a} {b}  ({c} Kennungen)", a = format!("{id:<6}"), b = g.titel(), c = g.kennungen.len()));
        }
        return Ok(());
    };
    let g = anpassung::geraet_finden(&s, id).with_context(|| tr!("Steuergerät {a} ist nicht in dieser Sicherung", a = format!("{id:#x}")))?;
    anpassung::fahrzeug_pruefen(be, &s)?;
    println!("{}", tr!("{a}: vergleiche {b} Kennungen mit dem Auto (nur lesen) …", a = g.titel(), b = g.kennungen.len()));
    let punkte = anpassung::pruefen(be, g, &s.vehicle, |_, _| {})?;
    let gleich = punkte.iter().filter(|p| p.zustand == Zustand::Gleich).count();
    println!("{}", tr!("{a} gleich, {b} anders:", a = gleich, b = punkte.len() - gleich));
    for p in punkte.iter().filter(|p| p.zustand != Zustand::Gleich) {
        println!("  {}", p.zeile());
    }
    let ziel: Vec<u16> = if alle {
        punkte.iter().filter(|p| p.zustand == Zustand::Anders).map(|p| p.did).collect()
    } else {
        kennung.to_vec()
    };
    if ziel.is_empty() {
        if punkte.iter().any(|p| p.zustand == Zustand::Anders) {
            println!("{}", tr!("\nZum Zurückschreiben: --kennung <hex> (einzeln) oder --alle, dann mit --ja."));
        }
        return Ok(());
    }
    println!("{}", tr!("\nZurückschreiben: {a}", a = ziel.iter().map(|d| format!("{d:04X}")).collect::<Vec<_>>().join(", ")));
    if !ja {
        println!("{}", tr!("Nichts geschrieben. Zum Schreiben denselben Befehl mit --ja ausführen (Zündung an, Motor aus, Ladegerät empfohlen)."));
        return Ok(());
    }
    let r = anpassung::zurueckschreiben(be, g, &s.vehicle, &ziel)?;
    for e in &r {
        match &e.ergebnis {
            Ok(()) => println!("{}", tr!("  {a}  geschrieben und kontrolliert ✓", a = format!("{:#06X}", e.did))),
            Err(t) => println!("{}", tr!("  {a}  nicht geschrieben: {b}", a = format!("{:#06X}", e.did), b = t)),
        }
    }
    if let Some(p) = anpassung::vorher_sichern(&s.vehicle, g, &r)? {
        println!("{}", tr!("Die Werte davor sind gesichert: {a}\n(Rückgängig: anpassung-zurueck --datei {a} --geraet {b} --alle --ja)", a = p.display(), b = format!("{id:x}")));
    }
    Ok(())
}

/// Codierung eines TP-2.0-Steuergeräts lesen und aufschlüsseln.
pub(super) fn kwp_coding(be: &Backend, address: u8, db: &LabelDb) -> Result<()> {
    use ecu_werkbank::kwp::TpCodierung;
    let mut ch = kwp_open(be, address)?;
    let mut k = Kwp::new(&mut ch);
    let ident_raw = k.read_ecu_id(kwp::ECU_ID_VAG).unwrap_or_default();
    let key = ecu_werkbank::codiertexte::tp20_key(address);
    match kwp::codierung_lesen(&mut k, &ident_raw) {
        Some(TpCodierung::Lang(b)) => {
            println!("{}", tr!("Codierung {a} (TP 2.0) = {b}", a = format!("{address:02X}"), b = ecu_werkbank::backup::hex(&b)));
            println!("{}", tr!("Eigene Texte dazu: codiertext --tx {a} …", a = format!("{key:03X}")));
            print_coding(None, Some(key), &b, db);
        }
        Some(TpCodierung::Kurz { wert, roh }) => {
            println!("{}", tr!("Kurzcodierung {a}: {b}", a = format!("{address:02X}"), b = format!("{wert:07}")));
            if let Some((w, i, h)) = roh.get(4..10).and_then(ecu_werkbank::swinfo::betriebsnummer) {
                println!("{}", tr!("Betriebsnummer der letzten Codierung: Werkstatt {w} · Importeur {i} · Händler {h}", w = format!("{w:05}"), i = format!("{i:03}"), h = format!("{h:05}")));
            }
        }
        None => anyhow::bail!(tr!("Keine Codierung lesbar (weder in der Identifikation noch über Kennung 0x0600).")),
    }
    Ok(())
}

/// Alle Blöcke von `von` bis `bis` lesen; nicht vorhandene still überspringen.
pub(super) fn kwp_blockscan(be: &Backend, address: u8, von: u8, bis: u8) -> Result<()> {
    let texte = ecu_werkbank::messwerttexte::Texte::laden();
    println!("{}", tr!("Blockscan Steuergerät {a}, Block {b}–{c} (nur lesen) …", a = format!("{address:02X}"), b = von, c = bis));
    let gefunden = kwp::blockscan(be, address, von, bis, |_| {})?;
    for (block, v) in &gefunden {
        let name = texte.block(address, *block).map(|n| format!("  [{n}]")).unwrap_or_default();
        println!("{block:03}{name}: {}", texte.zeile(address, *block, v));
    }
    println!("{}", tr!("{a} Block/Blöcke mit Werten. Felder mit „~“ haben noch keinen eigenen Namen (messwerttext oder ✏ in der GUI).", a = gefunden.len()));
    Ok(())
}

/// Namensvorschläge gegen die echten Blöcke prüfen und auf Wunsch übernehmen.
pub(super) fn messwert_vorschlaege(be: &Backend, address: u8, ja: bool) -> Result<()> {
    use ecu_werkbank::messwerttexte;
    use ecu_werkbank::mwvorschlag::{self, Pruefung};
    let vorschlaege = mwvorschlag::fuer(address);
    if vorschlaege.is_empty() {
        anyhow::bail!(tr!("Für Steuergerät {a} gibt es noch keine Vorschläge (bisher nur Motor 01).", a = format!("{address:02X}")));
    }
    let mut ch = kwp_open(be, address)?;
    let texte = messwerttexte::Texte::laden();
    let (mut neu, mut passend) = (0, 0);
    for b in vorschlaege {
        let werte = match Kwp::new(&mut ch).read_block(b.block) {
            Ok(w) => w,
            Err(_) => {
                println!("{}", tr!("Block {a} ({b}): gibt es hier nicht", a = format!("{:03}", b.block), b = b.name));
                continue;
            }
        };
        println!("{}", tr!("Block {a} ({b}):", a = format!("{:03}", b.block), b = b.name));
        let mut block_passt = false;
        for (f, p) in mwvorschlag::pruefen(b, &werte) {
            let (zeichen, info) = match &p {
                Pruefung::Passt => ("✓", String::new()),
                Pruefung::Anders(x) => ("✗", tr!("  (Formel {a} passt nicht — anders belegt)", a = format!("{x:#04x}"))),
                Pruefung::Fehlt => ("–", tr!("  (Feld fehlt)")),
            };
            let eigen = texte.eigen(address, b.block, f.feld);
            let schon = eigen.map(|t| tr!("  — schon benannt: {a}", a = t)).unwrap_or_default();
            println!("  {zeichen} {}{info}{schon}", tr!("Feld {a}: {b}", a = f.feld, b = f.name));
            if p == Pruefung::Passt {
                passend += 1;
                block_passt = true;
                if ja && eigen.is_none() {
                    messwerttexte::setzen(address, b.block, Some(f.feld), f.name)?;
                    neu += 1;
                }
            }
        }
        if ja && block_passt && texte.block(address, b.block).is_none() {
            messwerttexte::setzen(address, b.block, None, b.name)?;
        }
    }
    if ja {
        println!("{}", tr!("{a} Feldname(n) übernommen. Ändern jederzeit mit ✏ in der GUI oder `messwerttext`.", a = neu));
    } else {
        println!("{}", tr!("{a} Vorschlag/Vorschläge passen. Übernehmen mit --ja (vorhandene eigene Texte bleiben).", a = passend));
    }
    Ok(())
}

pub(super) fn kwp_block(
    be: &Backend,
    address: u8,
    blocks: &[u8],
    csv: Option<&std::path::Path>,
    count: u32,
    interval: u64,
    db: &LabelDb,
) -> Result<()> {
    let mut ch = kwp_open(be, address)?;
    let texte = ecu_werkbank::messwerttexte::Texte::laden();
    for &block in blocks {
        let name = texte.block(address, block).map(str::to_string).or_else(|| db.measuring(u16::from(address), u16::from(block)).map(|m| m.name.clone()).filter(|n| !n.is_empty()));
        if let Some(n) = name {
            println!("{}", tr!("Block {block}: {a}", block = format!("{:03}", block), a = n));
        }
    }
    let mut log = csv.map(ecu_werkbank::csvlog::CsvLog::create).transpose()?;
    let mut n = 0u32;
    loop {
        let mut cols = Vec::new();
        for &block in blocks {
            let values = Kwp::new(&mut ch).read_block(block)?;
            println!("{block:03}: {}", texte.zeile(address, block, &values));
            cols.extend(block_columns(block, &values));
        }
        if let Some(l) = log.as_mut() {
            l.write(&cols)?;
        }
        n += 1;
        if count != 0 && n >= count {
            break;
        }
        std::thread::sleep(Duration::from_millis(interval));
    }
    if let Some(l) = log {
        println!("{}", tr!("{a} Zeile(n) in {b} gespeichert.", a = l.rows(), b = l.path().display()));
    }
    Ok(())
}

/// CSV-Spalten eines Messwertblocks: "B001.1 [1/min]" → Wert.
pub(super) fn block_columns(block: u8, values: &[kwp::BlockValue]) -> Vec<(String, String)> {
    values
        .iter()
        .enumerate()
        .map(|(i, v)| match v.decode() {
            Some((x, unit)) => (format!("B{block:03}.{} [{unit}]", i + 1), ecu_werkbank::csvlog::num(x)),
            None => (tr!("B{block}.{a} [roh]", block = format!("{:03}", block), a = i + 1), format!("{:02X}{:02X}{:02X}", v.formula, v.a, v.b)),
        })
        .collect()
}
