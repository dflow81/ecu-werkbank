#!/usr/bin/env python3
"""Beispiel: ECUWerkbank-Datenbank über die REST-Schnittstelle abfragen.

1. In einem Terminal:  ecu-werkbank api
2. Die angezeigte Adresse (mit Schlüssel) hier übergeben:
   python3 api_abfrage.py http://127.0.0.1:8787/<schluessel>/api/v1/

Nur Python-Standardbibliothek. Die Schnittstelle liest nur.
"""
import json
import sys
import urllib.request


def hole(basis: str, pfad: str):
    with urllib.request.urlopen(basis + pfad, timeout=5) as r:
        return json.load(r)


def main() -> None:
    if len(sys.argv) != 2:
        print(__doc__)
        sys.exit(1)
    basis = sys.argv[1].rstrip("/") + "/"
    status = hole(basis, "status")
    print(f"Schema {status['schema_version']}, Programm {status['programm']}")
    for q in hole(basis, "quellen"):
        print(f"  Quelle {q['id']}: {q['art']} {q['name']} ({q['eintraege']} Einträge, Lizenz {q['lizenz'] or '–'})")
    for h in hole(basis, "hersteller"):
        geraete = hole(basis, f"hersteller/{h['id']}/steuergeraete")
        print(f"{h['name']}: {len(geraete)} Steuergeräte")
        for g in geraete[:5]:
            print(f"  {g['adresse']:>3}  {g['name']}")


if __name__ == "__main__":
    main()
