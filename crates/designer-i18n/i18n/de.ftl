# DrawnUI Designer, Deutsch. Quelle: en.ftl (gleiche Schlüssel).

## Window

app-title = DrawnUI Designer
apps = Apps
help = Hilfe
settings = Einstellungen
back-to-designer = { $page }, zurück zum Designer

## Panels

panel-screens = Seiten
panel-layers = Ebenen
panel-properties = Eigenschaften
resize-screens = Breite von Seiten ändern
resize-properties = Breite von Eigenschaften ändern
screen-open = { $title } öffnen
controls-count = { $count ->
    [one] { $count } Steuerelement
   *[other] { $count } Steuerelemente
}
empty-screens-title = Keine Seiten
empty-screens-hint = Hier erscheinen die Routen der Shell der App.
empty-app-title = Keine App verbunden
empty-app-hint = Starte eine DrawnUI-App mit dem Designer-Agenten oder starte die letzte erneut.
empty-selection-title = Nichts ausgewählt
empty-selection-hint = Wähle in Ebenen ein Steuerelement, um seine Eigenschaften zu sehen und zu ändern.

## Title bar apps

app-building = wird gebaut…
app-starting = startet…
app-failed = fehlgeschlagen
app-restart = { $name } neu starten
app-run = { $name } starten
app-starting-name = { $name } startet…
app-shown = { $name }, { $detail }, angezeigt
app-show = { $name } anzeigen, { $detail }
app-run-detail = { $name } starten, { $detail }

## Layers

pick = Auswählen
pick-active = In der App klicken oder tippen
pick-label = Ein Steuerelement in der App auswählen
fold = { $name } einklappen
unfold = { $name } ausklappen
row-level = Ebene { $level }
row-folded = eingeklappt
row-open = ausgeklappt
row-selected = ausgewählt

## Properties

order = Reihenfolge der Eigenschaften
order-set = Gesetzt
order-groups = Typ
group-changed = Gesetzt
group-others = Nicht gesetzt
group-layout = Layout
group-text = Text
group-look = Aussehen
group-transform = Transformation
group-behavior = Verhalten
group-row = { $group }, { $count ->
    [one] { $count } Eigenschaft
   *[other] { $count } Eigenschaften
}, { $state }
group-open = ausgeklappt
group-folded = eingeklappt
prop-changed = in der App geändert
prop-set = gesetzt
value = Wert
decrease = Verringern
increase = Erhöhen
undo-prop = { $name } rückgängig machen
write-prop = { $name } in den Code schreiben

## Status line

status-connected = { $count ->
    [one] Hub verbunden · { $count } App
   *[other] Hub verbunden · { $count } Apps
}
status-connecting = Verbindung zum Hub…
status-other-hub = Mit dem Hub eines anderen Designers verbunden (Version { $version }); schließe ihn, um diesen zu verwenden.
status-other-hub-older = Mit dem Hub eines älteren Designers verbunden; schließe ihn, um diesen zu verwenden.
status-run-failed = { $name } fehlgeschlagen: { $message }
status-live = { $prop } = { $value } in der App, noch nicht im Code
status-writing = { $prop } wird in den Code geschrieben…
status-cancelled = { $prop }: Änderung verworfen
status-written = Geschrieben in { $file }, Zeile { $line }
status-not-written = Nicht in den Code geschrieben: { $reason }
status-not-undone = Im Code nicht rückgängig gemacht: { $reason }
status-undoing = { $prop } wird im Code rückgängig gemacht…
status-undone-code = { $prop } wieder { $value } in { $file }
status-undone-app = { $prop } wieder { $value } in der App

## Settings

settings-sections = Bereiche der Einstellungen
section-appearance = Darstellung
section-layers = Ebenen
section-connection = Verbindung
section-about = Info
appearance-hint = Wie der Designer aussieht.
theme = Design
theme-hint = Die Farben des ganzen Fensters. Das neue Design öffnet sich dort, wo du klickst.
theme-dark = Dunkel
theme-midnight = Mitternacht
theme-graphite = Graphit
theme-spaceship = Raumschiff
theme-light = Hell
theme-paper = Papier
theme-high-contrast = Hoher Kontrast
language = Sprache
language-hint = Die Sprache des ganzen Fensters.
language-system = System ({ $language })
layers-hint = Der Baum der Steuerelemente der Seite, die die App zeigt.
library-parts = Teile der Bibliothek
library-parts-hint = Auch die Steuerelemente zeigen, die DrawnUI selbst baut, etwa die innere Beschriftung eines Buttons. Kein Code von dir erstellt sie, daher können ihre Werte nicht in den Code geschrieben werden.
connection-hint = Wo Apps und KI-Agenten den Designer erreichen. Laufende Apps verbinden sich selbst.
connection-apps = Apps
connection-mcp = MCP-Server für KI-Agenten
connection-claude = Zu Claude Code hinzufügen
copy = Kopieren
copied = Kopiert

## About

about-logo = DrawnUI-Logo
about-version = Version { $version }
about-description = Sieh die Seiten, Steuerelemente und Eigenschaften einer laufenden DrawnUI-App, ändere sie live und schreibe jede Änderung zurück in ihren Code.
made-with = Gebaut mit
uses = Nutzt
credit-drawnui-rust = Die gezeichnete UI-Engine auf Skia: ein Code für Desktop, Telefone und das Web.
credit-nx = Sein Sprachprotokoll: UI-Quelltext als Baum von Knoten gelesen und Knoten für Knoten bearbeitet.
credit-tree-sitter = Liest und bearbeitet den C#-Code der App und lässt jedes Byte, das sich nicht ändert, wie es war.
credit-mcp = Der Model-Context-Protocol-Server, mit dem KI-Agenten eine laufende App sehen und ändern.
credit-lucide = Die Symbole.

## Help

help-title = Den Designer verwenden
help-intro = Sieh die Seiten, Steuerelemente und Eigenschaften einer laufenden DrawnUI-App, ändere sie live und schreibe jede Änderung in den C#-Code der App.
help-connect = Eine App verbinden
help-connect-1 = Starte deine App mit dem Designer-Agenten: Sie verbindet sich selbst und erscheint in der Titelleiste.
help-connect-2 = Eine App, die schon lief, bleibt dort ebenfalls: Klicke darauf, um sie erneut zu starten. Der runde Pfeil einer laufenden App startet sie neu.
help-connect-3 = Sind mehrere Apps verbunden, klicke eine an, um sie hier zu zeigen; auf einem Computer kommt ihr Fenster nach vorn.
help-phones = Telefone
help-phones-1 = Ein iPhone oder iPad am USB-Kabel verbindet sich von selbst: Starte darauf einen Debug-Build deiner App mit dem Designer-Agenten. Kein WLAN nötig.
help-phones-2 = Auswählen, dann ein Steuerelement auf dem Telefon antippen, um es hier auszuwählen. Zum Beenden erneut auf Auswählen drücken.
help-phones-3 = Ohne Kabel: Einstellungen, Verbindung, Apps auf anderen Geräten zeigt die Adresse, mit der die App gebaut wird.
help-screens = Seiten
help-screens-1 = Die Seiten, die die Shell der App öffnen kann. Klicke eine an, um sie in der App zu öffnen.
help-layers = Ebenen
help-layers-1 = Die Steuerelemente der Seite, die die App zeigt, als Baum. Klicke eines an, um es auszuwählen: In der App erscheint ein Rahmen darum.
help-layers-2 = Doppelklicke eine Zeile oder klicke ihr Kästchen auf der Baumlinie, um sie ein- oder auszuklappen.
help-layers-3 = Auswählen, dann ein Steuerelement in der App anklicken oder auf einem Telefon antippen, um es hier auszuwählen.
help-properties = Eigenschaften
help-properties-1 = Klicke einen Wert an, um ihn zu ändern: Tippe ihn ein, nutze - und + bei Zahlen, wähle aus einer Liste oder wähle eine Farbe.
help-properties-2 = Enter setzt ihn in der laufenden App. Esc stellt ihn wieder her, wie er war.
help-properties-3 = Geänderte Eigenschaften erscheinen heller. Gesetzt zeigt sie zuerst (Gesetzt, dann Nicht gesetzt), Typ nach Art.
help-write = In den Code schreiben
help-write-1 = Der Code-Button einer Zeile schreibt den Wert in die C#-Datei, die das Steuerelement erstellt: Nur dieser Wert ändert sich, der Rest der Datei bleibt, wie er war.
help-write-2 = Rückgängig nimmt die letzte Änderung einer Zeile zurück: in der App und nach dem Schreiben auch in der Datei.
help-write-3 = Die Zeile unten sagt, welche Datei sich geändert hat oder warum nichts geschrieben wurde.
help-write-4 = Strg+Z nimmt deine letzte Aktion zurück, welche es auch war: eine Änderung, ein Schreiben, ein Einfügen, ein Löschen. Strg+Y führt sie wieder aus. Das Menü Bearbeiten nennt beide.
help-keyboard = Tastatur
help-keyboard-1 = Tab wechselt zwischen Gruppen, die Pfeiltasten bewegen sich in einer Liste oder einem Menü, Enter oder Leertaste führt aus.
help-keyboard-2 = Esc schließt ein Menü, beendet eine Bearbeitung und führt aus Einstellungen oder Hilfe zurück.
help-keyboard-3 = Die Menüleiste: Klicke einen Titel an oder drücke Alt mit seinem unterstrichenen Buchstaben. Die Einträge zeigen ihre Tasten, die auch bei geschlossenen Menüs wirken.
help-agents = KI-Agenten
help-agents-1 = Agenten nutzen dieselben Funktionen über MCP unter { $address }.
help-agents-2 = Einstellungen, Verbindung zeigt die Adresse und den Befehl, der sie zu Claude Code hinzufügt.

## Color picker

picker = Farbe
picker-plane = Sättigung und Helligkeit
picker-plane-value = Sättigung { $saturation } %, Helligkeit { $brightness } %
picker-hue = Farbton
picker-opacity = Deckkraft
picker-hex = Hex-Farbe
picker-old-new = Vorher: { $old }, rechts die neue
picker-keys = Pfeiltasten bewegen den gewählten Balken oder das Quadrat (Umschalt: zehn Schritte). Esc stellt die alte Farbe wieder her.

## Hub

src-not-number = `{ $text }` ist keine Zahl
src-not-bool = `{ $text }` ist weder true noch false
src-not-color = `{ $text }` ist keine Farbe im Format #RGB, #RRGGBB oder #AARRGGBB
src-not-thickness = `{ $text }` ist kein Abstand ("alle", "h,v" oder "l,o,r,u")
src-type-unsupported = Werte vom Typ { $type } können noch nicht in den Code geschrieben werden
src-not-in-tree = das Steuerelement ist nicht im letzten Baum der App
src-library-part = die Bibliothek hat dieses Steuerelement für ihre eigenen Teile gebaut: kein Code deklariert es
src-no-code = kein Code gefunden, der dieses Steuerelement erstellt
src-arg-no-value = { $prop } kommt aus dem Argument `{ $param }` von `{ $call }`, das hier keinen Wert hat
src-several-args = { $prop } wird aus mehreren Argumenten von `{ $call }` gespeist
src-key-missing = `{ $key }` steht nicht im Code
src-no-value = die Eigenschaft hat keinen Wert
src-not-literal = `{ $key }` ist kein Literal, das der Designer ändern kann (ein Ausdruck oder ein Aufruf)
src-value-misfit = der Wert passt nicht zu `{ $key }` (eine andere Art von Literal)
src-not-single = die Änderung ist nicht eine einzige Änderung
src-empty-parens = leere Klammern
src-rewrites-colors = { $prop } wird durch `{ $code }` gesetzt; der Designer schreibt nur Color.Parse("#..") und Colors.X um
src-rewrites-thickness-numbers = { $prop } wird durch `{ $code }` gesetzt; der Designer schreibt nur Thickness-Argumente um, die Zahlen sind
src-rewrites-new-thickness = { $prop } wird durch `{ $code }` gesetzt; der Designer schreibt nur new Thickness(..) mit Zahlenargumenten um
src-no-arguments = keine Argumente
src-with-not-numbers = With{ $prop }(..) hat Argumente, die keine Zahlen sind
src-with-misfit = With{ $prop }(..) nimmt diesen Wert nicht an
src-insert-no-list = der Code, der die Kinder dieses Steuerelements erstellt, ist keine Liste, die der Designer ergänzen kann (eine `Children`-Liste, eine `new List<SkiaControl>` oder `.WithChildren(..)`)
src-insert-no-initializer = `{ $key }` wird ohne Objektinitialisierer erstellt, daher kann der Designer ihm keine Kinder geben
src-insert-templated = dieses Steuerelement erstellt seine Kinder aus einer Vorlage: füge stattdessen ein Element zu seiner ItemsSource hinzu
src-insert-unmapped = die Kinder dieses Steuerelements entsprechen nicht eins zu eins seinem Code (eine Schleife oder ein Helfer erstellt mehrere), daher weiß der Designer nicht, wohin das neue Steuerelement gehört
src-shared-helper = { $helper } erstellt ein solches Steuerelement für jede seiner { $count } Verwendungen, ein dort geschriebener Wert würde alle ändern: ändere ihn in { $helper } oder übergib ihn als Argument
src-component-no-prop = { $component } nimmt { $prop } nicht an: ändere den Code von { $component } oder gib seine Props an dieses Steuerelement weiter ({"{"}...props{"}"})
src-insert-vec-type = die Kinder sind ein vec![..] von { $type }, das kein { $new } aufnimmt
src-insert-tuple-full = das Kinder-Tupel hat schon { $count } Steuerelemente, mehr nimmt es nicht auf
src-insert-type-unknown = der Typ von { $prop } ist unbekannt, daher kann sein Startwert nicht geschrieben werden
src-remove-not-in-list = `{ $key }` ist kein Eintrag einer Liste von Kindern, daher kann der Designer es nicht aus dem Code entfernen
src-move-not-in-list = `{ $key }` ist kein Eintrag einer Liste von Kindern, daher kann der Designer es im Code nicht verschieben
src-move-into-itself = ein Steuerelement kann nicht in sich selbst oder in eines seiner Kinder verschoben werden
src-move-other-file = das neue Elternelement entsteht in einer anderen Datei; der Designer verschiebt ein Steuerelement nur innerhalb einer Datei
hub-read-failed = { $file } konnte nicht gelesen werden: { $error }
hub-changed-since = { $file } wurde an dieser Stelle seit dem Schreiben geändert
hub-write-failed = { $file } konnte nicht geschrieben werden: { $error }
hub-nothing-written = mit dieser Anfrage wurde nichts geschrieben
hub-not-connected = die App ist nicht verbunden
hub-no-project = die App hat nicht gesagt, aus welchem Projekt sie gebaut wurde
hub-no-tree = die App hat ihren Baum noch nicht gesendet
hub-type-unknown = der Typ von { $prop } ist unbekannt: die App hat ihn nicht unter den Eigenschaften dieses Steuerelements gesendet
hub-no-sources = keine C#-Dateien unter { $folder }
hub-changed-during = { $file } wurde während der Änderung geändert
hub-failed = der Hub ist fehlgeschlagen: { $error }
hub-no-app = keine App mit der ID { $app } ist verbunden
run-no-way = eine { $engine }-App kann noch nicht gestartet werden
run-agent-unknown = der Pfad des Agenten der App ist unbekannt: starte sie einmal mit dem Agenten, damit der Designer ihn kennt
run-already = { $project } läuft schon (App { $app }): starte sie stattdessen neu
run-not-recent = { $project } ist keine zuletzt verwendete App
run-cannot-start = { $program } konnte nicht gestartet werden: { $error }
run-build-failed = der Build ist fehlgeschlagen
run-no-connect = keine App hat sich innerhalb von { $seconds } s verbunden
run-ended-code = der Prozess endete mit Code { $code }, bevor die App sich verbunden hat
run-ended = der Prozess endete, bevor die App sich verbunden hat
connection-devices = Apps auf anderen Geräten
connection-devices-hint = Lässt eine App auf einem Telefon oder einem anderen Computer in diesem Netzwerk mit der Adresse unten verbinden. Die Adresse enthält einen geheimen Schlüssel: Wer sie hat, kann sich verbinden.
connection-devices-address = Adresse für Apps auf anderen Geräten
status-devices-failed = Apps auf anderen Geräten können sich nicht verbinden: { $reason }
hub-no-network = dieser Computer ist in keinem Netzwerk
hub-port-taken = Port { $port } ist belegt: { $error }
app-not-running = läuft nicht
status-wifi-on = WLAN an
status-wifi-off = WLAN aus
status-usb = USB: { $phones }
apps-not-running = Zuletzt verwendet
status-hot-applied = Code-Änderung in der App übernommen
status-hot-reloaded = Hot Reload: der Bildschirm wurde neu aufgebaut
gradient-summary = { $count ->
    [one] { $type } · { $count } Farbe
   *[other] { $type } · { $count } Farben
}
gradient-none = keiner
gradient-editor = Verlauf
gradient-type = Typ
gradient-bar = Farben des Verlaufs
gradient-bar-hint = Klicke auf den Balken, um eine Farbe hinzuzufügen, ziehe einen Griff, um sie zu verschieben, Entf entfernt die gewählte.
gradient-stop-value = Farbe { $index } von { $count } bei { $position } %
gradient-angle = Winkel
gradient-angle-less = 15° nach links drehen
gradient-angle-more = 15° nach rechts drehen
row-item = Element { $index }
status-live-cell = { $prop } = { $value } nur in dieser Zelle (Element { $index }). In den Code schreiben ändert alle Elemente
empty-tree-title = Noch keine Steuerelemente
empty-tree-hint = Steuerelemente erscheinen hier, sobald die App sie zeigt.
empty-tree-hint-page = Schreibe Code in Source, und seine Steuerelemente erscheinen hier.
choice-empty = (leer)
choice-empty-label = Leer, löscht den Wert

## Controls panel

panel-controls = Steuerelemente
resize-controls = Höhe von Seiten und Steuerelemente ändern
controls-search = Steuerelemente suchen
control-row-hint = Klicken, dann in der App dorthin klicken, wo es hinsoll. Oder auf Ebenen ziehen.
empty-controls-title = Keine Steuerelemente
empty-controls-hint = Verbinde eine App: Die Steuerelemente, die sie erstellen kann, erscheinen hier.
empty-controls-wait = Die App hat noch nicht gemeldet, welche Steuerelemente sie erstellen kann.
empty-controls-match-title = Kein Treffer
empty-controls-match-hint = Kein Steuerelement hat das im Namen.
group-media = Medien
group-input = Eingabe
group-shapes = Formen
group-other = Weitere
group-own = Diese App
place-title = { $type } platzieren
place-hint = In der App dorthin klicken, wo es hinsoll. Esc bricht ab.
status-inserted = { $type } in der App hinzugefügt, noch nicht im Code
status-not-inserted = { $type } wurde nicht hinzugefügt
status-app-refused = Die App hat abgelehnt: { $reason }
status-removed = { $type } aus der App entfernt
status-removed-code = Aus { $file } und aus der App entfernt
row-added = hier hinzugefügt, noch nicht im Code
row-added-written = hier hinzugefügt, im Code
help-controls = Ein Steuerelement hinzufügen
help-controls-1 = Steuerelemente, unter Seiten, listet, was deine App erstellen kann: die Steuerelemente der Bibliothek nach Art, deine eigenen Klassen unter Diese App. Tippe ins Suchfeld, um eines zu finden.
help-controls-2 = Klicke auf ein Steuerelement, dann in der App dorthin, wo es hinsoll; Esc bricht ab. Oder ziehe es auf Ebenen: zwischen zwei Zeilen oder auf ein Layout, um es hineinzulegen.
help-controls-3 = Ein neues Steuerelement ist nur in der App, mit einem Punkt in seiner Ebenen-Zeile: Schreiben bringt es in den Code, Rückgängig nimmt es wieder heraus.
selection-color = Auswahlfarbe
selection-color-hint = Der Rahmen um das ausgewählte Steuerelement in deiner App.
animate-changes = Wechsel von Design und Sprache animieren
animate-changes-hint = Ein neues Design oder eine neue Sprache öffnet sich als Kreis über dem Fenster. Aus: sofort.
color-magenta = Magenta
color-lime = Limette
color-cyan = Cyan
color-orange = Orange
color-white = Weiß
selection-color-other = Andere Farbe

## Layers menu

layers-menu = Aktionen für das Steuerelement
menu-copy = Kopieren
menu-paste = Einfügen
menu-duplicate = Duplizieren
menu-copy-style = Stil kopieren
menu-paste-style = Stil einfügen
menu-delete = Löschen
menubar = Menüleiste
menubar-file = &Datei
menubar-edit = &Bearbeiten
menubar-view = &Ansicht
menubar-help = &Hilfe
menu-run-app = App starten
menu-restart-app = App neu starten
menu-settings = Einstellungen
menu-exit = Beenden
menu-undo = Rückgängig
menu-redo = Wiederholen
menu-undo-step = Rückgängig: { $step }
menu-redo-step = Wiederholen: { $step }
menu-props-order = Reihenfolge der Eigenschaften
menu-library-parts = Teile der Bibliothek zeigen
menu-about = Über
step-edit = { $prop } ändern
step-paste-style = Stil einfügen
step-write = { $name } schreiben
step-write-all = Alle schreiben
step-add = { $type } hinzufügen
step-paste = { $type } einfügen
step-duplicate = { $type } duplizieren
step-delete = { $type } löschen
step-write-delete = { $type } aus dem Code löschen
status-undone = Rückgängig gemacht: { $step }
status-redone = Wiederholt: { $step }
write-all = Alle schreiben
write-all-label = Alle { $count } Änderungen in den Code schreiben
status-copied = { $type } kopiert mit { $count ->
    [one] { $count } Eigenschaft
   *[other] { $count } Eigenschaften
}
status-style-copied = Stil von { $type } kopiert: { $count ->
    [one] { $count } Eigenschaft
   *[other] { $count } Eigenschaften
}
status-style-pasted = { $count ->
    [one] { $count } Stileigenschaft in der App gesetzt, noch nicht im Code
   *[other] { $count } Stileigenschaften in der App gesetzt, noch nicht im Code
}
status-style-none = Nichts vom kopierten Stil passt zu { $type }
status-style-same = { $type } hat den kopierten Stil schon
status-removed-live = { $type } aus der App entfernt, noch nicht aus dem Code
status-removed-written = { $type } aus der App und aus dem Code entfernt
status-restored = { $type } ist wieder in der App
row-removed = aus der App entfernt, noch nicht aus dem Code
row-removed-written = aus der App und aus dem Code entfernt
row-removed-tag = entfernt
help-copy = Kopieren, Einfügen und Stile
help-copy-1 = Rechtsklick auf ein Steuerelement in Ebenen, oder die Menütaste oder Umschalt+F10, für Kopieren (Strg+C), Einfügen (Strg+V), Duplizieren (Strg+D), Stil kopieren (Strg+Umschalt+C), Stil einfügen (Strg+Umschalt+V) und Löschen (Entf).
help-copy-2 = Einfügen legt die Kopie in das ausgewählte Layout oder direkt hinter das ausgewählte Steuerelement. Stil einfügen setzt die kopierten Farben, Schriften, Konturen, Ecken, Schatten und Innenabstände am ausgewählten Steuerelement; Alle schreiben bringt sie in den Code.
help-copy-3 = Ein eingefügtes oder gelöschtes Steuerelement ändert nur die App, mit einem Punkt in seiner Ebenen-Zeile: Schreiben ändert auch den Code, Rückgängig nimmt es zurück.

## Hub: writes while other live changes wait

hub-not-in-code-yet = { $type } ist noch nicht im Code: zuerst in den Code schreiben
hub-parent-not-in-code = das übergeordnete { $type } ist noch nicht im Code: zuerst dieses schreiben
hub-in-code-already = { $type } ist schon im Code

## Copy with what is inside

status-copied-inside = { $type } kopiert mit { $count ->
    [one] { $count } Steuerelement darin
   *[other] { $count } Steuerelementen darin
}
status-copy-gone = Nicht kopiert: die App zeigt dieses Steuerelement nicht mehr

## Live changes kept across a rebuild

hub-lost-prop = { $type } { $prop }: das Steuerelement ist nicht mehr im Code
hub-lost-insert = hier hinzugefügtes { $type }: seine Stelle ist nicht mehr im Code
hub-lost-remove = hier entferntes { $type }: es ist nicht mehr im Code
status-not-kept = { $count ->
    [one] 1 Änderung konnte nicht erhalten werden: { $what }
   *[other] { $count } Änderungen konnten nicht erhalten werden: { $what }
}
status-moved = { $type } in der App verschoben, noch nicht im Code
status-not-moved = { $type } wurde nicht verschoben
step-move = { $type } verschieben
row-moved = hier verschoben, noch nicht im Code
row-moved-written = hier verschoben, im Code
hub-lost-move = hier verschobenes { $type }: seine Stelle ist nicht mehr im Code
help-move = Steuerelemente verschieben
help-move-1 = Ziehe die Zeile eines Steuerelements in Ebenen: Die Zeilen öffnen eine Lücke, wo es landet. Zwischen zwei Zeilen kommt es dorthin, auf ein Layout hinein, als letztes. Am oberen oder unteren Rand läuft die Liste mit; Esc bricht ab.
help-move-2 = Alt+Pfeil hoch und Alt+Pfeil runter verschieben das ausgewählte Steuerelement um eine Stelle unter seinen Geschwistern.
help-move-3 = Ein verschobenes Steuerelement ändert nur die App, mit einem Punkt in seiner Ebenen-Zeile: Schreiben verschiebt es auch im Code, Rückgängig setzt es zurück.
pick-label-keys = Ein Steuerelement in der App auswählen (Strg+P)
section-code = Code
code-hint = Wie die Änderungen von hier in den Code der App gelangen.
write-mode = Änderungen in den Code schreiben
write-mode-hint = Manuell: Sie schreiben sie selbst, mit Schreiben einer Zeile oder Alle schreiben. Automatisch: Sie werden geschrieben, wenn der Code gebraucht wird, vor Neu starten und Ausführen. Immer: Jede fertige Änderung wird sofort geschrieben.
write-mode-manual = Manuell
write-mode-auto = Automatisch
write-mode-always = Immer
changes-not-in-code = { $count ->
    [one] { $count } Änderung nicht im Code
   *[other] { $count } Änderungen nicht im Code
}
write-changes-label = Alle { $count } Änderungen in den Code schreiben
step-write-changes = Alle Änderungen schreiben
unwritten-title = { $count ->
    [one] { $count } Änderung ist noch nicht im Code.
   *[other] { $count } Änderungen sind noch nicht im Code.
}
unwritten-write-restart = Schreiben, dann neu starten
unwritten-skip-restart = Ohne Schreiben neu starten
unwritten-write-run = Schreiben, dann ausführen
unwritten-skip-run = Ohne Schreiben ausführen
unwritten-write-close = Schreiben, dann schließen
unwritten-skip-close = Ohne Schreiben schließen
unwritten-failed-title = { $count ->
    [one] { $count } Änderung konnte nicht in den Code geschrieben werden:
   *[other] { $count } Änderungen konnten nicht in den Code geschrieben werden:
}
unwritten-more-reasons = und { $count } weitere
unwritten-cancel = Abbrechen
