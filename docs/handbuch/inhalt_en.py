"""Content of the manual (English). Executed by build_handbuch.py --en —
the helpers (h1, p, bild, tabelle …) come from there. Pictures from bilder-en/."""
# flake8: noqa
# ---------------------------------------------------------------- Title page
story += [Spacer(1, 60 * mm), Paragraph("ECUWerkbank", S["titel"]), Spacer(1, 6 * mm),
          Paragraph("Manual", S["unter"]), Spacer(1, 3 * mm),
          Paragraph("Diagnostics &amp; coding for German cars — on Linux", S["unter"]),
          Spacer(1, 70 * mm),
          Paragraph(f"Version {VERSION} · as of {STAND} · tested on a Golf 6 (2010, 1.4 TSI)", S["unter"]),
          PageBreak()]

# ---------------------------------------------------------------- Contents
toc = TableOfContents()
toc.levelStyles = [S["toc1"]]
story += [Paragraph("Contents", S["h1"]), toc, PageBreak()]

# ---------------------------------------------------------------- 1
h1("1  What is ECUWerkbank?")
p("ECUWerkbank is a diagnostic program for vehicles of the VW Group. It reads fault memories, measured values, "
  "mileage, software and coding versions from all control units — via a cheap CAN adapter at the OBD socket. "
  "There is a graphical interface with windows (GUI) and commands for the terminal.")
h2("What it can do")
liste(
    "Find all control units, including older ones with the <b>TP 2.0</b> protocol (about 2004–2010) and newer ones with <b>UDS</b>.",
    "Read faults and show them clearly, save them as PDF and compare them with last time.",
    "Live values, measuring blocks with your own names and favourites, battery and cranking test, mixture check, "
    "emission test results.",
    "Keep one folder per vehicle: scans, mileage, maintenance, coding backups, reports.",
    "Used car check: cleared faults? does the odometer fit? the same VIN everywhere?",
    "Look things up: extended identification, lexicon of basic settings.",
)
h2("What it deliberately cannot do")
liste(
    "No changes to the immobiliser, component protection or other protection functions.",
    "<b>No odometer tampering.</b> ECUWerkbank reads the mileage but never writes it — every write path rejects "
    "the mileage identifiers.",
    "It ships no third-party program code and no third-party descriptions (labels). You create your own "
    "descriptions yourself (chapter 12).",
)
kasten("Reading is always safe",
       "Almost everything in ECUWerkbank <b>only reads</b>. Functions that change something on the car (clear faults, "
       "write coding, basic settings) are clearly marked, <b>ask first</b> and back up the old state beforehand. "
       "More in chapter 13.")
p("Language note: the program is available in German and English. Command names in the terminal, saved data "
  "(scans, maintenance items) and fault texts of the control units stay German.")

# ---------------------------------------------------------------- 2
h1("2  What you need")
tabelle(["Part", "Note"], [
    ["Laptop with Linux", "Tested with Linux Mint. A kernel with SocketCAN (standard)."],
    ["CAN adapter", "Any SocketCAN adapter, e.g. DSD TECH SH-C31x, CANable, candleLight, PCAN. "
                    "It appears as <b>can0</b>."],
    ["OBD cable", "Pin 6 = CAN high, pin 14 = CAN low, pin 5 = ground."],
    ["Vehicle", "VW, Audi, Seat, Škoda with CAN diagnostics (from about 2004)."],
], [40, 130])
h2("CAN adapter or ELM327?")
p("A cheap <b>ELM327</b> adapter (USB, Bluetooth, Wi-Fi) also works, but can do much less. "
  "For older VWs like the Golf 6 there is no way around the CAN adapter.")
tabelle(["Function", "CAN adapter", "ELM327"], [
    ["OBD-II, quick check, battery test", "yes", "yes"],
    ["UDS control units (faults, coding, service …)", "yes", "known modules only"],
    ["Find all control units automatically", "yes", "OBD addresses only"],
    ["TP 2.0: measuring blocks, favourites, engine checks, basic settings", "yes", "no"],
    ["CAN capture, auto-detection", "yes", "no"],
], [100, 30, 40])
klein("Both adapters were tested on the Golf 6; the ELM327 for reading only.")
h2("Further connections")
p("<b>Older control units behind the gateway</b> (e.g. ABS and power steering in the Polo 6R) sit on a "
  "K-line that the gateway passes through over CAN. ECUWerkbank detects this by itself during the scan — "
  "the normal CAN adapter is enough (tested on the Polo 6R).")
tabelle(["Connection", "What for", "Setting"], [
    ["DoIP (Ethernet)", "Newer cars with diagnostics over network (roughly from Golf 8). Needs a DoIP cable "
                        "(OBD to network plug).", "Connection → transport “DoIP” and IP address of the car"],
    ["CAN FD", "Newer control units with faster CAN. Only if the adapter supports CAN FD.",
               "Set up the adapter with <font face='DV-M'>fd on</font>, then <font face='DV-M'>VAG_CANFD=1</font>"],
    ["K-line (KKL cable)", "Very old VWs (before about 2004) with KW1281. In the terminal so far: identification, "
                           "faults, measuring groups.", "<font face='DV-M'>ecu-werkbank kline --port /dev/ttyUSB0 --adresse 0x01</font>"],
    ["J2534 adapter", "<b>Windows</b> only: adapter with J2534 driver (64 bit).",
                      "Connection → transport “J2534 adapter”"],
], [35, 80, 55])
klein("DoIP, KKL cable and J2534 have not been tested on a real car yet; CAN FD was tested on a virtual CAN bus.")
h2("Installing")
p("Build once in the project folder and install for your user (no sudo):")
code("cargo build --release", "./packaging/install.sh")
p("After that <b>ECUWerkbank</b> is in the start menu. Optionally <font face='DV-M'>./packaging/install.sh --can</font> "
  "sets up the adapter so that it is ready automatically when plugged in (asks for the password once).")

# ---------------------------------------------------------------- 3
h1("3  Connecting the adapter")
schritte(
    "Plug the adapter into the laptop, the cable into the OBD socket (on the Golf at the bottom left under the steering wheel).",
    "<b>Switch on the ignition.</b> You only need the engine running for live values, mixture and battery test.",
    "If you did not use <font face='DV-M'>install.sh --can</font>, switch the adapter on:",
)
code("sudo ip link set can0 type can bitrate 500000", "sudo ip link set can0 up")
p("In the GUI click <b>Connection</b> at the top. The connection assistant checks adapter, bitrate and whether a "
  "control unit answers, and tells you what to do.")
kasten("Adapter DSD TECH SH-C31x starts in update mode",
       "Sometimes this adapter reports itself as “STM Device in DFU Mode” after plugging in "
       "(<font face='DV-M'>lsusb</font>). Then there is no can0. This is how you get it back:"
       "<br/><font face='DV-M'>sudo dfu-util -a 0 -s 0x08000000:leave</font><br/>"
       "Wait about 2 seconds and then run the two <font face='DV-M'>ip link</font> commands from above.", GELB)

# ---------------------------------------------------------------- 4
h1("4  The interface")
p("Always at the very top: the <b>connection type</b> (real adapter or simulation), the <b>active vehicle</b>, "
  "the buttons <b>Connection</b>, <b>Quick check</b>, <b>Fault PDF</b>, the language and the light/dark switch. "
  "Below them the five areas:")
tabelle(["Area", "Pages"], [
    ["Start", "Vehicle at a glance"],
    ["Diagnostics", "Control units · Older control units (TP 2.0) · OBD-II · Coding · "
                    "Checks &amp; troubleshooting · Safety &amp; assistance · Other makes"],
    ["Measure", "Live graph · Battery · Engine checks · Trip recorder; for diesels target range and particulate filter"],
    ["Vehicle", "Vehicles &amp; maintenance · Service display · History &amp; trends · Software versions · "
                "Coding backup · Retrofit"],
    ["Tools", "Test run · CAN monitor · Scripts · Capture &amp; learning aid · Vehicle database · "
              "Fault patterns &amp; reference · Experts: routine &amp; output test (UDS)"],
], [32, 138])
h2("Language")
p("At the top right you choose the language (field with the globe). German and English are built in. You create "
  "another language with <font face='DV-M'>ecu-werkbank sprache --vorlage fr</font>: the file contains all texts, "
  "you enter the translation on the right. Empty lines stay German. On the next start the language can be chosen.")
h2("Try it without a car")
p("At the top choose <b>Demo vehicle</b> as the connection type. Next to it you can choose the model: "
  "“Demo vehicle” (completely invented) or <b>“Golf 6 (2010, simulated)”</b> — a replica of a real Golf 6 with the "
  "same 17 control units and faults. All pictures in this manual come from this simulation "
  "(example vehicle M-AB 123, invented VIN).")

# ---------------------------------------------------------------- 5
h1("5  Start page: the vehicle at a glance")
bild("start", "Start page with the example Golf: four cards, control units with faults, open maintenance "
              "and fault overview.", 132)
liste(
    "<b>Mileage</b> with date and kilometres driven per year.",
    "<b>Faults stored</b> — and whether there are more or fewer than in the previous scan.",
    "<b>Last scan</b> and <b>next maintenance</b> (yellow = due soon).",
    "<b>Control units with faults</b> — a click opens the control unit for reading or clearing.",
    "<b>Fault overview</b>: new, still present, fixed, and which faults keep coming back.",
)
p("Everything on the start page comes from the vehicle folder. The car does not need to be connected for this. "
  "<b>Mileage history</b> and <b>faults per scan</b> (from two scans) are under Vehicle → History &amp; trends.")
h2("Detect the car automatically")
p("At the top next to the adapter display it says “no car”. When you plug the adapter into the car and switch on "
  "the ignition, a car symbol with the vehicle appears after a few seconds, and ECUWerkbank selects it by itself. "
  "It only asks read-only and only when nothing else is running. A click on the display switches detection off "
  "or on again.")

# ---------------------------------------------------------------- 6
h1("6  Reading faults")
h2("Quickest: fault PDF")
p("Click <b>Fault PDF</b> at the top. ECUWerkbank queries all control units, saves a PDF in the vehicle folder "
  "and compares with the last scan. The first time it searches all addresses (Golf 6: about 50 seconds). After "
  "that it knows the control units of the car and only needs about 20 seconds (<b>quick scan</b>). With the "
  "<b>Full scan</b> checkbox everything is searched again — useful after installing a new control unit.")
bild("steuergeraete", "Diagnostics → Control units: all control units from the last scan with address, "
                      "part number and number of faults. A click reads the faults of this module live.", 140)
h2("What the details mean")
tabelle(["Detail", "Meaning"], [
    ["currently present", "The fault exists right now."],
    ["sporadic", "Was there, is not present right now (e.g. loose contact)."],
    ["being tested", "Not yet confirmed."],
    ["Fault type 12 (TP 2.0)", "Number of the fault type; VW defines its meaning."],
    ["Fault type 13: open circuit", "Standardised for UDS control units — ECUWerkbank adds it in plain text."],
    ["Warning lamp on", "The fault switches on a warning light."],
    ["NEW / still / FIXED", "Comparison with the last scan in which the control unit was readable."],
], [45, 125])
h2("Where do the fault texts come from?")
p("ECUWerkbank needs <b>no label files</b> for this. It searches in this order:")
schritte(
    "<b>Your own fault texts</b> — what you entered yourself (Vehicle database → “Own texts”).",
    "<b>Vehicle database</b> — your own labels and imported data packages (optional, chapter 12).",
    "<b>Built-in list</b> of the standardised OBD codes (Pxxxx). These texts are German.",
)
p("For the engine, ECUWerkbank converts the 5-digit manufacturer number into the OBD code. On the Golf 6, for "
  "example, <b>08851 = P2293</b> (fuel pressure regulator valve). The other control units (doors, parking aid …) "
  "report manufacturer-specific component numbers like 01627, for which there is no open list. Your own text helps "
  "here: next to a code without text there is a <b>✏</b> button. Click, write the text, Enter. It then appears "
  "everywhere, also in the PDF.")
h2("Freeze frame data")
p("For many faults the control unit stores <b>when and under which circumstances</b> it occurred: mileage, date "
  "and time, frequency, unlearn counter and some measured values. ECUWerkbank shows them indented below the fault, "
  "also for the older TP 2.0 control units of the Golf 6 (checked against VCDS).")
h2("Experience from your history")
p("If a fault has occurred more often for you, the guided troubleshooting shows a small <b>statistic</b>: how often "
  "it was there, how often it went away, how often it came back, and after which repair note it disappeared most "
  "often. It is built only from your own scans.")
kasten("Good to know",
       "If a control unit does not answer (e.g. radio off or just starting up), it is listed as “not readable” "
       "in the report. Its faults then do <b>not</b> count as fixed.")

# ---------------------------------------------------------------- 7
h1("7  Measuring")
h2("Live values (OBD-II)")
p("Diagnostics → OBD-II shows VIN, engine warning light, readiness (emission tests) and live values. Tick the "
  "values you want and start <b>Live</b>; with “Record CSV” everything is written along. The readiness assistant "
  "tells you how to complete open emission tests by driving.")
bild("obd", "Diagnostics → OBD-II with readiness, readiness assistant and live values.", 100)
h2("Measuring blocks (TP 2.0)")
p("Older control units deliver their values in <b>measuring blocks</b> with up to 8 fields. Under Diagnostics → "
  "Older control units (TP 2.0) click <b>Search control units</b>, choose an address (engine = 0x01) and read a "
  "block — once or continuously (<b>Live</b>). <b>Read all blocks</b> shows in one pass which blocks a control "
  "module has at all. With the <b>🔍</b> next to a field only this one value is shown large (<b>single measured "
  "value</b>) and updated faster — handy when you glance at it from a distance while working on the car.")
bild("tp20", "Diagnostics → Older control units (TP 2.0): all TP 2.0 control units found, with identification and number of faults.", 96)
h2("Favourites and your own names")
p("Without a description ECUWerkbank only shows the <b>kind</b> of a value, with “~” in front (e.g. “~Engine speed”). "
  "You enter the right name once yourself — with the <b>✏</b> next to the field or the block name. It then applies "
  "everywhere: live values, test run, recorder and graphs.")
p("Remember frequently used blocks with the <b>star</b> (☆ → ★) next to the block name. They then appear as a bar "
  "<b>★ Favourites</b> above the block; one click opens and reads the block immediately. Remove with ✖ or the star "
  "again.")
bild("favoriten", "Measuring block 2 of the engine with your own field names and the favourites bar above it.", 120)
p("All your own names at a glance — to change and delete — under Tools → Vehicle database → <b>Own texts</b> → "
  "own measured value names.")
bild("namen", "Tools → Vehicle database → Own texts: list of all saved measured value names.", 80)
h2("Battery and cranking test")
schritte("Ignition on, engine off.", "Measure → Battery → <b>Start test</b>.",
         "At “START THE ENGINE NOW” start the engine and let it run.")
bild("batterie", "Measure → Battery: voltage curve while cranking (simulation).", 76)
tabelle(["Measuring point", "Good", "Measured on the real Golf 6 (29.10.2026)"], [
    ["Before starting", "12.2–12.6 V", "11.7 V"],
    ["Lowest value while cranking", "above approx. 10 V", "<b>9.4 V — battery weak</b>"],
    ["After starting", "13.5–14.5 V", "13.8 V — alternator charging"],
], [55, 40, 75])
p("If the voltage drops this much, some control units (e.g. the radio) restart. They then do not answer, or "
  "answer late, for a few seconds.")
h2("Live graph and gauges")
p("Measure → <b>Live graph</b> records any values at the same time: OBD values, measuring blocks of older control "
  "modules and UDS identifiers. With <b>🕓 Gauges</b> they become round instruments, large numbers or bars (right-click "
  "on an instrument). You save a combination as a <b>view</b>. <b>⚠ Warn at limit</b> reports exceedances and records "
  "the 30 seconds before and after; <b>⏺ Record CSV</b> writes everything along.")
bild("scope", "Measure → Live graph with a measuring block of the engine (simulation).", 120)
bild("armaturen", "The same values as gauges.", 120)
h2("Live values on the phone (QR code)")
schritte("Start the values you want in the live graph.",
         "Tick <b>📱 Phone display</b> — a window with a QR code opens.",
         "Scan the QR code with the <b>phone camera</b> and open the link. The values run along in the browser.")
bild("handy-qr", "Live graph with the phone display switched on and the QR code to scan.", 120)
bild("handy", "This is what the phone display looks like in the phone's browser.", 140)
kasten("Good to know",
       "Phone and laptop must be on the <b>same Wi-Fi</b> — in the car the easiest is the phone's hotspot. If the ufw "
       "firewall is active, allow it once: <font name='DV-M'>sudo ufw allow from 192.168.0.0/16 to any port 8088 proto "
       "tcp</font>. The phone can only view, not control anything. The address contains a secret key — do not pass "
       "the QR code on.")

# ---------------------------------------------------------------- 8
story.append(PageBreak())
h1("8  Checks and troubleshooting")
bild("diagnose", "Diagnostics → Checks &amp; troubleshooting: used car check, guided troubleshooting, "
                 "repair mode, emission test results and mixture assistant.", 150)
h2("Mixture assistant")
p("Finds air leaks or problems with the fuel pressure. Engine at <b>operating temperature</b> (above 80 °C), all "
  "consumers off, car stationary. Phase 1: 20 seconds idle. Phase 2: hold about 2500 rpm for 20 seconds — the "
  "assistant shows “more throttle” or “less throttle”. Then the likely cause appears.")
h2("Emission test results (mode 06)")
p("Shows how close catalyst, lambda sensors and ignition are to their limits — before the engine light comes on. "
  "“Margin” means: this is how far the value still is from the limit.")
h2("Used car check")
p("One pass: all control units, faults, when last cleared, VINs in all control units, readiness, emission tests, "
  "battery and mileage from all sources. Result as a PDF “purchase advice”. With <b>From last scan</b> this also "
  "works without the car.")
h2("Repair mode")
p("Back up the state before → repair → clear faults → test drive → check afterwards. The PDF shows what is fixed and "
  "what is still there.")

# ---------------------------------------------------------------- 9
h1("9  Vehicle, maintenance and history")
h2("Vehicles &amp; maintenance")
p("Every car has its own folder, named after the VIN. A number plate helps to find it. Scans are assigned to the "
  "right car automatically — so e.g. your car and your partner's car stay cleanly separated.")
bild("fahrzeuge", "Vehicle → Vehicles &amp; maintenance: details, mileage, maintenance plan with traffic light "
                  "and contents of the vehicle folder.", 145)
liste(
    "<b>Template</b> creates the usual items (oil, filters, spark plugs, brake fluid, timing belt, inspection). "
    "The item names are stored in German.",
    "Click <b>done today</b> at an item — ECUWerkbank calculates when it is due again. Yellow = due soon, red = "
    "overdue. When the program starts, a hint appears if something is due.",
    "<b>Vehicle record (PDF)</b> and <b>ZIP backup</b> of the whole folder.",
)
h2("Company header and logo for reports")
p("For workshops: under Vehicle → Vehicles &amp; maintenance → <b>Company header for reports</b> enter name, address, "
  "phone, e-mail and an extra line, add a company logo (PNG or JPG) with <b>🖼 Choose logo…</b> and save. The company "
  "header then appears at the top of every page of all PDF reports — on page 1 on the right in the title bar with "
  "the logo.")
p("The same section holds the <b>workshop code</b> (workshop number/importer/equipment number, e.g. 12345/123/12345). "
  "ECUWerkbank sends it along when coding and adapting, just like a workshop tool. Many control units (e.g. the "
  "central electrics in the Golf 6) refuse without it — so ECUWerkbank does not write without a workshop code. "
  "It does not appear in reports.")
bild("bericht", "Fault memory report (PDF) with company header and logo (example data).", 150)
h2("All PDF reports at a glance")
p("ECUWerkbank writes these reports as PDF into the vehicle's <i>berichte</i> folder. All have the same structure: "
  "title bar with company header, vehicle details, then the content. The pictures each show the first page with "
  "the example data of the simulation.")
tabelle(["Report", "Where to create", "Content"],
        [["Fault memory report", "Diagnostics → Control units, “Save as PDF”", "all control units, faults with freeze frame data, comparison with the last scan"],
         ["Used car check", "Diagnostics → Checks &amp; troubleshooting", "traffic light verdict, check points, mileages, readiness, emission test values"],
         ["Vehicle record", "Vehicle → Vehicles &amp; maintenance", "maintenance, parts, costs, history of all scans, software versions"],
         ["Measured value recording", "Measure → Live graph, “As PDF”", "table min/max/mean and a chart for each curve"],
         ["Repair report", "Diagnostics → Checks &amp; troubleshooting, repair mode", "before/after, fixed and still open faults"],
         ["Retrofit report", "Vehicle → Retrofit", "what changed through the installation"],
         ["Spare part pass", "Vehicle → Coding backup, emergency backup", "part numbers, software, coding of all control units"]],
        [36, 60, 74])


def bilder_paar(links, rechts):
    """Two PDF pages side by side: each (name, caption)."""
    from PIL import Image as PILImage
    zellen = []
    for name, text in (links, rechts):
        if not name:
            zellen.append("")
            continue
        pfad = BILDER / f"{name}.png"
        w, h = PILImage.open(pfad).size
        breite = 80 * mm
        img = Image(str(pfad), width=breite, height=breite * h / w)
        innen = Table([[img]], colWidths=[breite + 2])
        innen.setStyle(TableStyle([("BOX", (0, 0), (-1, -1), 0.6, colors.HexColor("#999999")),
                                   ("LEFTPADDING", (0, 0), (-1, -1), 0), ("RIGHTPADDING", (0, 0), (-1, -1), 0),
                                   ("TOPPADDING", (0, 0), (-1, -1), 0), ("BOTTOMPADDING", (0, 0), (-1, -1), 0)]))
        zellen.append([innen, Paragraph(text, S["klein"])])
    t = Table([zellen], colWidths=[85 * mm, 85 * mm])
    t.setStyle(TableStyle([("VALIGN", (0, 0), (-1, -1), "TOP"),
                           ("LEFTPADDING", (0, 0), (-1, -1), 0), ("RIGHTPADDING", (0, 0), (-1, -1), 0)]))
    story.append(KeepTogether([Spacer(1, 4), t]))
    story.append(Spacer(1, 6))


bilder_paar(("pdf-kaufcheck", "Used car check"), ("pdf-fahrzeugakte", "Vehicle record"))
bilder_paar(("pdf-messwerte", "Measured value recording (CSV from the live graph)"), ("pdf-reparaturbericht", "Repair report"))
bilder_paar(("pdf-nachruestung", "Retrofit report"), ("pdf-ersatzteilpass", "Spare part pass from the emergency backup"))
h2("Mileage")
p("The scan reads the mileage by itself. On the Golf 6 the instrument cluster only gives it out <b>rounded down to "
  "10 km</b> (odometer 242 488 km → 242 480 km). You can enter the exact value by hand — or let <b>Read exactly from "
  "the cluster</b> search once for the place where the cluster keeps it km-exact (read only). A later scan only "
  "overwrites the reading if it is higher.")
h2("Service display (inspection / oil change)")
bild("service", "Vehicle → Service display: instructions for resetting with the buttons and the one-time "
                "setup.", 130)
p("The page reads from the instrument cluster in how many km and days inspection and oil change are due. Overdue "
  "values are red, those due soon (below 1000 km or 30 days) yellow.")
p("VW does not publish under which identifiers the cluster stores these values. ECUWerkbank therefore learns them "
  "<b>once per vehicle</b> — best at the next oil change:")
schritte(
    "Ignition on. Expand “Set up reset”, <b>remember state “before”</b> (reads for about a minute, read only).",
    "Reset the service as usual <b>with the buttons on the cluster</b> (instructions are on the page).",
    "<b>Remember state “after”</b>, then <b>Compare</b>.",
    "ECUWerkbank shows the identifiers that changed and suggests what they mean. Compare with the display in the "
    "cluster, then <b>Take over into the vehicle</b>.",
)
p("After that ECUWerkbank can reset the service by itself: tick, <b>Reset service</b>. The old values are backed up "
  "before and read back afterwards; the oil change is entered as done in the maintenance plan (can be switched off). "
  "<b>Undo last reset</b> writes the backed-up values back. If the cluster requires a login, ECUWerkbank stops — "
  "then the way via the buttons remains.")
h2("History &amp; trends")
bild("historie", "Vehicle → History &amp; trends: mileage and fault history, all scans with date, km and "
                 "number of faults, comparison of two scans.", 110)
h2("Software versions and coding backup")
p("<b>Software versions</b> lists part number, software and hardware version of every control unit and compares "
  "with last time — so you see whether something was replaced or reprogrammed at the workshop.")
bild("software", "Vehicle → Software versions.", 72)
p("<b>Coding backup</b> saves the coding of all control units and compares two backups. Before every change to the "
  "car: back up first. Individual control units can be written back from it. <b>🔀 Compare whole configuration</b> "
  "puts coding, adaptations and software version of a control unit from two backups side by side and only shows "
  "what differs.")
bild("sicherung", "Vehicle → Coding backup: compare backups and write back individually.", 145)
h2("Where are the files?")
p("Since version 0.4 all details, scans, backups and settings are stored in <b>one file</b>, the vehicle database. "
  "Only documents (PDF, CSV recordings, captures) remain separate files in the vehicle folders.")
code("~/.local/share/ecu-werkbank/",
     "    fahrzeugdaten.sqlite   vehicles, scans, backups, own labels and texts",
     "    fahrzeuge/<VIN>/berichte/   PDFs and logs",
     "    vor-datenbank/         the files from before version 0.4 (just to be safe)")
p("Vehicle → Vehicles &amp; maintenance → <b>Data backup</b> writes everything as a ZIP with readable files, e.g. to a "
  "USB stick, and restores it too.")

# ---------------------------------------------------------------- 10
h1("10  Test run and safety &amp; assistance")
h2("Test run")
p("Tools → Test run queries everything important in one pass and writes a log with raw data: connection, VIN, all "
  "control units with fault memory, measuring blocks 1–30 of the engine and the mileage in the instrument cluster. "
  "Enter the odometer reading, then ECUWerkbank compares it with the cluster. The test run only reads and takes "
  "about a minute.")
bild("testlauf", "Tools → Test run.", 45)
h2("Safety &amp; assistance")
p("Summarises airbag, ABS/ESP, steering, parking aid and assistance systems with their faults — read only. Work on "
  "airbag systems belongs in a specialist workshop.")
bild("sicherheit", "Diagnostics → Safety &amp; assistance.", 60)

# ---------------------------------------------------------------- 11
story.append(PageBreak())
h1("11  Tools and reference")
p("These pages help with understanding and investigating, without changing anything on the car.")
h2("CAN monitor")
p("Tools → <b>CAN monitor</b> shows all messages on the bus live: per CAN ID the data, how often they come and which "
  "bits change. It only listens, nothing is sent. To find a signal first <b>remember noise</b>, then operate "
  "something on the car (indicator, window) — only the bits that change because of it stay highlighted. Names and "
  "signals come from the vehicle database (imported DBC files) or from your own DBC file. Needs the CAN adapter; on "
  "the Golf 6 usually only the diagnostic traffic arrives at the OBD socket.")
bild("canmonitor", "Tools → CAN monitor (simulation).", 120)
h2("Scripts")
p("For your own sequences: small programs in the Rhai language, e.g. “read these three values, compare, repeat”. "
  "Scripts can <b>only read</b> — writing to the car only works via the normal pages with confirmation.")
bild("skripte", "Tools → Scripts with the bundled example.", 110)
h2("Extended identification")
p("The button <b>🆔 Extended identification</b> at the selected control unit reads all standardised identifiers of a "
  "<b>UDS</b> control unit (VIN, part numbers, software and hardware version, serial number, coding …) and names "
  "them. For TP 2.0 control units the identification is in the TP 2.0 tab.")
bild("ident", "Extended identification of the instrument cluster (0x714).", 80)
h2("Other makes")
p("Under Connection → <b>Other makes</b> ECUWerkbank also searches the control units of Mercedes, Opel, Ford &amp; "
  "co. and reads identification and fault memory (standardised UDS and OBD diagnostics). The brand is recognised from "
  "the VIN, with a hint on what works for it. Manufacturer texts are only available from your own data packages. Such "
  "scans are kept separately and do not disturb the history of the car.")
h2("Reading memory (experts)")
p("In the terminal <font face='DV-M'>speicher-lesen</font> reads a memory area of a UDS control unit and saves it as "
  "a file — <b>read only</b>. Many control units refuse this; after several refusals ECUWerkbank stops by itself.")
h2("Interface for your own programs (REST)")
p("<font face='DV-M'>ecu-werkbank api</font> provides the vehicle database as an interface in the home network (with a "
  "secret key). With <font face='DV-M'>--live</font> OBD live values from the car are added, e.g. "
  "<font face='DV-M'>/api/v1/live/obd?pids=0C,0D</font>. Read only.")
h2("Basic settings (reference)")
p("A lexicon of the usual workshop functions per system: throttle body adaptation, steering angle sensor, bleeding "
  "the ABS, parking brake, service interval … For each it says what it is for, what to watch out for and how "
  "delicate it is (harmless / with care / safety-relevant). <b>Nothing is executed here.</b> The lexicon texts are "
  "German.")
bild("lexikon", "Tools → Fault patterns &amp; reference: basic settings by system, searchable.", 140)

# ---------------------------------------------------------------- 12
h1("12  Vehicle database and your own descriptions")
p("Tools → <b>Vehicle database</b> is the place for all knowledge about vehicles: what ECUWerkbank brings along, what "
  "you import and what you enter yourself. Every <b>source</b> is listed with kind and licence and can be removed again "
  "as a whole. Data without a licence always stays on your computer only.")
bild("datenbank", "Tools → Vehicle database: overview, sources, import and part number search.", 130)
h2("Importing data — with preview")
p("<b>📂 Import file…</b> takes DBC files (CAN messages), data packages (TOML or JSON), CSV lists, label files and "
  "ODX/PDX data you bought yourself. The kind is detected. Before taking it over, a <b>preview</b> shows what would be "
  "entered where: manufacturer, address, control unit with fault texts, measured values, coding bits and "
  "adaptations. Only <b>Take over</b> writes into the database. Files can also simply be dragged into the window. A "
  "template with everything a data package can contain is included: <font name='DV-M'>datenpaket-alles.toml</font>.")
bild("datenbank-vorschau", "Preview before importing the template datenpaket-alles.toml.", 130)
h2("Viewing and editing content")
p("Choose a source, click an entry on the left, change it on the right: name, address, part number, CAN identifiers, "
  "coding identifier, fault texts, measured values, coding bits (also “needs” and “not with”), adaptation channels and "
  "<b>target values</b> for the traffic light at measured values. <b>+ Control unit</b> creates a new one.")
bild("datenbank-inhalt", "A control unit of the template in the editor.", 130)
kasten("The part number is the key",
       "On the car ECUWerkbank looks up the names of adaptation channels and coding bits by the <b>part number</b> of the "
       "control unit (e.g. 5K0937086) — it is in every identification. So do not forget the part number when "
       "entering data. With <b>🔎 Part number</b> you see what the database knows about a control unit.")
p("You can also create descriptions yourself — as much or as little as you like. There are three ways:")
tabelle(["Way", "What for", "Where"], [
    ["Own names", "Names of measuring blocks and their fields", "✏ at the measuring block; list: Vehicle database → Own texts"],
    ["Own fault texts", "Plain text for a fault code", "✏ next to the code; list: Vehicle database → Own texts"],
    ["Own labels", "Everything about a control unit: name, fault texts, measured values, coding bits, target values", "Vehicle database → View and edit content"],
], [35, 65, 70])
h2("Own labels and “Create from last scan”")
p("The button <b>⬇ Create from last scan</b> creates an entry in the source “Own labels” for every control unit from "
  "the last scan of the active vehicle — with address, name and part number from <b>your</b> scan. Control units that "
  "already have an entry are skipped; nothing is overwritten. Then choose the entry, add fault texts, measured values "
  "and target values, <b>Save</b>.")
bild("editor", "Tools → Vehicle database, source “Own labels” with the entries created from the scan.", 75)
kasten("Good to know",
       "Own labels are assigned by <b>address</b>, not by car. If two cars have different control units at the same "
       "address, measured value names can overlap. Fault texts and coding are usually not affected.")
# ---------------------------------------------------------------- 13
story.append(PageBreak())
h1("13  Functions that change something on the car")
kasten("Caution",
       "The following functions <b>write</b> into the control unit. Only when stationary, ignition on, engine off "
       "(except basic settings that require something else), battery well charged — connect a charger if the battery "
       "is weak. ECUWerkbank backs up automatically beforehand and <b>always asks first</b>. In the terminal, writing "
       "commands without <font face='DV-M'>--ja</font> only show what they would do.", ROT)
tabelle(["Function", "Where", "Note"], [
    ["Clear faults (one control unit)", "Diagnostics → Control units", "Note the faults first or save them as PDF."],
    ["Clear all faults", "Diagnostics → Control units", "Also clears the OBD readiness. Then several drives are needed "
                                                          "before an emissions test."],
    ["Write / restore coding", "Diagnostics → Coding, Vehicle → Coding backup", "Only with a backup before. "
                                                                               "Workshop code needed (chapter 9)."],
    ["Change adaptation (TP 2.0)", "Diagnostics → Older control units (TP 2.0) → Adaptation", "Workshop code needed. "
                                                                                              "Tested on the Golf 6 (central electrics)."],
    ["Routine, output test (UDS)", "Tools → Experts: routine &amp; output test (UDS)", "Only if you know what the "
                                                                                     "control unit does."],
    ["Basic settings (TP 2.0)", "Diagnostics → Older control units (TP 2.0)", "Only enabled after your own capture. "
                                                                                 "Throttle body tested on the Golf 6."],
    ["Reset service", "Vehicle → Service display", "Only after learning. Old values are backed up, undo is possible."],
], [50, 55, 65])
kasten("Mileage",
       "ECUWerkbank <b>never</b> writes the mileage. Every write path — coding, adaptations, service values and the "
       "general write command — rejects the mileage identifiers, also with <font face='DV-M'>--ja</font>. Odometer "
       "tampering is a criminal offence.", GELB)
kasten("Voltage guard",
       "For sequences with many write operations (e.g. writing back several adaptations) ECUWerkbank measures the "
       "supply voltage not only at the start but again at most every 10 seconds. If it falls below 12 V, it stops "
       "<b>before</b> the next write — nothing stays half written.")
h2("Routine and output test (UDS)")
p("In the tab Tools → Experts: routine &amp; output test (UDS) you enter control unit, routine ID or identifier and "
  "values by hand. With <b>Sub</b>, 1 = start, 2 = stop, 3 = request result. Before sending, a yellow box always "
  "appears with exactly what will be sent. Only <b>Yes, send</b> sends it.")
bild("rueckfrage", "The confirmation before sending a routine.", 70)
h2("Basic settings on TP 2.0 control units (function 04)")
p("Which requests VW sends for this over TP 2.0 is not reliably published. ECUWerkbank does not guess, but <b>learns "
  "the sequence from your own capture</b>. Until then sending is locked for this control unit (padlock symbol).")
bild("grundeinstellung", "Diagnostics → Older control units (TP 2.0): the section “Basic settings”, still locked for the engine (0x01).", 100,
     ausschnitt=GRUND_AUSSCHNITT)
schritte(
    "Connect the CAN adapter and the cable of a workshop diagnostic tool to the car at the same time — this needs an <b>OBD Y-cable</b>.",
    "Tools → Capture &amp; learning aid: <b>Record capture</b>.",
    "With the diagnostic tool, choose basic settings (function 04) on this control unit, switch on a harmless group "
    "(e.g. throttle body adaptation), let it run for a few seconds, switch it off.",
    "Stop recording. In ECUWerkbank enter the same group, <b>Analyse capture…</b>, check the result and <b>Accept</b>.",
)
p("After that every group of this control unit can be started (with confirmation), the values run live, <b>Off</b> "
  "ends it. Only what the control unit confirmed in the capture is taken over; when starting only the group number is "
  "exchanged. The sequence only applies to <b>this</b> vehicle.")
p("Some workshop tools only switch on the basic setting once the values are already running (button “ON”). "
  "ECUWerkbank learns that as well and sends it after the first query. Tested this way on the Golf 6: "
  "<b>throttle body adaptation</b> (engine, group 60) — the same course as with the workshop tool, “ADP OK” at the "
  "end. In the terminal use <font face='DV-M'>kwp-grundeinstellung</font> (chapter 15).")
bild("mitschnitt", "Tools → Capture &amp; learning aid with an example capture.", 95)

# ---------------------------------------------------------------- 14
h1("14  Problems and solutions")
tabelle(["Problem", "Cause and solution"], [
    ["No can0", "Adapter in update mode → <font face='DV-M'>sudo dfu-util -a 0 -s 0x08000000:leave</font>, then the "
                "<font face='DV-M'>ip link</font> commands (chapter 3)."],
    ["Bus-off right at the first send", "CAN high and CAN low swapped (pin 6 / pin 14) or wrong bitrate."],
    ["“ERROR-PASSIVE”, no answer", "Ignition off or plug not seated properly."],
    ["Only a few control units found", "Ignition off: on the Golf 6 only 4 of 17 are awake then. Switch the ignition on "
                                         "and scan again. The list for the quick scan never gets smaller this way."],
    ["“temporarily not available”", "The control unit is briefly busy (Golf 6 engine up to 3 s after an OBD query). "
                                    "ECUWerkbank waits and tries again."],
    ["Radio does not answer", "Radio is off or restarts after cranking. Switch it on, wait a moment, read again."],
    ["Mileage differs by up to 9 km", "The cluster rounds down to 10 km. Read exactly from the cluster or enter by hand "
                                      "(chapter 9)."],
    ["No freeze frame data for a fault", "Not every control unit stores them for every fault. That is not an error."],
    ["Stop “voltage too low”", "Connect a charger, wait a moment and start the sequence again (chapter 13)."],
    ["Basic settings locked", "There is no learned sequence for this control unit yet (chapter 13)."],
    ["Writing refused (“conditions not correct”)", "Usually the workshop code is missing — enter it under Vehicle → "
                                                   "Vehicles &amp; maintenance → Company header (chapter 9)."],
    ["Field is called “~Engine speed” instead of a name", "No own name entered yet — ✏ at the field (chapter 7)."],
    ["GUI hangs / shows nothing new", "Read the status line at the bottom. If needed, close the GUI and start it again; "
                                      "no data is lost."],
], [55, 115])

# ---------------------------------------------------------------- 15
h1("15  Commands for the terminal")
p("Everything from the GUI also works in the terminal. Without an adapter: add <font face='DV-M'>-t demo</font> or "
  "<font face='DV-M'>-t golf6</font>. Help for every command: <font face='DV-M'>ecu-werkbank &lt;command&gt; --help</font>. "
  "Command names and options are German; <font face='DV-M'>--sprache en</font> switches the output to English.")
tabelle(["Command", "What it does"], [
    ["verbindung / detect", "Connection assistant / which kind of diagnostics the car speaks"],
    ["report", "Fault memory of all control units as PDF (quick scan; <font face='DV-M'>--voll</font> = everything)"],
    ["check", "Quick check with traffic light and PDF"],
    ["testlauf --km 242488", "Test run with odometer reading"],
    ["obd-info / obd-live", "OBD-II: readiness / live values (<font face='DV-M'>--pids 0C,0D,05</font>, hexadecimal)"],
    ["kwp-dtc / kwp-block", "TP 2.0: faults / measuring blocks (<font face='DV-M'>--address 0x01 --block 1,2</font>)"],
    ["ident --geraet 714", "Extended identification of a UDS control unit"],
    ["grundeinstellung", "Lexicon of basic settings (lookup only)"],
    ["batterie", "Battery and cranking test"],
    ["gemisch", "Mixture assistant"],
    ["obd-tests", "Emission test results (mode 06)"],
    ["kaufcheck", "Used car check with PDF"],
    ["software", "Software versions of all control units"],
    ["sicherheit", "Safety &amp; assistance"],
    ["coding-backup / coding-compare", "Back up coding / compare backups"],
    ["km 242488 / km-genau", "Enter mileage by hand / read exactly from the cluster"],
    ["service", "Read service display; <font face='DV-M'>service merken --name vorher</font>, "
                "<font face='DV-M'>service vergleich --uebernehmen</font>, <font face='DV-M'>service anleitung</font>"],
    ["wartung / wartung-vorlage / wartung-erledigt", "Maintenance plan"],
    ["history / compare", "Scans of a vehicle / compare two scans"],
    ["sprache --vorlage fr", "Template for a new language; <font face='DV-M'>--verwaist</font> / "
                             "<font face='DV-M'>--aufraeumen</font> maintain a language file"],
    ["rdid-scan --tx 0x714 --rx 0x77E --von 0x2200 --bis 0x22FF", "Read identifiers of a control unit (read only)"],
    ["fremd", "Other makes: search control units, read identification and faults"],
    ["kline --adresse 0x01 --gruppe 1", "Very old VWs via K-line (KKL cable)"],
    ["speicher-lesen --geraet 0x7E0 --von 0x0 --laenge 256", "Read memory of a UDS control unit (experts)"],
    ["api --live", "REST interface with database and live values"],
    ["pruefplan --code P0300", "Test plan for a fault, with statistics from your history"],
    ["firmenkopf --werkstattcode 12345/123/12345", "Enter the workshop code for writing"],
], [72, 98])
h2("Commands that write")
p("<b>Without</b> <font face='DV-M'>--ja</font> (yes) these commands only show a preview of what they would send:")
code("ecu-werkbank routine   --tx 0x7E0 --rx 0x7E8 --id 0x0203 --sub 1      # preview",
     "ecu-werkbank routine   --tx 0x7E0 --rx 0x7E8 --id 0x0203 --sub 1 --ja # really send",
     "ecu-werkbank iocontrol --tx 0x7E0 --rx 0x7E8 --did 0x0101 --control 0x03 --state 01",
     "ecu-werkbank code-write --tx 0x714 --rx 0x77E --did 0x0600 --bytes 01",
     "ecu-werkbank service zuruecksetzen",
     "ecu-werkbank kwp-langcodierung      --address 20 --neu <hex>      # long coding (TP 2.0)",
     "ecu-werkbank kwp-anpassung-schreiben --address 20 --kanal 23 --wert 10",
     "ecu-werkbank kwp-grundeinstellung   --address 01 --gruppe 60      # learned with --lernen <capture>")
klein("Likewise clear, kwp-clear, obd-clear, fehler-loeschen, coding-restore, codier-bit, anpassung-zurueck and stellen "
      "ask first. Every one of these paths rejects mileage identifiers. See chapter 13.")

# ---------------------------------------------------------------- Appendix
story.append(PageBreak())
h1("Appendix: control units in the Golf 6 (2010)")
p("This is how the Golf 6 (1.4 TSI, built 2010) on which ECUWerkbank was tested reports itself. With TP 2.0 the "
  "“address” is a logical address; it is not the same as the usual diagnostic address.")
tabelle(["Protocol", "Address / ID", "Control unit", "Part number"], [
    ["UDS", "0x711", "Immobiliser", "5K0920860F"],
    ["UDS", "0x714", "Instrument cluster (km: identifier 0x2203)", "5K0920860F"],
    ["UDS", "0x715", "Airbag", "5K0959655"],
    ["UDS", "0x746", "Air conditioning (Climatronic)", "5K0907044BD"],
    ["UDS", "0x76C", "Radio/navigation (RNS 310)", "3C0035270"],
    ["TP 2.0", "0x01", "Engine control unit (MED17.5.5)", "03C906016"],
    ["TP 2.0", "0x03", "ABS/ESP (MK60EC1)", "1K0907379CD"],
    ["TP 2.0", "0x09", "Power steering", "1K0909144H"],
    ["TP 2.0", "0x1D", "Parking aid", "1K9919475"],
    ["TP 2.0", "0x1F", "Gateway (J533)", "1K0907530AA"],
    ["TP 2.0", "0x20", "Central electrics", "5K0937086"],
    ["TP 2.0", "0x22–0x25", "Door control units (4×)", "1K0959701AE …704AG"],
    ["TP 2.0", "0x2A", "Steering column electronics (J527)", "1K0953549CQ"],
    ["TP 2.0", "0x43", "Trailer control unit", "1K0907383F"],
], [22, 27, 71, 50])
klein("These part numbers apply to the tested car. Other equipment levels have other control units.")
klein("ECUWerkbank is an independent project and not affiliated with Volkswagen AG or Ross-Tech. Trademarks mentioned "
      "belong to their owners. Licence: MIT or Apache-2.0, © 2026 dflow81.")
