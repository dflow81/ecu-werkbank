# ECUWerkbank

🇩🇪 [Deutsche Version](README.md)

**Diagnostics & coding for German cars** — an open tool on top of **SocketCAN**, written in Rust.
Suitable for vehicles from VW, Audi, Seat and Škoda. An independent implementation of the
published standard protocols:

- **ISO 15765-2** (ISO-TP) – transport, implemented from scratch
- **ISO 14229** (UDS) – diagnostic services
- **ISO 15765-4** – CAN addressing (OBD)

> **New name:** The project used to be called **vag-diag**. ECUWerkbank takes over existing
> data (vehicles, scans, backups, your own labels) automatically on first start; old data
> backups can still be restored.

No proprietary code, no licence-bound hardware. Works with any SocketCAN adapter
(CANable/candleLight, PCAN, MCP2515 HAT, …).

> **⚠ Use at your own risk.** ECUWerkbank can write to control units (coding,
> adaptations, service reset, basic settings, output tests). Wrong values can change or
> disable functions of the vehicle. Before every write there is a confirmation (in the
> terminal: without `--ja` only a preview), coding and adaptations are backed up first.
> Still: only with the car stationary, and only if you know what the value does. There is
> no warranty (see licence).
>
> **Odometer:** ECUWerkbank reads the mileage but **never** writes it — every write path
> rejects the mileage identifiers (odometer tampering is a criminal offence, e.g. § 22b StVG
> in Germany).

## Manual

Detailed guide with pictures (simple language):
[English: docs/handbuch/ecu-werkbank-Manual.pdf](docs/handbuch/ecu-werkbank-Manual.pdf) ·
[Deutsch: docs/handbuch/ecu-werkbank-Handbuch.pdf](docs/handbuch/ecu-werkbank-Handbuch.pdf).
Rebuild: `python3 docs/handbuch/build_handbuch.py` (German) and
`python3 docs/handbuch/build_handbuch.py --en` (English). The pictures are screenshots of the
GUI with the Golf 6 simulation; `docs/handbuch/bilder_erzeugen.sh` creates an example vehicle
in an empty working folder and photographs all pages (`VAG_SPRACHE=en` for English pictures).
A single page: the GUI photographs itself and closes:
`VAG_TRANSPORT=golf6 VAG_HELL=1 VAG_FENSTER=1000x880 VAG_TAB=<page> VAG_FOTO=bild.ppm
VAG_FOTO_WARTEN=<seconds> ecu-werkbank-gui` (`VAG_HELL` = light theme, `VAG_FENSTER` =
window size; list of pages in the header of `build_handbuch.py`). Use your own
`VAG_DATA_DIR`/`VAG_LABELS` so no real data ends up in the picture.

## Languages (German, English, your own)

Top right in the GUI: **🌐 Language** selector (remembered). In the terminal:
`ecu-werkbank --sprache en …` for one call or `ecu-werkbank sprache --setzen en` permanently.
Without a choice the system language (`LANG`) applies, otherwise German. `VAG_SPRACHE=en`
overrides everything.

**Adding a new language (no programming, no rebuild):**

```
ecu-werkbank sprache --vorlage fr          # writes <data folder>/sprachen/fr.toml with all texts
#   translate on the right:  "Speichern" = "Enregistrer"   — empty = stays German
#   at the top enter  _name = "Français"
ecu-werkbank sprache                       # shows all languages and how much is translated
```

Placeholders like `{n}` or `{a}` must stay in the translation. The same template also
completes an existing language when new texts have been added (the previous file is saved
as `.toml.bak`). To build it in permanently: put the file into `lang/<code>.toml` and add it
to `EINGEBAUT` in `src/i18n.rs`.

For developers: texts are written in German in the code. `tr!("…")` translates (with
placeholders: `tr!("{n} Fehler", n = anzahl)`), `trs!("…")` returns `&'static str`, `trn!("…")`
marks texts in fixed tables (displayed with `i18n::t`), `platzhalter!` is the same without
translation (for paths and file formats). `build.rs` collects all marked texts for the
template; a test checks that the placeholders in `lang/en.toml` match.

When a German text changes or a function is removed, the old translation remains as an
“orphaned” entry. Show or remove them (with backup `.toml.bak`; order and comments of the
file are kept):

```
ecu-werkbank sprache --verwaist   --datei lang/en.toml
ecu-werkbank sprache --aufraeumen --datei lang/en.toml
```

Still German (also in English): plain texts of fault codes, names of control units,
readiness and emission test names, the fault status in saved scans, the guide and lexicon
contents (guided troubleshooting, basic settings lexicon) and the descriptions in `--help`.
Command names and options in the terminal are German, too (e.g. `--ja` = yes, `--neu` = new).

Saved data (scans, maintenance items, trend names, folder names) deliberately stays German
so that comparisons and old files work in every language. When the code recognises
particular errors (TP 2.0 channel briefly blocked, safety stop of the controller, missing
freeze frame data), it does so by a fixed error kind — never by the translated text.

## Using the GUI

The GUI starts with a **start page**: an overview of the active vehicle (mileage, last
scan, maintenance due, trend hints) and tiles for the most common tasks (quick check,
fault memory PDF, check connection, troubleshooting, live values, battery test, mixture
check, used car check, vehicle record). Everything else is sorted into four groups:

| Group | Pages |
|---|---|
| 🔧 Diagnostics | Control units · Older control units (TP 2.0) · OBD-II · Coding · Checks & troubleshooting · Safety & assistance · Other makes |
| 📈 Measure | Live graph · Battery · Engine checks · Trip recorder · for diesels also target range and particulate filter |
| 🚘 Vehicle | Vehicles & maintenance · Service display · History & trends · Software versions · Coding backup · Retrofit |
| 🛠 Tools | Test run · CAN monitor · Scripts · Capture & learning aid · Vehicle database · Fault patterns & reference · Experts: routine & output test (UDS) |

Always reachable at the top: connection type, active vehicle, 🔌 connection,
✅ quick check, 📄 fault PDF, light/dark.

## Try it without an adapter (demo vehicle)

A simulated vehicle is built in (UDS, TP 2.0/KWP2000 and OBD-II; values change like on a
drive, fault memories can be cleared):

```bash
./target/release/ecu-werkbank -t demo autoscan
./target/release/ecu-werkbank -t demo obd-info
./target/release/ecu-werkbank -t demo kwp-block --address 0x01 --block 1,2 --count 0
VAG_TRANSPORT=demo ./target/release/ecu-werkbank-gui      # or choose “Demo vehicle” at the top of the GUI
```

## Installing

```bash
./packaging/install.sh          # for your user (~/.local), no sudo; entry in the start menu
./packaging/install.sh --can    # additionally: set CAN adapters to 500 kbit/s automatically when plugged in (sudo)
./packaging/build-deb.sh        # or build a .deb package → sudo apt install ./target/ecu-werkbank_*.deb
./packaging/build-appimage.sh   # or build an AppImage → target/ecu-werkbank-<version>-x86_64.AppImage
```

**AppImage:** a single file, runs without installation. Without an argument the GUI
starts, `… .AppImage ecu-werkbank scan` runs the terminal program, `… .AppImage mockecu can0`
the test control unit. Licences, credits and the manual are inside the AppImage under
`usr/share/doc/ecu-werkbank/`. It needs at least the glibc of the build machine (built on
Debian 13: glibc 2.39 → Ubuntu 24.04, Linux Mint 22 and newer). The packing tool
`appimagetool` is downloaded by the script on first use from github.com/AppImage/appimagetool;
`--nur-appdir` only assembles the folder without packing.

After `--can` or the .deb the `sudo ip link …` commands are no longer needed: plug in the
adapter, done. Labels then live in `~/.config/ecu-werkbank/labels` (the project folder
`./labels` takes precedence, `VAG_LABELS` overrides both), history and coding backups in
`~/.local/share/ecu-werkbank/`.

### Windows (light version)

```bash
./packaging/build-windows.sh    # cross-compiled on Linux → target/ecu-werkbank-<version>-windows-x86_64.zip
```

Unpack the ZIP, start `ecu-werkbank-gui.exe` — no installation. Windows has no SocketCAN,
therefore: **ELM327** (COM port or Wi-Fi), **demo vehicle** and everything without a car
(vehicles, maintenance, parts, PDFs, captures, vehicle database). **Not**: CAN adapters and
thus TP 2.0 (older control units). Data lives in `%APPDATA%\ecu-werkbank`. Needed once for
building: `rustup target add x86_64-pc-windows-gnu` and `sudo apt install mingw-w64`. CAN
access is encapsulated in `src/can.rs` — a Windows driver (e.g. slcan or gs_usb) could be
plugged in there later.

### macOS (light version, untested)

Same scope as on Windows (ELM327, demo, everything without a car; no CAN adapter). Built on
a Mac with `./packaging/build-mac.sh` → `target/ecu-werkbank-<version>-macos.zip`
(`ecu-werkbank.app` for Intel and Apple silicon). Without your own Mac, GitHub builds it:
tab “Actions” → “Mac-Version” → “Run workflow”. The app is not registered with Apple: on
first start right-click → “Open”. Data lives in `~/.local/share/ecu-werkbank/`.

## Building

```bash
cargo build --release
```

## Testing without a car (virtual CAN)

```bash
sudo modprobe vcan
sudo ip link add dev vcan0 type vcan
sudo ip link set up vcan0

./target/release/ecu-werkbank -i vcan0 scan
```

## With a real adapter

```bash
# Bitrate usually 500 kbit/s (powertrain CAN); 100 kbit/s on some buses
sudo ip link set can0 type can bitrate 500000
sudo ip link set up can0

./target/release/ecu-werkbank -i can0 scan
./target/release/ecu-werkbank -i can0 discover --seconds 5
./target/release/ecu-werkbank -i can0 dtc   --tx 0x7E0 --rx 0x7E8
./target/release/ecu-werkbank -i can0 rdid  --tx 0x7E0 --rx 0x7E8 --did 0xF190   # VIN
./target/release/ecu-werkbank -i can0 clear --tx 0x7E0 --rx 0x7E8
```

### Using the GUI

As with a workshop diagnostic tool: **Diagnostics → Control units**, read all control
modules once, then click one. Below it all functions of this control unit appear: read
/clear faults, identification, measured values, coding, adaptation, basic settings and
output tests (depending on what the control unit supports). A click jumps to the right
page with the address filled in.

### Which adapter can do what? (CAN interface vs. ELM327)

**CAN interface** = SocketCAN adapter (e.g. USB-CAN, `-i can0`). **ELM327** = cheap OBD
adapter via USB, Bluetooth or Wi-Fi (`-t elm327`).

| Function | CAN interface | ELM327 | Note |
|---|:---:|:---:|---|
| **OBD-II** (faults, readiness/emissions, live values, VIN, freeze frame) | ✅ | ✅ | manufacturer-independent |
| **UDS control units** (read/clear fault memory, identification, measured values by identifier) | ✅ | ⚠️ | ELM327: only modules with a known CAN ID (OBD addresses, label file, vehicle profile); reading tested on the Golf 6 |
| Coding, adaptations, login (UDS) | ✅ | ⚠️ | ELM327: reading coding tested on the Golf 6, writing not yet tested |
| Service reset, particulate filter, mileage, software versions (UDS) | ✅ | ⚠️ | ELM327: reading works, writing untested |
| Quick check, battery test, used car check | ✅ | ✅ | via OBD-II |
| **Auto-scan of all control units** | ✅ | ⚠️ | ELM327: only OBD addresses + known modules, no manufacturer sweep; also finds pure OBD modules |
| Other makes: search control units | ✅ | ⚠️ | ELM327: only OBD addresses |
| **TP 2.0 / KWP2000** (older vehicles, e.g. Golf 6 comfort modules) | ✅ | ❌ | ELM327 cannot do TP 2.0 |
| Measuring blocks, ⭐ favourites, block scan | ✅ | ❌ | need TP 2.0 |
| **Gateway installation list and coding** (directly from the gateway) | ✅ | ❌ | works with the CAN interface, tested on the Golf 6; the ELM327 cannot do TP 2.0 |
| Engine checks (timing chain, idle …), trip recorder | ✅ | ❌ | read measuring blocks (TP 2.0) |
| Basic settings on TP 2.0 modules (function 04) | ✅ | ❌ | enabled only after your own capture; throttle body adaptation tested on the Golf 6 |
| Live graph / scope | ✅ | ⚠️ | ELM327: only OBD and UDS channels, no measuring blocks |
| Record **CAN captures**, learning aid from captures | ✅ | ❌ | needs direct bus access |
| Auto-detection (car on/off, choose vehicle automatically) | ✅ | ❌ | reads the adapter counters |
| Adapter test mode, CAN state (BUS-OFF …) | ✅ | ❌ | SocketCAN only |
| Analyse captures, vehicle database, own texts, coding helper, maintenance, PDFs | ➖ | ➖ | offline, no adapter needed |

✅ works · ⚠️ works with limits · ❌ does not work · ➖ needs no adapter

> **Test status:** Both adapters are tested on the Golf 6. The ELM327 (USB, v1.5, FTDI
> chip) since 14.11.2026 **for reading**: OBD-II (VIN, readiness, faults, live values),
> control unit search and the UDS control units with manufacturer address (instrument
> cluster, airbag, Climatronic, radio: fault memory, coding and extended identification).
> On the CAN interface additionally compared with a workshop diagnostic tool via captures
> (15.11.2026): codings, fault types, measured value formulas and the gateway installation
> list. Writing via ELM327 is not yet tested. For older vehicles (TP 2.0) there is no way
> around the CAN interface. Everything can be tried beforehand without a car with `-t demo`.

### Experience on a real vehicle (Golf 6, 2010)

- Adapter DSD TECH SH-C31x (candleLight/gs_usb): sometimes starts in update mode after
  plugging in (`lsusb` shows “STM Device in DFU Mode”). Get it back:
  `sudo dfu-util -a 0 -s 0x08000000:leave`. This firmware does not know `restart-ms`.
- Bus-off right after the first send → CAN high/low swapped.
  OBD: pin 6 = high, pin 14 = low, pin 5 = ground.
- TP 2.0 logical addresses are NOT the usual diagnostic addresses (Golf 6:
  0x09 power steering, 0x1D parking aid, 0x1F gateway, 0x20 central electrics,
  0x22–0x25 doors, 0x2A steering column, 0x43 trailer). The names therefore come from the
  answer of the control unit.
- Instrument cluster, airbag, climate control and radio already speak UDS (0x714, 0x715,
  0x746, 0x76C; answer = request + 0x6A). The scan searches 0x700–0x7DF (≈ 3 s).
- Control units require 10 ms between frames (T3), report “busy” (0x21) or only know
  0x22 instead of 0x1A — all handled.
- Polo 6R (2010): ABS (ESP8.2i) and power steering (TRW) sit on a K-line behind the
  gateway — the scan reads them through the tunnel (see “Further connections”). With that
  ECUWerkbank finds all 12 control units there, like the workshop tool.

### Quick scan, fault texts, automatic adapter

- **Quick scan:** After the first full scan, `fahrzeuge/<VIN>/profil.toml` records which
  control units the car has. From then on the scan reads the VIN first and only queries
  those (Golf 6: ~10 s instead of ~40 s). Full sweep: `report --voll` or the “Full scan”
  checkbox in the GUI. Purchase check and retrofit always search everything.
- **Status of old fault codes** (TP 2.0/KWP) in plain text according to ISO 14230:
  sporadic / current / being tested, warning lamp, fault type number.
- **Your own fault texts:** `ecu-werkbank fehlertext --code 01627 --text "…"` or GUI
  Tools → Fault patterns → “Own fault texts”. Applies in all views and PDFs.
  Many at once: `ecu-werkbank fehler-geruest` reads **all possible** fault codes from all
  UDS control units (read only) and writes a file to fill in; afterwards
  `ecu-werkbank fehlertexte-uebernehmen <file>`. TP 2.0 does not know this query.
- **Import auto-scan logs:** Vehicle → History & trends → “📥 Import auto-scan logs”
  (or `ecu-werkbank autoscan-import <files…>`, dry run with `--probe`). Auto-scan logs of a
  workshop diagnostic tool as text files (German/English, also old Latin-1 files) go into the
  vehicle history with mileage, date and all freeze frame data per fault (recognised by the
  VIN); their fault texts become your own fault texts — only for codes without your own
  text. Logs and texts stay in your data folder.
- **Readiness assistant (OBD-II):** a driving guide for each open emission test, progress
  since clearing (km, warm-ups, engine run time) and — if the car reports it — which test has
  already been completed during the current drive.
- **Names for measuring blocks and adaptations in one go:** Diagnostics → Older control
  modules (TP 2.0) → “Read all blocks (1–255)” → “✏ Enter names for all blocks and fields”
  shows each field with current value and unit for naming. Vehicle → Coding backup →
  Adaptations → “✏ Name identifiers” lists all identifiers of the newest adaptation backup
  (without the car); the names then appear in the comparison
  (`<data folder>/kennungstexte.toml`).
- **Your own measured value texts (TP 2.0):** name per block and per field, e.g.
  `ecu-werkbank messwerttext --adresse 01 --block 1 --feld 2 --text "Coolant temperature"`
  or in the GUI with ✏ at the measuring block. Without your own text ECUWerkbank shows the
  kind from the conversion formula with “~” (e.g. “~Temperature”). Stored in
  `<data folder>/messwerttexte.toml`; applies in the GUI, `kwp-block`, test run and graphs.
- **Block scan:** `ecu-werkbank -i can0 kwp-blockscan --address 01` reads all measuring
  blocks (1–255, read only) and shows value and kind per field — so you find out which blocks
  your control unit has at all.
- **Name suggestions (engine 01):** `ecu-werkbank -i can0 messwert-vorschlaege` checks the
  commonly known layout (blocks 1–4 basic values, 115 boost pressure) against your control
  module: ✓ only if the conversion formula matches the expected kind of value. With `--ja`
  the matching names are taken over; your own texts stay.
- **Fault texts without label files:** order: your own texts → label files (optional) →
  built-in OBD list. Engine codes are converted to OBD codes: 16384–17383 = P0000–P0999,
  17408–18407 = P1000–P1999 (older engines), and with newer engine/gearbox control units
  (MED17) the number is the OBD code itself (Golf 6: **08851 = 0x2293 = P2293**).
  Manufacturer-specific component codes of other control units (01627, 00097 …) have no
  open list — use your own texts for those (GUI: ✏ button right next to codes without text).
  UDS faults show the standardised fault type in plain text (e.g. “Fault type 13: open
  circuit”).
- **Bring the adapter back from update mode automatically:**
  `./packaging/install.sh --can --dfu <serial number>` (the connection assistant shows the
  serial number while the adapter is in update mode).

### Start page, control unit list, connection hints

- **Fault overview** on the start page: new / still present / gone since the previous scan,
  “was there before” and “keeps coming back” (≥ 3 scans).
- **Control units of this vehicle** (Diagnostics → Control units): all modules from the
  last scan with name and part number; click → read faults or identification live, whether
  UDS or TP 2.0.
- **Connection assistant** evaluates the CAN state: bus-off (wires swapped?), error-passive
  (no connection / ignition off), error-active without answer (car asleep).

### Test run on the vehicle

`ecu-werkbank testlauf --km <odometer reading>` (GUI: Tools → Test run) reads in one pass —
read only — connection, VIN, all control units with fault memory, measuring blocks 1–30 of
the engine (with raw data) and searches the mileage in the UDS identifiers of the instrument
cluster (0x2200–22FF, 0x0200–02FF, 0x1000–10FF). The log ends up in the vehicle folder under
`berichte/testlauf-….txt`.

### GUI

- Traffic light at the top next to the interface: ready / no connection / bus-off / update
  mode / no adapter (click opens the connection assistant).
- Missing characters (→ ✓ ≤ …) come from the system fonts DejaVu Sans or Noto Symbols.
- The control unit sidebar only appears with measured values and coding.

### Test the adapter without a car (test mode)

In loopback mode the adapter hears its own messages; the demo control unit then runs
directly on the real adapter. ECUWerkbank detects the mode and switches off the duplicate
Linux echo itself.

```bash
sudo ip link set can0 down
sudo ip link set can0 type can bitrate 500000 loopback on
sudo ip link set can0 up
./target/release/mockecu can0 &
./target/release/ecu-werkbank -i can0 autoscan

# back for the car
sudo ip link set can0 down
sudo ip link set can0 type can bitrate 500000 loopback off
sudo ip link set can0 up
```

## TP 2.0 / KWP2000 (older CAN vehicles ~2004–2010)

```bash
./target/release/ecu-werkbank -i can0 detect                       # UDS or TP 2.0?
./target/release/ecu-werkbank -i can0 kwp-ident --address 0x01     # part number, description
./target/release/ecu-werkbank -i can0 kwp-dtc   --address 0x01     # fault memory (5 digits + P code)
./target/release/ecu-werkbank -i can0 kwp-clear --address 0x01
./target/release/ecu-werkbank -i can0 kwp-block --address 0x01 --block 1 --count 0   # live, Ctrl-C
./target/release/ecu-werkbank -i can0 tp20 --address 0x01 --request 1a9b             # raw KWP request
```

In the GUI (`./target/release/ecu-werkbank-gui`) there is the tab **“Older control units
(TP 2.0)”** for this: search control units (runs in the background), identification, read
/clear fault memory and live measuring blocks. Preset the start interface with
`VAG_IFACE=can0`.

**⭐ Favourite measured values:** Mark frequently used blocks with the **star (☆ → ★)** at
the open measuring block. They then appear as a bar above the block — one click opens and
reads the block immediately, without typing address and number every time. Remove with the
**✖** in the bar or the star again. The list applies across devices (with several cars the
addresses of the respective model are in it) and is stored in `messwert_favoriten.toml` in
the data folder.

**⚙ Basic settings (function 04) on TP 2.0 control units:** Which requests the manufacturer
sends for this over TP 2.0 is not reliably published. ECUWerkbank does not guess, but
**learns the sequence from your own capture** — until then sending is **locked** for this
control unit:

1. Tab “Capture & learning aid”: start recording (CAN interface and the cable of a workshop
   diagnostic tool connected to the car at the same time via an OBD Y-cable).
2. With the diagnostic tool, switch on the basic setting (function 04) of a group on the
   control unit, let it run for a few seconds, switch it off.
3. Stop recording. In the tab “Older control units (TP 2.0)” under “⚙ Basic settings”
   enter the same group, “📂 Analyse capture…”, check the result and accept it.

After that every group of this control unit can be started (with confirmation), the
values run live, “⏹ Off” ends it. Only what the control unit confirmed in the capture is
taken over; when starting only the group number is exchanged. The sequence is stored in the
vehicle folder (`grundeinstellung.toml`) — a sequence learned on the Golf never applies to
another car. Tested on the Golf 6: throttle body adaptation (engine, group 60) — the
switch-on that only comes while values are being polled (“ON”) is learned as well.

In the terminal:

```bash
ecu-werkbank -f <vehicle> kwp-grundeinstellung --address 01 --gruppe 60 --lernen mitschnitt.log  # learn
ecu-werkbank -f <vehicle> kwp-grundeinstellung --address 01 --gruppe 60 --ja --sekunden 20        # run
```

In the tab **Tools → “Experts: routine & output test (UDS)”** (UDS control units, routine
0x31 and output control 0x2F) there is a confirmation before every send; in the terminal
`routine` and `iocontrol` without `--ja` only show what would be sent.

`autoscan` and `report` search **both worlds**: UDS (labels + OBD range) and TP 2.0
(addresses 0x01..0x7F, approx. 8 s). Skip with `--no-tp20`.

> **Important:** TP 2.0 has not been officially published by the manufacturer. The
> implementation in [`src/tp20.rs`](src/tp20.rs) is based on public community reverse
> engineering documentation. Confirmed are setup ID 0x200, opcodes 0xC0/0xD0 and the
> keep-alive < 1 s. Byte offsets marked `TODO(verify)` (ID encoding of the setup response,
> timing parameters, length prefix) **must be checked against a real capture of your
> vehicle** before relying on them:
> ```bash
> candump -L can0 > handshake.log   # while a channel is being opened
> ```

### Sources on the TP 2.0 structure
- Autosport Labs Forum – TP 2.0 reverse engineering:
  https://forum.autosportlabs.com/viewtopic.php?p=27858
- OVMS developer list – tested TP 2.0 engine (SEAT Mii):
  https://lists.openvehicles.com/archives/list/ovmsdev@lists.openvehicles.com/thread/FDVRIIZNUR765CESHVMECH73E4DF4EOC/
- NI Automotive Diagnostic Command Set – keep-alive/timing:
  https://www.ni.com/docs/en-US/bundle/automotive-diagnostic-command-set-toolkit-c-api-ref/page/ndvwtpconnectiontest.html

## Fault memory as PDF

```bash
./target/release/ecu-werkbank -i can0 report --out fehlerspeicher.pdf --note "Golf 6, B-XY 123, 154,300 km"
./target/release/ecu-werkbank -t demo report --out beispiel.pdf      # example without a car
```

Queries all control units (UDS and TP 2.0) and creates an A4 report: date, VIN, note,
overview of all control units and per control unit the fault codes with your plain texts
and the status (current / stored / sporadic). With `--out file.txt` a text file is created
instead. In the GUI: **“📄 Fault PDF”** at the top (note in the tab “Control units”), then
“Open PDF” in the status line.

## Installation list (installed control units)

```bash
./target/release/ecu-werkbank -i can0 verbauliste                       # live from the car (full sweep)
./target/release/ecu-werkbank -i can0 verbauliste --no-tp20 --out liste.txt
./target/release/ecu-werkbank -f "B XY 123" verbauliste --gespeichert   # without the car, from the last scan
```

Shows all installed control units, sorted by system (engine & drivetrain, chassis &
assistance, body & comfort …), each with part number and status (OK / fault / memory not
readable), plus a summary. Corresponds to the installation list of common workshop
diagnostics — the names come from the component identification of the control unit, not
from a guessed number translation. In the GUI in the tab **“🔧 Control units”** under
**“📋 Installation list by system”** (with “💾 save as text file”).

**Directly from the gateway** (TP 2.0, read only): which control units the gateway knows as
installed and which have a stored fault — fast, without querying every control unit
separately:

```bash
./target/release/ecu-werkbank -i can0 gateway-verbauliste
```

In the GUI under **“📋 Installation list by system” → “🔌 Read installation list from the
gateway”**. `kwp-coding --address 1f` reads the gateway coding. Compared on the Golf 6 with a
workshop diagnostic tool via capture (15.11.2026): the same 20 control units, the same 4
with faults, the same coding.

## Quick check, battery test, maintenance

```bash
./target/release/ecu-werkbank -f "B XY 123" -i can0 check       # traffic light: faults, new/fixed, readiness, voltage, maintenance → PDF
./target/release/ecu-werkbank -f "B XY 123" -i can0 batterie    # rate resting, cranking and charging voltage → PDF
./target/release/ecu-werkbank -f "B XY 123" wartung-vorlage     # oil, filters, spark plugs, brake fluid, inspection …
./target/release/ecu-werkbank -f "B XY 123" wartung-erledigt --name öl --km 154300
./target/release/ecu-werkbank -f "B XY 123" wartung             # what is due?
./target/release/ecu-werkbank -f "B XY 123" km 154300
./target/release/ecu-werkbank -f "B XY 123" teil "Brake discs + pads front" --km 154300 --teilenummer 5K0698151 --notiz "ATE" --preis 189,90
./target/release/ecu-werkbank -f "B XY 123" teile               # replaced parts with date, km and price
./target/release/ecu-werkbank -f "B XY 123" tanken 42,5 --km 154800 --preis 74,30   # --teil = not filled up
./target/release/ecu-werkbank -f "B XY 123" tankbuch            # fuel log with consumption (l/100 km, full to full)
./target/release/ecu-werkbank -f "B XY 123" kosten              # total, per month, per 1000 km, per year
./target/release/ecu-werkbank -f "B XY 123" kalender            # maintenance dates as .ics (reminder 1 week ahead)
./target/release/ecu-werkbank -f "B XY 123" reparatur "Ignition coil cyl. 3 replaced, 45 €"
./target/release/ecu-werkbank -f "B XY 123" akte                # vehicle record as PDF
./target/release/ecu-werkbank -f "B XY 123" akte-zip --out /media/usb/golf.zip
./target/release/ecu-werkbank datensicherung --ziel /media/usb/ecu-werkbank --tage 7 --behalten 8   # set up
./target/release/ecu-werkbank datensicherung                    # back up now (all vehicles + own labels)
./target/release/ecu-werkbank datensicherung-zurueck /media/usb/ecu-werkbank/ecu-werkbank-sicherung-….zip
```

GUI: **“✅ Quick check”** at the top (result window with traffic light), tab **“Battery”**
(live curve while cranking), in the tab **“Vehicles”** maintenance plan (“✔ done today”),
mileage, **replaced parts** (part, part number, km, date, price, note), **fuel log**
(consumption by the full-to-full method), **costs** (total, per month, per 1000 km, per year),
**“📅 Dates to calendar”** (.ics file for the phone; overdue dates land on today, items only by
km are estimated from the mileage so far), **vehicle record (PDF)** with costs and consumption
and **Save as ZIP**. If a part has the same name as a maintenance item (e.g. “Air filter”,
selectable via ▼), the item is recorded as well.
When the program starts, a hint appears if something is due on the active vehicle.
**Data backup:** at the top under “Vehicles & maintenance” → “💾 Data backup” choose a folder
(USB stick, cloud folder). The GUI then backs up by itself on start when the last backup is
older than configured and the folder is reachable; old backups are cleaned up except for the
newest ones. “Restore backup” brings back missing files without overwriting existing ones.

More helpers in the GUI:

- **🔎 next to every fault code**: web search for code + vehicle model.
- **Freeze frame data** for faults (UDS snapshot: e.g. mileage, engine speed) and the
  **OBD freeze frame** are in the fault PDF; in the OBD-II tab via a button.
- **Readiness assistant** (OBD-II): follows live which emission tests are still open, with
  driving hints, and reports when everything is done.
- **Target values / traffic light**: values outside the range turn red. Your own limits in the
  labels (generous defaults are built in for some OBD values):

  ```toml
  [[sollwert]]
  quelle = "kwp"   # kwp (measuring block) | uds (DID) | obd (PID)
  id = 1           # block / DID / PID
  feld = 2         # KWP only: field 1–4
  min = 80.0
  max = 105.0
  ```

- **Scope**: ⚠ **warn at limit** (sound + message, records the 30 s before and after),
  🚗 **trip recorder** (starts with the engine running, saves CSV + PDF when switched off),
  🕓 **gauges** (round instruments), 📱 **phone display** (scan the QR code with the phone
  camera or open the address in the phone browser — same Wi-Fi; read only), **load CSV**
  (overlay recordings).

### Phone display — no app, in the browser

```bash
./target/release/ecu-werkbank -i can0 handy                 # or in the GUI: Scope → 📱 Phone display
```

**Scan the QR code with the phone camera** (in the GUI in a small window, in the terminal as
text graphics) or open the address (e.g. `http://192.168.1.50:8088/3f9a…/`) in the phone
browser — with the key at the end, without it the server shows nothing; phone and laptop on
the same Wi-Fi — in the car the easiest is to switch on the **phone hotspot** and connect the
laptop to it. If the `ufw` firewall is active (default on Linux Mint), allow it once:

```bash
sudo ufw allow from 192.168.0.0/16 to any port 8088 proto tcp   # local network only
sudo ufw allow from 172.16.0.0/12 to any port 8088 proto tcp    # iPhone hotspot (172.20.10.x)
```

## Diagnostics: test results, mixture, trends, earlier repairs

ECUWerkbank is a diagnostic tool — it is not permanently plugged into the car. Everything is
stored per diagnostic session in the vehicle folder on the laptop; trends emerge from several
sessions.

```bash
./target/release/ecu-werkbank -i can0 obd-tests                 # mode 06: how close are catalyst, lambda sensors … to the limit? misfires per cylinder
./target/release/ecu-werkbank -f "B XY 123" -i can0 gemisch     # idle vs. 2500 rpm → air leak / fuel pressure / too rich
./target/release/ecu-werkbank -f "B XY 123" trends              # histories + hints of creeping deterioration
./target/release/ecu-werkbank -t demo --demo-muster "fehler=falschluft" gemisch --seconds 5   # demo
```

- **Test results (mode 06):** value, limits and margin per test; “tight” below 25 % margin; a
  cylinder with clearly more misfires is reported. Standardised, works on every petrol car
  with OBD-II. The engine control unit supplies the limits itself. **Manufacturer tests**
  (test number from 0x80, e.g. “Lambda sensor B1 S2 · test 0x81”) are not published — often
  counters or ratios that are normally close to a limit. They therefore only count as
  passed/failed, never as “tight”. Every quick check and purchase check stores all test
  values raw in the scan (`[[pruefwerte]]`: mid, tid, value, min, max) so they can be checked
  later.
- **Mixture assistant:** compares the mixture correction at idle and at raised engine speed.
  Lean only at idle ⇒ air leak; lean everywhere ⇒ fuel supply/mass air flow sensor; rich at
  idle ⇒ tank ventilation/injector. With displacement (tab Vehicles) additionally an air mass
  check. Guide values, not a component test. Diesels usually report no mixture values.
- **Trends:** quick check, battery test and mixture assistant store their values
  (`trends.toml` in the vehicle folder). From 3 sessions on ECUWerkbank detects e.g. falling
  battery voltage, growing mixture correction, rising misfires or a shrinking catalyst
  margin. Shown in the quick check, in the history (section trends, with graph) and in the
  vehicle record.
- **Earlier repairs:** If a fault code comes back, ECUWerkbank shows your repair note from
  back then — also from your other vehicles — below the code, in the fault PDF and as a
  warning in the quick check.

In the GUI: tab **“Diagnostics”**.

## Engine checks, trip recorder, adaptations

GUI: **Measure → Engine checks** and **Measure → Trip recorder**; adaptations on the
page **Vehicle → Coding backup**. All read only, via the measuring blocks of the engine
(TP 2.0, address 01). Which block/field it is for your engine you set once per vehicle
(`motorcheck.toml` in the vehicle folder); “Search candidates” or `kwp-blockscan` shows
suitable fields.

- **Timing chain** (a topic especially with the 1.4 TSI CAXA): camshaft phase position warm
  at idle, as a trend. The first measurement is the reference; if the value drifts by 2° or
  4°, a hint or a warning appears. Plus the standardised faults P0016/P0017/P0011/P0014.
- **Boost pressure / fuel pressure:** accelerate at full throttle in 3rd gear, ECUWerkbank
  records target and actual and rates the gap (boost 100/200 mbar, fuel 7 %/15 %; the first
  second counts as response time). Default for boost: block 115 field 3/4. Limits are own,
  cautious estimates.
- **Trip recorder:** blocks run along in a ring buffer; on a new engine fault, a limit
  (`003/4>40`) or “Mark”, pre- and post-trigger are saved as CSV (`rekorder/` in the vehicle
  folder).
- **Back up adaptations:** all readable identifiers 0x0100–0x04FF and 0x2000–0x24FF of every
  UDS control unit; the before/after comparison shows changed identifiers.

```bash
./target/release/ecu-werkbank -i can0 kette --feld 093/2          # remember field + measure
./target/release/ecu-werkbank -i can0 kette                       # repeat later
./target/release/ecu-werkbank -i can0 druckpruefung --art ladedruck --sekunden 20
./target/release/ecu-werkbank -i can0 rekorder --bloecke 1,2,3,115 --grenze "001/2>110"
./target/release/ecu-werkbank -i can0 anpassung-sichern
./target/release/ecu-werkbank anpassung-vergleich
```

## Used car check, troubleshooting, repair, connection, learning aid

```bash
./target/release/ecu-werkbank -i can0 kaufcheck --note "Advert XY"   # cleared faults? odometer? VINs? → PDF purchase advice
./target/release/ecu-werkbank -f WVWZZZ1KZAM000017 kaufcheck --aus-scan   # without the car: from the last saved scan (GUI: “📄 From last scan”)
./target/release/ecu-werkbank pruefplan P0301                         # guided troubleshooting
./target/release/ecu-werkbank -f "B XY 123" -i can0 report            # state before …
./target/release/ecu-werkbank -i can0 fehler-loeschen --ja            # … repair, clear, test drive …
./target/release/ecu-werkbank -f "B XY 123" -i can0 reparaturbericht --arbeiten "Ignition coil cyl. 3 replaced"
./target/release/ecu-werkbank -i can0 verbindung                      # check adapter/bitrate/control units
./target/release/ecu-werkbank lernen mit-aktion.log --vergleich ohne-aktion.log --out gelernt.toml
./target/release/ecu-werkbank -t demo --demo-muster "kauf=manipuliert" kaufcheck   # demo: manipulated car
```

- **Used car check:** “distance/warm-ups since faults were cleared” (recently cleared?),
  mileage from engine, instrument cluster and fault memory (fault at a higher mileage than
  the odometer ⇒ manipulation?), the same VIN in all control units (otherwise replaced),
  readiness, emission tests, battery. A plausibility check, not an expert report.
- **Guided troubleshooting:** test plans for mixture, misfires, catalyst, lambda sensors,
  mass air flow sensor, thermostat, boost pressure, EGR, tank ventilation, sensors, supply
  voltage, data bus; steps open the right tool, result as a repair note.
- **Repair mode:** back up before → repair → clear all faults → test drive (readiness) →
  check afterwards → **repair report** (PDF).
- **Connection assistant:** finds CAN interfaces, checks “up”, bitrate, ELM327 port and
  permissions, and whether control units answer — with a command to fix it.
- **Learning aid:** suggests label entries from a workshop capture (output functions, coding
  bits, adaptations, measured values) — this makes “control unit adjusts itself” and coding
  labels possible for your car.

In the GUI: tab **“Diagnostics”** (used car check, troubleshooting, repair mode),
**“🔌 Connection”** at the top, learning aid in the tab **“Capture”**.

## Software versions, retrofit, coding helper, particulate filter, fault patterns, safety

```bash
./target/release/ecu-werkbank -f "B XY 123" -i can0 software                              # part/SW/HW versions, comparison with last time
./target/release/ecu-werkbank -f "B XY 123" -i can0 nachruestung --name AHK --phase vorher
./target/release/ecu-werkbank -f "B XY 123" -i can0 nachruestung --name AHK --phase nachher --notiz "Westfalia, coded"
./target/release/ecu-werkbank -i can0 codier-bit --tx 0x7E0 --rx 0x7E8 --byte 1 --bit 2 --wert ein --ja   # with backup before
./target/release/ecu-werkbank -f "B XY 123" -i can0 dpf                                   # particulate filter (identifiers in the vehicle, [dpf])
./target/release/ecu-werkbank -f "B XY 123" wissen --code P0300 --neu "usually ignition coil"   # own fault pattern
./target/release/ecu-werkbank -f "B XY 123" wissen --code P0300                           # matches
./target/release/ecu-werkbank -i can0 sicherheit                                          # airbag, ESP, assistance (read only)
```

(`vorher` = before, `nachher` = after, `ein` = on.)

- **Software versions:** part number, software/hardware version, system name and serial
  number per control unit; the comparison shows replaced, reprogrammed, new or missing
  control units. Also in the vehicle record.
- **Retrofit assistant:** back up scan + coding before, install, compare afterwards (new
  control units, new faults, coding changes) → PDF.
- **Coding helper** (page “Coding”): choose a control unit from the last coding backup or
  read it from the control unit, see and switch all bytes as bits, **give every bit and byte
  your own name** (✏), list of changed bits, writing with automatic backup and check. See
  “Own coding texts”.
- **Particulate filter:** soot load, km since regeneration, differential pressure, exhaust
  temperature with traffic light and driving hints. The identifiers are engine-specific (from
  capture/learning aid) and are stored per vehicle.
- **Fault patterns:** your own experience per model and code; appear in troubleshooting, plus
  repairs on the same models.
- **Safety & assistance:** airbag, brakes/ESP, steering, parking brake and assistance systems
  grouped, with faults and freeze frame data — read only.

## Target range: hold engine speed (or other values) or let it oscillate

Patterns: `halten:2500` (hold), `schwanken:2000:3000:10` (oscillate between 2000 and 3000,
every 10 s), `rampe:800:3000:20` (ramp from 800 to 3000 in 20 s).

**1. Driver assistant** — you hold the value yourself with the accelerator; large display
“MORE THROTTLE / LESS THROTTLE / IN RANGE”, a sound if off for longer, share “in range”, also
on the phone. Works on every car:

```bash
./target/release/ecu-werkbank -i can0 ziel --muster halten:2500 --toleranz 150
./target/release/ecu-werkbank -i can0 ziel --pid 0D --muster schwanken:50:80:30 --toleranz 5   # speed
```

**2. Control unit adjusts itself** (workshop function, e.g. raise idle speed). **Only when
stationary.** Automatic stop when driving, exceeding the limit, communication errors, time
limit, Ctrl-C or EMERGENCY STOP — afterwards the control unit gets control back. The
identifiers differ per engine control unit and come from a **capture** (a workshop tool
raises the engine speed, `ecu-werkbank record` records, `decode-log` shows the output
control command). Then enter them in a label file:

```toml
[[stellfunktion]]
name = "Raise idle speed"
art = "uds-io"        # uds-io (0x2F output control) | uds-routine (0x31)
tx = 0x7E0
rx = 0x7E8
id = 0x....           # from the capture
bytes = 2
faktor = 1.0          # raw value = (target - offset) / factor
offset = 0.0
min = 800.0           # safety limits
max = 1500.0
rueckmeldung_pid = 0x0C
max_dauer_s = 300
einheit = "1/min"
```

```bash
./target/release/ecu-werkbank -i can0 stellen --funktion leerlauf --muster schwanken:900:1400:8 --seconds 60       # only shows the plan
./target/release/ecu-werkbank -i can0 stellen --funktion leerlauf --muster schwanken:900:1400:8 --seconds 60 --ja  # runs it
```

**3. Simulation** — the demo vehicle follows adjustable patterns:

```bash
./target/release/ecu-werkbank -t demo --demo-muster "drehzahl=schwanken:2000:3000:10" ziel --muster halten:2500
./target/release/ecu-werkbank -t demo --demo-muster "geschwindigkeit=halten:0" stellen --funktion demo --muster halten:1500 --ja
```

In the GUI: tab **“Target range”** (assistant, control unit function with EMERGENCY STOP,
simulation).

## Vehicle record (one folder per vehicle)

Everything about a car automatically ends up in one folder, named after the **VIN**:

```text
~/.local/share/ecu-werkbank/fahrzeuge/WVWZZZ1KZAW000001/
  fahrzeug.toml   berichte/   historie/   codierung/   aufzeichnungen/   scope/   mitschnitte/
```

You assign the **number plate** (and a description like “Golf 6”) to the VIN — that makes the
vehicle easier to find: in the list, in the PDF and with `--fahrzeug "B XY 123"`. Older cars
without a readable VIN temporarily get a folder named after the number plate; as soon as the
VIN is known, it is renamed.

```bash
./target/release/ecu-werkbank fahrzeug-neu --kennzeichen "B XY 123" --name "Golf 6 1.4 TSI"
./target/release/ecu-werkbank -f "B XY 123" -i can0 report          # PDF → berichte/, scan → historie/
./target/release/ecu-werkbank -f "B XY 123" -i can0 kwp-block --address 0x01 --block 1,2 --count 0 --csv fahrt.csv
./target/release/ecu-werkbank -f "B XY 123" csv-pdf ~/.local/share/ecu-werkbank/fahrzeuge/*/aufzeichnungen/fahrt.csv
./target/release/ecu-werkbank fahrzeuge                               # overview of all vehicles
```

In the GUI: choose the **active vehicle** at the top (remembered), tab **“Vehicles”** to
create/edit and “Open folder”. Fault PDF, CSV recordings, coding backups and **scope PDF**
(“📄 As PDF” in the Scope tab: overview + one chart per measured value with min/max/mean) are
saved there without asking.

## Vehicle history and comparison

Every `report` (and every fault PDF in the GUI) is automatically saved under the VIN and
compared with the last scan: **new**, **fixed**, **still present**, and whether a new fault
was there before (**recurring**). This is also in the PDF.

```bash
./target/release/ecu-werkbank history                       # all vehicles
./target/release/ecu-werkbank history --vin WVWZZZ1KZAW000001
./target/release/ecu-werkbank compare --vin WVWZZZ1KZAW000001   # newest two scans
```

In the GUI: **Vehicle → History & trends** — at the top mileage and fault history, below
choose two scans, see the differences, save as PDF.

## Complete backup for emergencies (spare part pass)

In case a control unit breaks: **one click backs up everything you need for the
replacement** — part number, software and hardware version, coding and adaptations of all
control units (UDS and TP 2.0) — and creates the PDF **“spare part pass”** (per control
module: which part to buy, coding, what to do after installation, e.g. teach the immobiliser
online). Read only, takes about 10–20 minutes. GUI: **Backup → “🛟 Start complete backup”**.

```bash
ecu-werkbank notfall-sicherung
```

The program memory (the software) itself cannot be read via diagnostics and is not needed — a
spare part with the same part number brings it along. Adaptations of TP 2.0 control units:
everything the module gives out via identifiers (0x22) is backed up; very old modules without
this service give nothing this way.

### Extended identification, basic settings (reference)

```bash
ecu-werkbank -i can0 ident --geraet 714        # all standard identification fields of a UDS module (part no., SW/HW, serial no., date …)
ecu-werkbank grundeinstellung --system bremse  # catalogue of common workshop functions (lookup only)
```

`ident` only reads (works on every UDS module, also on the Golf). `grundeinstellung` is a
pure lookup aid and never touches the car.

In the GUI: **Extended identification** via the 🆔 button at the selected UDS control unit
(Diagnostics → Control units), the **reference** at the bottom of the page Tools → Fault
patterns & reference.

### After replacement: write back adaptations

GUI: **Backup → Adaptations → “↩ Write back adaptations”**: choose backup and control unit,
**compare** (read only), tick the values you want, confirm, write. In the terminal:

```bash
ecu-werkbank -i can0 anpassung-zurueck                                   # control units of the newest backup
ecu-werkbank -i can0 anpassung-zurueck --geraet 714                      # compare (read only)
ecu-werkbank -i can0 anpassung-zurueck --geraet 714 --kennung 2243       # shows what would be written
ecu-werkbank -i can0 anpassung-zurueck --geraet 714 --kennung 2243 --ja  # writes + checks
```

`--geraet` is hex: UDS request ID (e.g. `714` instrument cluster) or TP 2.0 address (e.g.
`20`). Protection: only what is **different** today, has **the same length** (otherwise the
layout does not match — different part) and **does not change by itself** on a second read
(measured value) is written. The **mileage is never written.** Every value is read back
afterwards for checking; the values before are stored as a separate backup (so everything can
be undone). The VIN of backup and car must match. If the control unit refuses (access
permission/login needed), nothing is forced. For TP 2.0 control units, writing identifiers
via 0x2E this way is not yet tested on the car.

### Login (SecurityAccess) when a module refuses writing

Some UDS control units require a login before writing: they first give out a random number
(the “seed”), and the tool must send the matching answer (the “key”). On these vehicles the
key is usually calculated via **SA2** — a small program (bytecode) stored in the **flash data
of your control unit**, different for each module and software version. **ECUWerkbank does
not ship any bytecode**; the calculating machine (SA2) is built in and tested, you provide the
bytecode for your module yourself.

```bash
ecu-werkbank -i can0 login --geraet 714 --sa2 "68 02 81 49 …" --level 3   # store bytecode
ecu-werkbank -i can0 login --geraet 714 --test                           # log in (writes nothing)
ecu-werkbank login                                                       # show stored logins
ecu-werkbank login --geraet 714 --loeschen                               # remove again
```

In the GUI the field is under **Write back adaptations → “🔑 Login”**. If a login is stored,
writing back logs in automatically on “access denied” and then writes. The login only opens
the door to **coding and adaptation** — ECUWerkbank cannot flash a control unit. For the
older TP 2.0 modules the login is not built in yet (it works differently there).

The SA2 machine is our own code based on a publicly documented description, cross-checked
against known values (seed `1a1b1c1d` → key `6a37f02e`). Thanks to the open documentation of
[bri3d/sa2_seed_key](https://github.com/bri3d/sa2_seed_key).

### Back up and restore (whole vehicle)

```bash
ecu-werkbank --fahrzeug <VIN> akte-zip --out golf.zip   # all data of the vehicle as ZIP
ecu-werkbank fahrzeug-import golf.zip                    # restore (e.g. after reinstalling)
```

Restores scans, codings, software, adaptation and emergency backups including reports.
Existing files stay, unless `--ueberschreiben` (overwrite) is given.

## Coding backup

```bash
./target/release/ecu-werkbank -i can0 coding-backup         # back up all control units
./target/release/ecu-werkbank coding-list
./target/release/ecu-werkbank coding-compare                # newest two, down to the bit (with label names)
./target/release/ecu-werkbank -i can0 coding-restore --file <backup.toml> --tx 0x7E0        # only shows
./target/release/ecu-werkbank -i can0 coding-restore --file <backup.toml> --tx 0x7E0 --ja   # writes + checks
```

UDS control units: coding via DID 0x0600 (or `coding_did` from the label), writing back with
a check afterwards. Some control units require access permission for this — then it stops
with a clear message.
TP 2.0 control units:
- **Newer** ones (Golf 6: central electrics 20, ABS 03, parking aid 1D, trailer 43) understand
  identifiers like UDS — the long coding comes via identifier 0x0600 and can be viewed and
  named bit by bit in the coding helper. Confirmed on the Golf 6 via capture with a workshop
  diagnostic tool (14.11.2026): central electrics (30 bytes) and ABS (18 bytes) deliver the
  same coding byte by byte.
- **Older** ones (engine, doors, steering column) have a short coding in the identification
  (bytes 16–19 = `03 xx xx xx`); ECUWerkbank shows it as a number (compared on the Golf 6 with
  a workshop diagnostic tool: correct). Others (gateway, steering): raw identification.
- **Writing long coding** (`kwp-langcodierung --neu … --ja`, GUI page “Coding”): central
  electrics/ABS via identifier 0x0600, gateway via identification 0x9A, then read again for
  checking. Likewise `kwp-anpassung-schreiben` (change an adaptation channel). All three
  written by ECUWerkbank itself and confirmed on the Golf 6.
- **Workshop code:** For writing, ECUWerkbank sends a workshop code like a workshop tool does
  (workshop number/importer/equipment number). The Golf 6 central electrics refuses without it
  (or with all zeros). Enter it in the GUI under Vehicles → Company header or with
  `ecu-werkbank firmenkopf --werkstattcode 12345/123/12345` (alternatively the environment
  variable `VAG_WERKSTATTCODE`). Without a workshop code ECUWerkbank does not write.
- TP 2.0 addresses are always given in hex, with or without `0x` (`20` = `0x20` = central
  electrics).

```bash
./target/release/ecu-werkbank -i can0 kwp-coding --address 20     # read central electrics
./target/release/ecu-werkbank -i can0 kwp-langcodierung --address 20 --neu <hex>   # preview: which bits change
./target/release/ecu-werkbank codiertext --tx 220 --byte 0 --bit 2 --text "…"   # TP 2.0: 200 + address
```

In the GUI: tab **“Coding backup”** or page “Coding”. Both only show the backups of the vehicle
selected at the top — so no foreign coding can be written to the car. A new backup lands with
the connected car (by VIN) and selects it.

### Own coding texts

What a coding bit does you find out yourself and enter it once — it then applies in the coding
helper, when decoding and in the backup comparison. The texts belong to the CAN ID of the
control unit (e.g. 714 = instrument cluster on the Golf 6) and are stored in
`codiertexte.toml` in the data folder. Without your own text, the name from your `[[coding]]`
labels is used.

```bash
./target/release/ecu-werkbank codiertext --tx 714 --byte 0 --bit 3 --text "Daytime running lights"
./target/release/ecu-werkbank codiertext --tx 714 --byte 2 --text "Country variant"   # whole byte
./target/release/ecu-werkbank codiertext --tx 714                                      # show texts
./target/release/ecu-werkbank decode-coding --tx 714 --bytes 0901A3                    # decode
```

How to learn a bit: change the bit and write (or code with another tool and back up
before/after), look at the effect on the car, then in the coding helper at “Changed compared
to …” click ✏ and enter the name. `--tx` is always hexadecimal (714 = 0x714).

## Live graph

GUI tab **“Scope”**: several curves at the same time from OBD-II values, TP 2.0 measuring
blocks and UDS DIDs; time window, “normalised” to compare different units,
current/min/max table, record CSV and load old CSV recordings again.

## OBD-II (manufacturer-independent)

```bash
./target/release/ecu-werkbank -i can0 obd-info                 # VIN, MIL, readiness
./target/release/ecu-werkbank -i can0 obd-dtc                  # stored / pending / permanent
./target/release/ecu-werkbank -i can0 obd-live --pids 0C,0D,05 --count 0
./target/release/ecu-werkbank -i can0 obd-clear                # also clears readiness!
```

Also works with ELM327 (`-t elm327 --port /dev/ttyUSB0`). In the GUI: tab **“OBD-II”**.

## Other makes (e.g. Mercedes, BMW, Opel)

ECUWerkbank is built for VW/Audi/Škoda/Seat. For other makes, what is standardised works:
**OBD-II** (page OBD-II: engine/gearbox faults, readiness, emission test values, live values)
and the page **Diagnostics → Other makes**: it searches the control units at the OBD socket
and reads identification (UDS identifiers F187, F18C, F189, F191, F197 … or KWP2000 0x1A) and
fault memory (UDS 0x19 or KWP2000 0x18). Result as fault memory PDF in the vehicle folder (by
VIN).

```bash
./target/release/ecu-werkbank -i can0 fremd          # search, read, PDF (read only)
```

How the search works (SocketCAN only): listen for 1.5 s; CAN IDs with traffic are never used.
With more than 20 busy IDs (socket attached to a vehicle bus) only 0x7E0–0x7E7, otherwise:

- all free IDs 0x400–0x7FF, 25 ms each, only “tester present” (0x3E),
- 29-bit addresses (ISO 15765-4): once to all via 0x18DB33F1, answers from 0x18DAF1xx,
- **BMW/Mini** (extended addressing according to ISO 15765-2): on 0x6F1 with the address of the
  control unit in the first byte, 20 ms each; answer on 0x600 + address. In the list they
  appear as `0x6f1:12/0x612:f1` (12 = address of the control unit).

Hits are then verified one by one. With ELM327 and demo only the OBD addresses (and 29 bit if
11 bit finds nothing). Clearing faults works per control unit with confirmation (UDS 0x14 or
KWP2000 0x14).

Not possible: manufacturer texts for faults (only standardised P codes have text), coding,
adaptations, service reset. Not yet tried on a real Mercedes or BMW. To practise without a car:
`VAG_MOCK_BMW=1 cargo run --bin mockecu -- vcan0` additionally plays a control unit with BMW
addressing (0x12).

## Recording measured values (CSV)

```bash
./target/release/ecu-werkbank -i can0 kwp-block --address 0x01 --block 1,2,3 --count 0 --csv fahrt.csv
./target/release/ecu-werkbank -i can0 obd-live --count 0 --interval 250 --csv obd-fahrt.csv
```

Format for German spreadsheet programs: `;` as separator, decimal comma, first column time in
seconds. Every line is written immediately (stopping with Ctrl-C loses nothing). In the GUI:
“⏺ Record CSV” next to “Live”.

## Analysing captures

```bash
./target/release/ecu-werkbank -i can0 record --out mitschnitt.log --seconds 30   # or: candump -L can0 > mitschnitt.log
./target/release/ecu-werkbank decode-log mitschnitt.log
./target/release/ecu-werkbank -t demo record --out beispiel.log                  # example without a car
```

`decode-log` translates TP 2.0/KWP2000, UDS and OBD-II into plain text and outputs a **check
report**: Is the order of the channel IDs right? Does the length field fit? Does the sequence
number continue across the channel? Which measured value formulas occur? This lets you clarify
the `TODO(verify)` places in `src/tp20.rs` with a single capture of your car — best while
another, working diagnostic tool opens a channel. In the GUI: tab **“Capture”**.

### Captures for other programs, DBC signals

```bash
./target/release/ecu-werkbank log-export --log mitschnitt.log --out mitschnitt.pcap   # Wireshark
./target/release/ecu-werkbank log-export --log mitschnitt.log --out mitschnitt.asc    # Vector/SavvyCAN
./target/release/ecu-werkbank dbc-decode --dbc eigene.dbc --log mitschnitt.log [--nur Drehzahl]
```

`dbc-decode` reads your own or freely licensed DBC files (Intel/Motorola, signed, factor
/offset, value texts); ECUWerkbank does not ship manufacturer DBCs.

### Scripts (Rhai)

```bash
./target/release/ecu-werkbank -t golf6 skript beispiele/messwerte.rhai
```

Your own sequences as a small program: `obd(pid)`, `obd_vin()`, `uds_lesen(tx, did)`,
`uds_text(tx, did)`, `uds_fehler(tx)`, `kwp_block(adresse, block)`, `kwp_fehler(adresse)`,
`fehlertext(code)`, `hex(liste)`, `warte(ms)`, `zeit()`, `print(…)`. In the GUI: Tools →
Scripts (with stop button). Scripts can **only read**.

### REST interface (for your own scripts)

```bash
./target/release/ecu-werkbank api            # shows the address with key
python3 beispiele/api_abfrage.py http://127.0.0.1:8787/<key>/api/v1/
```

Reads the vehicle database: `status`, `quellen`, `hersteller`, `hersteller/{id}/steuergeraete`,
`steuergeraete/{id}`, `fehler/{code}`, `can/{id}/signale`, `fahrzeug?vin=…`. Answers are JSON.
The interface only listens on this computer, only accepts read requests and never sends
anything to the car.

### Compare the whole configuration

```bash
./target/release/ecu-werkbank konfig-vergleich                 # active vehicle: previous → newest backups
./target/release/ecu-werkbank konfig-vergleich --a golf --b polo   # two vehicles
```

Compares the stored coding and adaptation backups and software versions (back them up first
with `coding-backup`, `anpassung-sichern`, `software`). No car needed.

### Company header for reports

```bash
./target/release/ecu-werkbank firmenkopf --name "Garage Example" --adresse "Example Road 1, 12345 Exampletown" --telefon "0123 456"
./target/release/ecu-werkbank firmenkopf --logo logo.png        # PNG or JPG; --logo "" removes it
./target/release/ecu-werkbank firmenkopf --werkstattcode 12345/123/12345   # workshop code for writing (not shown in reports)
./target/release/ecu-werkbank firmenkopf --loeschen
```

The company header then appears **at the top** of every page of all PDF reports: on page 1 on
the right in the title bar with the logo on a white field, on the following pages small at the
top right. The logo is best landscape, with little margin and below 300 KB (at most 2 MB); PNG
with a transparent background works too. In the GUI: Vehicle → Vehicles & maintenance →
“Company header for reports” → “🖼 Choose logo…”.

### Vehicle database (local only)

Vehicle knowledge can be collected in your own database (SQLite file in the data folder, no
installation needed). Every source is stored with kind, licence and origin and can be removed
again as a whole.

```bash
./target/release/ecu-werkbank db                       # overview
./target/release/ecu-werkbank db eingebaut             # standard data + bundled community data
./target/release/ecu-werkbank db labels                # take over your own label files
./target/release/ecu-werkbank db dbc --datei vw_pq.dbc --lizenz MIT --hersteller Volkswagen
./target/release/ecu-werkbank db import --datei anything.dbc    # kind is detected (DBC, package, label, CSV, ODX)
./target/release/ecu-werkbank db paket --datei my-data.toml     # data package (TOML/JSON)
./target/release/ecu-werkbank db csv --datei faults.csv         # code;text  and/or  did;name;unit;factor;offset
./target/release/ecu-werkbank db export --ziel contribution.toml --lizenz CC0-1.0
./target/release/ecu-werkbank db odx --datei bought.pdx --hersteller Volkswagen   # ODX/PDX, stays local
./target/release/ecu-werkbank db quellen               # what is in it, from where, which licence
./target/release/ecu-werkbank db teil 5K0937086        # what is known about this part number
./target/release/ecu-werkbank db aufraeumen            # delete adaptation channels without names (“Kanal 4”)
./target/release/ecu-werkbank db entfernen --id 3      # preview; really with --ja
./target/release/ecu-werkbank db sichern --ziel kopie.sqlite
```

In the GUI: **Tools → Vehicle database** (or drag a file into the window). Data without a
licence and manufacturer data you bought yourself always stay with you only; `db export`
leaves them out and says what is missing. The format of the data packages is described in
`src/db/paket.rs`; an example with everything that fits in (control units, fault texts,
measured values, coding bits, adaptations) is
[beispiele/datenpaket-alles.toml](beispiele/datenpaket-alles.toml). A CSV may contain fault
texts and measured values mixed; header lines (`code;text`, `did;name;…`) switch, without a
header every line is detected individually — see [beispiele/gemischt.csv](beispiele/gemischt.csv).

**Fault location and fault type:** A fault consists of the component (e.g. “01627 parking aid
sensor”) and the fault type (e.g. “14 defective”). With TP 2.0 the fault type is a number
0–15 in the status byte, with UDS the 3rd byte of the fault. The known numbers are built in;
missing or differently wanted texts are entered as `[[fehlerart]]` in the data package (your
own take precedence). Typos in the package are reported, not silently skipped. Plan and
structure: [ROADMAP.md](ROADMAP.md), “Plan: Fahrzeug-Datenbank”.

### Texts for measured values with text numbers (TP 2.0)

Some measured values are delivered by the control unit as a text number (formula F25);
ECUWerkbank then shows “raw F25 02 FD”. The matching words can be learned from a block export
of your own workshop tool on your own car — matched against what ECUWerkbank itself has read:

```bash
./target/release/ecu-werkbank kwp-blockscan --address 20 > bordnetz.txt
./target/release/ecu-werkbank wertetexte-lernen --blockmap blockmap-09-….CSV --scan bordnetz.txt
./target/release/ecu-werkbank wertetext --code F25:02FD --text normal     # or by hand
```

Only what was displayed the same everywhere is taken over. The table
(`<data folder>/wertetexte.toml`) stays on your computer and is never uploaded.

### Replay a capture (vcan only)

```bash
sudo modprobe vcan && sudo ip link add vcan0 type vcan && sudo ip link set vcan0 up   # once
./target/release/ecu-werkbank replay --log mitschnitt.log --iface vcan0 [--tempo 2] [--schleife]
```

Plays a capture at the original timing onto a **virtual** CAN interface, e.g. to test the CAN
monitor or your own tools without a car. Real adapters are refused — nothing is ever sent to
the car.

### Live CAN monitor (GUI)

**Tools → CAN monitor** shows every CAN ID live with data, count and rate. Bytes that are
currently changing are coloured; a click on an ID shows the individual bits. The monitor
**only listens** and sends nothing.

Finding a signal: press **“Remember noise”**. Everything that changed by itself until then
(counters, checksums) is ignored afterwards. Then operate something on the car, e.g. the
indicator: only the bits that change because of it, and new IDs (★), are highlighted. With
**“Save capture”** everything lands as a candump file in the vehicle folder. Your own DBC file
shows message names and signals; with 📈 a signal goes as a curve into the live graph (the
monitor must keep running for that). Works with a SocketCAN adapter and in the demo vehicle
(there with invented traffic and an indicator to try).

Note Golf 6: at the OBD socket usually only diagnostic traffic arrives through the gateway.

**Drag files into the window:** capture (`.log`, `.asc`), DBC, recording (`.csv`), data package
or label file (`.toml`, `.json`) — just drag them onto the window. They open on the right page,
data for the vehicle database only with a preview. A data backup (`.zip`) is not restored this
way, that only happens deliberately via “Vehicles & maintenance”.

## Your own labels and your own texts (your content)

Since 0.4 all data (vehicles, scans, backups, settings), your own labels and all your own texts
are stored **in the vehicle database** (`<data folder>/fahrzeugdaten.sqlite`). Everything is
edited in one place: **Tools → Vehicle database**.

- **View and edit content:** choose a source (e.g. “Own labels” or an imported data package),
  then per control unit change name, address, part number, CAN IDs, coding identifier, fault
  texts, measured values, coding bits, adaptation channels and **target values** (traffic
  light for measured values). “+ Control unit” creates a new one.
- **⬇ Create from last scan** (with the source “Own labels”): creates an entry for each
  control unit from the last scan of the active vehicle with address, name, part number and
  CAN IDs from *your* scan. Nothing is overwritten.
- **Own texts:** your fault texts, names of measuring blocks (TP 2.0), coding texts,
  identifier names and value texts as a list to change and delete. They take precedence over
  all other sources. The fastest way to name measured values is ✏ directly at the measuring
  block in the TP 2.0 tab; “🔍 Read all blocks” shows a whole control unit there as a list
  for naming.

Label files (`*.toml` with `[ecu]`) only exist for exchange now: drag them into the window or
take them over via “Import file”; the database export writes data packages. **Migration:** On
first start ECUWerkbank takes over an existing label folder and the earlier text files into the
database once.

For more than 300 standardised codes (P0xxx, P2xxx, U0xxx) **German basic texts are built in**
(own wording, `src/dtc_db.rs`); your texts always take precedence.

```bash
./target/release/ecu-werkbank labels                      # shows loaded labels
```

> You write the texts yourself (or take them from open, standardised sources). ECUWerkbank does
> not read foreign label files of other diagnostic programs — the content belongs to you.

## Testing without a car and without sudo

```bash
cargo test --lib          # TP 2.0 + KWP2000 against a simulated control unit (in memory)
# end-to-end in a private network namespace (the vcan module must be loadable):
unshare -rn bash -c 'ip link add dev vcan0 type vcan && ip link set up vcan0 &&
  ./target/release/mockecu vcan0 & sleep 0.5; ./target/release/ecu-werkbank -i vcan0 autoscan'
```

`mockecu` provides the demo vehicle on a CAN interface (three UDS and three TP 2.0 control
modules, OBD-II at the engine; values invented).

### Golf 6 simulation (rebuilt from the real scan)

Besides the invented demo vehicle there is a replica of the Golf 6 (2010, 1.4 TSI) from the
real scan of 28.10.2026: the same 17 control units (5 UDS, 12 TP 2.0) with addresses, CAN IDs,
part numbers and the 5 faults (08851, P1000, 01627, 00097, 00929). The quirks are in too: the
engine (MED17) only knows `18 00`, answers first with “response pending”, ABS reports “busy” the
first time, address 0x29 only answers the channel setup, the engine on 0x7E0 only speaks OBD-II,
the instrument cluster reports “test not completed” entries (no faults).

```bash
ecu-werkbank -t golf6 report --voll           # terminal
VAG_TRANSPORT=golf6 ecu-werkbank-gui          # GUI (or choose “Demo” → “Golf 6” at the top)
VAG_DEMO=golf6 mockecu vcan0              # on a (virtual) CAN bus
```

Since the test run on the car (29.10.2026) the engine blocks 1–4 (8 fields per block, as with
the MED17) and the mileage in the instrument cluster have the real structure as well. **Not
real** are the VIN (invented so that your real vehicle folder stays untouched) and the values
themselves — they come from the simulation.

## Mileage from the instrument cluster (Golf 6)

On the Golf 6 the engine control unit does not give out a mileage via OBD. ECUWerkbank
therefore reads it from the instrument cluster during the scan: UDS identifier 0x2203, 2 bytes in
10 km steps (tested on the Golf 6 2010: `5E B8` = 24 248 → 242 480 km at 242 488 km on the
odometer). The value is therefore rounded down to 10 km. It ends up in the log and in the vehicle
folder. For searching in other control units there is `rdid-scan` (read only):

```
ecu-werkbank rdid-scan --tx 0x714 --rx 0x77E --von 0x2200 --bis 0x22FF --suche 242488
```

**Exact reading:** Where the instrument cluster stores the km-exact reading is not published.
Search once (read only, up to 1 minute; read the odometer exactly, do not move the car):

```
ecu-werkbank km-genau --tacho 242553   # searches 0x2000–0x23FF, 0x0200–0x02FF, 0x1000–0x10FF in the cluster
ecu-werkbank km-genau                  # afterwards: read the exact reading
```

A unique location is remembered in the vehicle's `profil.toml` (`[km_quelle]`); from then on every
scan takes the exact value — but only if it matches the 10 km value from 0x2203. Without a
remembered location the scan tries the usual manufacturer identifier 0x2002 (3 bytes), likewise
only with this cross-check. GUI: Vehicle → Vehicles & maintenance → odometer reading into the km
field, “🔍 Search location in the cluster”. If the odometer reading ends in **0**, nothing is
remembered: then every value in 10 km steps matches as well, a km-exact location cannot be
recognised. Only search when the last digit is 1–9. **Tested on the Golf 6 (2010, cluster
5K0920860F) on 07.11.2026:** the cluster does **not** give out the exact reading via diagnostics
(108 readable identifiers, no hit at odometer 242 847; 0x2002 delivers nothing suitable) — there
0x2203 with 10 km steps remains the source. The Golf 6 simulation has an exact reading only with
`VAG_SIM_KM_GENAU=1`.

## Service display: read and reset

GUI: Vehicle → Service display. CLI: `ecu-werkbank -f <vehicle> service …`

The identifiers of the service values in the instrument cluster are not published and differ per
cluster. They are therefore stored per vehicle in `fahrzeug.toml` under `[service]` and learned
once:

```
ecu-werkbank -f "M-AB 123" service merken --name vorher     # read only, ~1 min (“vorher” = before)
#   reset the service with the buttons on the cluster  (ecu-werkbank service anleitung)
ecu-werkbank -f "M-AB 123" service merken --name nachher    # (“nachher” = after)
ecu-werkbank -f "M-AB 123" service vergleich --uebernehmen  # changed identifiers = service values
ecu-werkbank -f "M-AB 123" service                          # read (also in the quick check)
ecu-werkbank -f "M-AB 123" service zuruecksetzen --ja       # backs up before, reads after, enters the oil change in the maintenance plan (--ohne-wartung: not)
ecu-werkbank -f "M-AB 123" service rueckgaengig --ja        # old values from the backup
```

If identifiers also change without a reset (clock …), remember a third state `--name ruhe` and use
`vergleich --ruhe ruhe`.

**Set up without a button reset:** read the display on the cluster and let it search. With
`--einrichten` unique hits are taken over; on reset the new interval is then written into the
found field (other bytes stay as they are):

```
ecu-werkbank -f "M-AB 123" service suche --km 4800 --tage 212 --oel-km 1200 --oel-tage 90 \
         --einrichten --intervall 30000,730,15000,365   # km,days inspection · km,days oil change (service book!)
ecu-werkbank -f "M-AB 123" service zuruecksetzen            # preview, then with --ja
```

In the GUI: “Set up reset” → enter display values → “Read cluster now” → “Search matching
identifiers” → tick one hit per value → “Set up as service reset”. A hit only matches the number
by chance if several identifiers have the same value — so choose exactly one per value; the backup
undoes every reset. If the cluster requires a login (NRC 0x33), the reset stops and nothing is
bypassed. The demo vehicle has demo identifiers (0x22A0–0x22A3 at the engine, simulation only);
the Golf 6 replica has none because the real ones have not been learned yet.

## Freeze frame data for TP 2.0 faults

For every fault ECUWerkbank queries the stored freeze frame data — read only: first the VAG freeze
frame (KWP 0x12 with the fault code), otherwise 0x17. The answer `6C 01 …` is shown in plain text:
fault status, priority, frequency, unlearn counter, mileage, date/time and further measured values
(e.g. temperature). The structure is derived from our own capture on the Golf 6 and compared with a
workshop tool on the same fault (25.11.2026), confirmed on the Polo 6R (26.11.2026). If the date
contains a month above 12, the clock in the cluster has never been set — ECUWerkbank says so.
Unknown answers remain as “freeze frame data (raw)”.

## Further connections (DoIP, CAN FD, K-line, J2534)

| Connection | What for | Command / setting | Status |
|---|---|---|---|
| **K-line behind the gateway** | older control units that the gateway passes through over CAN (Polo 6R: ABS, power steering) | automatically in the scan; individually `ktunnel --adresse 0x03` | ✅ on the Polo 6R (2010) |
| DoIP (Ethernet) | newer cars with diagnostics over network (roughly from Golf 8) | `-t doip --port 169.254.1.10`, GUI transport “DoIP” | 🟡 only against a simulated gateway |
| CAN FD | control units with fast CAN | adapter with `fd on`, then `VAG_CANFD=1` | 🟡 tested on vcan |
| K-line with KKL cable | very old VW (before ~2004, KW1281) | `kline --port /dev/ttyUSB0 --adresse 0x01` | 🟡 simulation only |
| J2534 adapter | Windows only, 64-bit drivers | `-t j2534`, GUI transport “J2534 adapter” | 🟡 built only |

**K-line behind the gateway** (from our own capture on the Polo 6R with a workshop tool): such
control units answer the TP 2.0 channel setup only briefly with `00 D0 xx`. ECUWerkbank then opens
the tunnel (`200#<target> C0 A0`, tester 0x7A0, answer 0x7xx), sends the 5-baud address as the
first message and then speaks KW1281 (whole blocks) or KWP2000 with header. Read only:
identification and fault memory.

## Start page: the vehicle at a glance

For the selected vehicle the start page shows four cards (mileage with km/year, stored faults with
comparison to the previous scan, last scan, next maintenance) and the control units with faults —
a click opens the control unit under “Control units” for reading or clearing. Everything from
the stored data, without a connection. Mileage and fault history (from two scans) are under
**Vehicle → History & trends**.

### Detect the vehicle automatically

The GUI notices by itself when a car is plugged in or the ignition is switched on. To the right of
the adapter display it then says “🚗 *vehicle*” instead of “no car”, and the vehicle is selected by
its VIN (new: created). It only asks read-only and only when the bus has been free for 5 s: first
the instrument cluster (UDS 0x714, identifier 0xF190), otherwise OBD mode 09. After that every 10 s
a “tester present” to the same control unit — if it no longer answers, it says “car
disconnected”. Without a car (adapter in ERROR-PASSIVE) nothing is sent continuously, only the
adapter counters are watched. SocketCAN and demo only, not ELM327. Clicking the display switches
detection off/on (remembered in `~/.local/share/ecu-werkbank/erkennung-aus`).

## Status & roadmap

In detail (German): [ROADMAP.md](ROADMAP.md) (50 functions by benefit/effort), structure:
[ARCHITECTURE.md](ARCHITECTURE.md), security: [SECURITY_REVIEW.md](SECURITY_REVIEW.md), changes:
[CHANGELOG.md](CHANGELOG.md), contributing: [CONTRIBUTING.md](CONTRIBUTING.md), extending:
[PLUGIN_DEVELOPMENT.md](PLUGIN_DEVELOPMENT.md).

Scope of common workshop diagnostics (function numbers as in VAG-155x/VAS-505x) and status in
ECUWerkbank:

| No. | Function | UDS (from ~2008) | TP 2.0 / KWP2000 |
|---|---|---|---|
| 00 | Auto-scan / report (PDF) | ✅ `autoscan`, `report` | ✅ `autoscan`, `report` |
| 01 | Control unit identification | ✅ `rdid` (0xF187, 0xF190 …) | ✅ `kwp-ident` (software version confirmed on the Golf 6) |
| 02 | Read fault memory | ✅ `dtc` (VW fault numbers as with the workshop tool) | ✅ `kwp-dtc` (fault type confirmed on the Golf 6) |
| 02 | … with freeze frame data | ✅ snapshot per fault (km, engine speed … — unknown values raw) | ✅ VAG freeze frame (0x12) in plain text, compared with a workshop tool on the Golf 6 |
| – | Import auto-scan logs (history, freeze frame, fault texts) | ✅ `autoscan-import` | ✅ `autoscan-import` |
| 03 | Output tests | 🟡 `iocontrol` (raw) | ✅ `kwp-stellglied`, GUI Control units → choose module → “▶ Output tests” (sequence learned via capture at the Golf 6 door) |
| 04 | Basic settings | 🟡 `routine` (raw) | ✅ after your own capture (GUI, `kwp-grundeinstellung`; throttle body adaptation tested on the Golf 6) |
| 05 | Clear fault memory | ✅ `clear` | ✅ `kwp-clear` |
| 06 | End output | ✅ | ✅ channel is closed cleanly |
| 07 | Coding | ✅ `coding`, `code-write` | ✅ read `kwp-coding` (long, short and gateway — confirmed on the Golf 6); write short coding `kwp-codierung-schreiben` (Golf 6 door tested); write long coding `kwp-langcodierung --neu` (✅ identifier 0x0600 and gateway, written on the Golf 6; needs a workshop code) |
| – | Coding backup / comparison / write back | ✅ `coding-*` | 🟡 back up only |
| – | Vehicle history, before/after | ✅ `history`, `compare` | ✅ |
| 08 | Measuring blocks | ✅ `rdid` + labels | ✅ `kwp-block`, `kwp-blockscan`, names in one list; formulas compared with a workshop tool on the Golf 6 |
| 09 | Single measured values | ✅ single identifier | ✅ 🔍 at the measured value field (reads the block like the workshop tool) |
| 10 | Adaptation | 🟡 back up/compare `anpassung-*`, name identifiers; writing only with login | ✅ read channels `kwp-anpassung` (Golf 6 central electrics/parking aid tested), back up; write `kwp-anpassung-schreiben` (✅ Golf 6 central electrics, needs a workshop code) |
| 11/16 | Login / access authorisation | 🟡 `login` (SecurityAccess, bytecode from the user) | 🟡 `login --kwp` (untested on the car) |
| 15 | Readiness | ✅ `obd-info` + driving guide (GUI) | – |
| 18 | Fault paths (all possible fault codes) | ✅ `fehler-geruest`, GUI Tools → Fault patterns | ❌ service does not exist in KWP |
| 1A | Extended identification | ✅ `ident` (incl. workshop number of the last coding) | 🟡 `kwp-ident` |
| 33 | OBD-II/EOBD (modes 01, 02, 03, 04, 07, 09, 0A) | ✅ `obd-*`, GUI “OBD-II” (incl. freeze frame) | – |
| – | Log measured values (CSV) | ✅ `obd-live --csv` | ✅ `kwp-block --csv` |
| – | Record/analyse captures | ✅ `record`, `decode-log` | ✅ incl. check report |
| – | Capture as PCAP/ASC, DBC signals | ✅ `log-export`, `dbc-decode` | ✅ |
| – | Live CAN monitor with noise filter | ✅ GUI “CAN monitor” (listen only) | ✅ |
| – | Restart control unit, fault count | ✅ `ecu-reset` (with `--ja`), `dtc-anzahl` | – |
| – | Read / reset service display | 🟡 `service` (learn identifiers per vehicle; Golf 6: km since service confirmed) | – |
| – | Installation list (installed control units, by system) | ✅ `verbauliste`, GUI Control units → “📋 Installation list by system” | ✅ from the scan |
| – | Gateway installation list directly from the gateway | ⏳ planned (newer cars with UDS gateway) | ✅ `gateway-verbauliste`, GUI button (Golf 6 tested) |
| – | Transport mode | ❌ | ❌ |
| – | K-line | – | ✅ behind the gateway `ktunnel` (Polo 6R); 🟡 KKL cable `kline` |

✅ works · 🟡 partly · ⏳ planned · ❌ does not work · – does not exist there

Next:
- Pin down the remaining measured value formulas (brake pressure, acceleration, yaw rate …) with
  a simultaneous comparison measurement
- Gateway installation list for newer UDS gateways
- Test DoIP, KKL cable and J2534 on real hardware
- **Control unit address map** from your own `discover` captures

## Licence, trademarks, sources

ECUWerkbank is licensed under either the **MIT licence** ([LICENSE-MIT](LICENSE-MIT)) or the
**Apache licence 2.0** ([LICENSE-APACHE](LICENSE-APACHE)), at your option. © 2026 dflow81.

**Trademarks:** ECUWerkbank is an independent project and is **not** affiliated with Volkswagen
AG, its brands (VW, Audi, Seat, Škoda) or with Ross-Tech LLC. Trademarks mentioned belong to their
owners and are used descriptively only.

**Third-party data:** ECUWerkbank ships **no** label files, databases or keys from Ross-Tech or
Volkswagen and does not read label files of other diagnostic programs. Login procedures (SA2) must
be provided by the user. Protocols (ISO-TP, UDS, KWP2000, TP 2.0) are implemented from public
standards or public community documentation.

**Sources of the login test values:** The test values of the seed/key procedures (SA2, KWP XOR)
come from public projects; origin and MIT notice see [CREDITS.md](CREDITS.md).

**Bundled libraries:** all under free licences (MIT, Apache-2.0, BSD, Zlib …; list:
`cargo metadata`). Via `egui` the GUI contains fonts under the SIL Open Font License and the Ubuntu
Font Licence — include their licence texts when passing on built programs.
