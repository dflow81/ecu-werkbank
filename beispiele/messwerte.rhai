// Beispiel-Skript für ECUWerkbank (nur lesen).
// Starten:  ecu-werkbank -t golf6 skript beispiele/messwerte.rhai
//           (ohne -t golf6 am echten Auto)
// Funktionen: siehe src/skript.rs

print(`Fahrgestellnummer: ${obd_vin()}`);

// Drehzahl und Kühlmittel 5× im Abstand von 1 s
for i in 0..5 {
    let n = obd(0x0C);
    let k = obd(0x05);
    print(`${zeit().to_int()} s   Drehzahl ${n} 1/min   Kühlmittel ${k} °C`);
    warte(1000);
}

// Älteres Steuergerät (TP 2.0): Motor, Messwertblock 2
try {
    for w in kwp_block(0x01, 2) {
        print(`  ${w.text}`);
    }
} catch (e) {
    print(`TP 2.0 nicht erreichbar: ${e}`);
}

// Fehlerspeicher Motor: erst UDS, sonst TP 2.0
let f = [];
try { f = uds_fehler(0x7E0); } catch { f = kwp_fehler(0x01); }
if f.is_empty() { print("Motor: keine Fehler"); } else { for x in f { print(x); } }
