# DrawnUI Designer, srpski (latinica). Izvor: en.ftl (isti ključevi).

## Window

app-title = DrawnUI Designer
apps = Aplikacije
help = Pomoć
settings = Podešavanja
back-to-designer = { $page }, nazad u dizajner

## Panels

panel-screens = Ekrani
panel-layers = Slojevi
panel-properties = Svojstva
resize-screens = Promeni širinu Ekrana
resize-properties = Promeni širinu Svojstava
screen-open = Otvori { $title }
controls-count = { $count ->
    [one] { $count } kontrola
    [few] { $count } kontrole
   *[other] { $count } kontrola
}
empty-screens-title = Nema ekrana
empty-screens-hint = Ovde se pojavljuju rute školjke aplikacije.
empty-app-title = Nijedna aplikacija nije povezana
empty-app-hint = Pokreni DrawnUI aplikaciju sa agentom dizajnera ili pokreni poslednju.
empty-selection-title = Ništa nije izabrano
empty-selection-hint = Izaberi kontrolu u Slojevima da vidiš i menjaš njena svojstva.

## Title bar apps

app-building = gradi se…
app-starting = pokreće se…
app-failed = neuspeh
app-restart = Ponovo pokreni { $name }
app-run = Pokreni { $name }
app-starting-name = Pokreće se { $name }…
app-shown = { $name }, { $detail }, prikazana
app-show = Prikaži { $name }, { $detail }
app-run-detail = Pokreni { $name }, { $detail }

## Layers

pick = Izaberi
pick-active = Klikni ili dodirni u aplikaciji
pick-label = Izaberi kontrolu u aplikaciji
fold = Skupi { $name }
unfold = Raširi { $name }
row-level = nivo { $level }
row-folded = skupljeno
row-open = rašireno
row-selected = izabrano

## Properties

order = Redosled svojstava
order-set = Zadata
order-groups = Tip
group-changed = Zadata
group-others = Nezadata
group-layout = Raspored
group-text = Tekst
group-look = Izgled
group-transform = Transformacija
group-behavior = Ponašanje
group-row = { $group }, { $count ->
    [one] { $count } svojstvo
    [few] { $count } svojstva
   *[other] { $count } svojstava
}, { $state }
group-open = rašireno
group-folded = skupljeno
prop-changed = promenjeno u aplikaciji
prop-set = zadato
value = Vrednost
decrease = Smanji
increase = Povećaj
undo-prop = Poništi { $name }
write-prop = Upiši { $name } u kod

## Status line

status-connected = { $count ->
    [one] Hab povezan · { $count } aplikacija
    [few] Hab povezan · { $count } aplikacije
   *[other] Hab povezan · { $count } aplikacija
}
status-connecting = Povezivanje sa habom…
status-run-failed = { $name }: neuspeh: { $message }
status-live = { $prop } = { $value } u aplikaciji, još nije u kodu
status-writing = { $prop } se upisuje u kod…
status-cancelled = { $prop }: izmena odbačena
status-written = Upisano u { $file }, red { $line }
status-not-written = Nije upisano u kod: { $reason }
status-not-undone = Nije poništeno u kodu: { $reason }
status-undoing = Poništava se { $prop } u kodu…
status-undone-code = { $prop } ponovo { $value } u { $file }
status-undone-app = { $prop } ponovo { $value } u aplikaciji

## Settings

settings-sections = Odeljci podešavanja
section-appearance = Izgled
section-layers = Slojevi
section-connection = Veza
section-about = O programu
appearance-hint = Kako dizajner izgleda.
theme = Tema
theme-hint = Boje celog prozora. Nova tema se otvara odakle klikneš.
theme-dark = Tamna
theme-midnight = Ponoć
theme-graphite = Grafit
theme-light = Svetla
theme-paper = Papir
theme-high-contrast = Visok kontrast
language = Jezik
language-hint = Jezik celog prozora.
language-system = Sistemski ({ $language })
layers-hint = Stablo kontrola ekrana koji aplikacija prikazuje.
library-parts = Delovi biblioteke
library-parts-hint = Prikaži i kontrole koje DrawnUI sam pravi, na primer unutrašnji natpis dugmeta. Njih ne pravi tvoj kod, pa se njihove vrednosti ne mogu upisati u kod.
connection-hint = Gde aplikacije i AI agenti pronalaze dizajner. Pokrenute aplikacije se povezuju same.
connection-apps = Aplikacije
connection-mcp = MCP server za AI agente
connection-claude = Dodaj u Claude Code
copy = Kopiraj
copied = Kopirano

## About

about-logo = DrawnUI logo
about-version = Verzija { $version }
about-description = Vidi ekrane, kontrole i svojstva pokrenute DrawnUI aplikacije, menjaj ih uživo i upiši svaku izmenu nazad u njen kod.
made-with = Napravljeno sa
uses = Koristi
credit-drawnui-rust = Mašina za crtani interfejs na Skia: jedan kod za računar, telefone i veb.
credit-nx = Njegov jezički protokol: kod interfejsa čitan kao stablo čvorova i menjan čvor po čvor.
credit-tree-sitter = Čita i menja C# kod aplikacije, a svaki bajt koji se ne menja ostavlja kakav je bio.
credit-mcp = Model Context Protocol server preko kog AI agenti vide i menjaju pokrenutu aplikaciju.
credit-lucide = Ikonice.

## Help

help-title = Korišćenje dizajnera
help-intro = Vidi ekrane, kontrole i svojstva pokrenute DrawnUI aplikacije, menjaj ih uživo i upiši svaku izmenu u C# kod aplikacije.
help-connect = Poveži aplikaciju
help-connect-1 = Pokreni svoju aplikaciju sa agentom dizajnera: povezuje se sama i pojavljuje se u naslovnoj traci.
help-connect-2 = Aplikacija koja je već bila pokrenuta takođe ostaje tamo: klikni je da je ponovo pokreneš. Okrugla strelica pokrenute aplikacije je ponovo pokreće.
help-connect-3 = Kad je povezano više aplikacija, klikni na jednu da je prikažeš ovde; na računaru njen prozor dolazi napred.
help-phones = Telefoni
help-phones-1 = iPhone ili iPad na USB kablu se povezuje sam: pokreni na njemu Debug verziju aplikacije sa agentom dizajnera. Wi-Fi nije potreban.
help-phones-2 = Izaberi, pa dodirni kontrolu na telefonu da je izabereš ovde. Ponovo pritisni Izaberi da prekineš.
help-phones-3 = Bez kabla: Podešavanja, Veza, Aplikacije na drugim uređajima daju adresu sa kojom se pravi aplikacija.
help-screens = Ekrani
help-screens-1 = Stranice koje školjka aplikacije može da otvori. Klikni jednu da je otvoriš u aplikaciji.
help-layers = Slojevi
help-layers-1 = Kontrole ekrana koji aplikacija prikazuje, kao stablo. Klikni jednu da je izabereš: u aplikaciji se oko nje pojavi okvir.
help-layers-2 = Dvaput klikni red, ili klikni njegov kvadratić na liniji stabla, da ga skupiš ili raširiš.
help-layers-3 = Izaberi, pa klikni na kontrolu u aplikaciji ili je dodirni na telefonu da je izabereš ovde.
help-properties = Svojstva
help-properties-1 = Klikni vrednost da je promeniš: ukucaj je, koristi - i + za brojeve, izaberi sa liste ili izaberi boju.
help-properties-2 = Enter je postavlja u pokrenutoj aplikaciji. Esc je vraća kakva je bila.
help-properties-3 = Promenjena svojstva su svetlija. Zadata ih prikazuje prva (Zadata, pa Nezadata), Tip po vrsti.
help-write = Upis u kod
help-write-1 = Dugme za kod u redu upisuje vrednost u C# fajl koji pravi kontrolu: menja se samo ta vrednost, ostatak fajla ostaje kakav je bio.
help-write-2 = Poništi vraća poslednju izmenu reda: u aplikaciji, a posle upisa i u fajlu.
help-write-3 = Red na dnu kaže koji fajl je promenjen ili zašto ništa nije upisano.
help-write-4 = Ctrl+Z opoziva tvoju poslednju radnju, kakva god da je: izmenu, upis, lepljenje, brisanje. Ctrl+Y je ponavlja. Meni Uređivanje imenuje obe.
help-keyboard = Tastatura
help-keyboard-1 = Tab prelazi između grupa, strelice se kreću unutar liste ili menija, Enter ili razmak izvršava.
help-keyboard-2 = Esc zatvara meni, izlazi iz izmene i vraća iz Podešavanja ili Pomoći.
help-keyboard-3 = Traka menija: klikni naslov ili pritisni Alt sa njegovim podvučenim slovom. Stavke pokazuju svoje tastere, koji rade i kad su meniji zatvoreni.
help-agents = AI agenti
help-agents-1 = Agenti koriste iste mogućnosti preko MCP na { $address }.
help-agents-2 = Podešavanja, Veza sadrži adresu i komandu koja je dodaje u Claude Code.

## Color picker

picker = Boja
picker-plane = Zasićenost i osvetljenost
picker-plane-value = zasićenost { $saturation } %, osvetljenost { $brightness } %
picker-hue = Nijansa
picker-opacity = Neprozirnost
picker-hex = Heks boja
picker-old-new = Pre: { $old }, nova desno
picker-keys = Strelice pomeraju izabranu traku ili kvadrat (Shift: deset koraka). Esc vraća staru boju.

## Hub

src-not-number = `{ $text }` nije broj
src-not-bool = `{ $text }` nije ni true ni false
src-not-color = `{ $text }` nije boja #RGB, #RRGGBB ili #AARRGGBB
src-not-thickness = `{ $text }` nije razmak ("sve", "h,v" ili "l,g,d,d")
src-type-unsupported = vrednosti tipa { $type } još ne mogu da se upišu u kod
src-not-in-tree = kontrola nije u poslednjem stablu aplikacije
src-library-part = ovu kontrolu je biblioteka napravila za svoje delove: nijedan kod je ne deklariše
src-no-code = nije pronađen kod koji pravi ovu kontrolu
src-arg-no-value = { $prop } dolazi iz argumenta `{ $param }` poziva `{ $call }`, koji ovde nema vrednost
src-several-args = { $prop } dobija vrednost iz više argumenata poziva `{ $call }`
src-key-missing = `{ $key }` nije u kodu
src-no-value = svojstvo nema vrednost
src-not-literal = `{ $key }` nije literal koji dizajner može da promeni (izraz ili poziv)
src-value-misfit = vrednost ne odgovara `{ $key }` (druga vrsta literala)
src-not-single = izmena nije jedna jedina promena
src-empty-parens = prazne zagrade
src-rewrites-colors = { $prop } se postavlja kroz `{ $code }`; dizajner prepisuje samo Color.Parse("#..") i Colors.X
src-rewrites-thickness-numbers = { $prop } se postavlja kroz `{ $code }`; dizajner prepisuje samo argumente Thickness koji su brojevi
src-rewrites-new-thickness = { $prop } se postavlja kroz `{ $code }`; dizajner prepisuje samo new Thickness(..) sa brojčanim argumentima
src-no-arguments = nema argumenata
src-with-not-numbers = With{ $prop }(..) ima argumente koji nisu brojevi
src-with-misfit = With{ $prop }(..) ne prima ovu vrednost
src-insert-no-list = kod koji pravi decu ove kontrole nije lista u koju dizajner može da doda (lista `Children`, `new List<SkiaControl>` ili `.WithChildren(..)`)
src-insert-no-initializer = `{ $key }` se pravi bez inicijalizatora objekta, pa dizajner ne može da mu doda decu
src-insert-templated = ova kontrola pravi decu po šablonu: dodaj stavku u njen ItemsSource
src-insert-unmapped = deca ove kontrole ne odgovaraju njenom kodu jedan na jedan (petlja ili pomoćna funkcija pravi više njih), pa dizajner ne zna gde ide nova kontrola
src-insert-vec-type = deca su vec![..] od { $type }, u koji ne ide { $new }
src-insert-tuple-full = torka dece već ima { $count } kontrola, najviše što prima
src-insert-type-unknown = tip od { $prop } nije poznat, pa njegova početna vrednost ne može da se upiše
src-remove-not-in-list = `{ $key }` nije stavka liste dece, pa dizajner ne može da ga ukloni iz koda
src-move-not-in-list = `{ $key }` nije stavka liste dece, pa dizajner ne može da ga premesti u kodu
src-move-into-itself = kontrola ne može da se premesti u samu sebe ni u neko svoje dete
src-move-other-file = novi roditelj se pravi u drugoj datoteci; dizajner premešta kontrolu samo unutar jedne datoteke
hub-read-failed = nije moguće pročitati { $file }: { $error }
hub-changed-since = { $file } je promenjen na tom mestu od upisa
hub-write-failed = nije moguće upisati { $file }: { $error }
hub-nothing-written = tim zahtevom ništa nije upisano
hub-not-connected = aplikacija nije povezana
hub-no-project = aplikacija nije rekla iz kog projekta je napravljena
hub-no-tree = aplikacija još nije poslala svoje stablo
hub-type-unknown = tip svojstva { $prop } je nepoznat: aplikacija ga nije poslala među svojstvima ove kontrole
hub-no-sources = nema C# fajlova u { $folder }
hub-changed-during = { $file } je promenjen tokom izmene
hub-failed = greška haba: { $error }
hub-no-app = nijedna aplikacija sa id { $app } nije povezana
run-no-way = { $engine } aplikacija još ne može da se pokrene
run-agent-unknown = putanja agenta aplikacije nije poznata: pokreni je jednom sa agentom da je dizajner sazna
run-already = { $project } je već pokrenuta (aplikacija { $app }): bolje je ponovo pokreni
run-not-recent = { $project } nije među skorašnjim aplikacijama
run-cannot-start = nije moguće pokrenuti { $program }: { $error }
run-build-failed = gradnja nije uspela
run-no-connect = nijedna aplikacija se nije povezala za { $seconds } s
run-ended-code = proces se završio sa kodom { $code } pre nego što se aplikacija povezala
run-ended = proces se završio pre nego što se aplikacija povezala
connection-devices = Aplikacije na drugim uređajima
connection-devices-hint = Dozvoljava aplikaciji na telefonu ili drugom računaru u ovoj mreži da se poveže preko adrese ispod. Adresa sadrži tajni ključ: svako ko je ima može da se poveže.
connection-devices-address = Adresa za aplikacije na drugim uređajima
status-devices-failed = Aplikacije na drugim uređajima ne mogu da se povežu: { $reason }
hub-no-network = ovaj računar nije ni u jednoj mreži
hub-port-taken = port { $port } je zauzet: { $error }
app-not-running = nije pokrenuta
status-wifi-on = Wi-Fi uključen
status-wifi-off = Wi-Fi isključen
status-usb = USB: { $phones }
apps-not-running = Nedavne
status-hot-applied = Izmena koda primenjena u aplikaciji
status-hot-reloaded = Hot reload: ekran je ponovo izgrađen
gradient-summary = { $count ->
    [one] { $type } · { $count } boja
    [few] { $type } · { $count } boje
   *[other] { $type } · { $count } boja
}
gradient-none = nema
gradient-editor = Prelaz
gradient-type = Tip
gradient-bar = Boje prelaza
gradient-bar-hint = Klikni na traku da dodaš boju, prevuci ručicu da je pomeriš, Delete uklanja izabranu.
gradient-stop-value = Boja { $index } od { $count } na { $position } %
gradient-angle = Ugao
gradient-angle-less = Okreni 15° ulevo
gradient-angle-more = Okreni 15° udesno
row-item = stavka { $index }
status-live-cell = { $prop } = { $value } samo u ovoj ćeliji (stavka { $index }). Upis u kod menja sve stavke
empty-tree-title = Još nema kontrola
empty-tree-hint = Kontrole se pojavljuju ovde čim ih aplikacija prikaže.
empty-tree-hint-page = Napiši kod u Source i njegove kontrole se pojavljuju ovde.
choice-empty = (prazno)
choice-empty-label = Prazno, briše vrednost

## Controls panel

panel-controls = Kontrole
resize-controls = Promeni visinu Ekrana i Kontrola
controls-search = Pretraži kontrole
control-row-hint = Klikni, pa klikni u aplikaciji gde treba da stoji. Ili je prevuci na Slojeve.
empty-controls-title = Nema kontrola
empty-controls-hint = Poveži aplikaciju: kontrole koje može da napravi pojavljuju se ovde.
empty-controls-wait = Aplikacija još nije javila koje kontrole može da napravi.
empty-controls-match-title = Nema rezultata
empty-controls-match-hint = Nijedna kontrola nema ovo u imenu.
group-media = Mediji
group-input = Unos
group-shapes = Oblici
group-other = Ostalo
group-own = Ova aplikacija
place-title = Postavljanje: { $type }
place-hint = Klikni u aplikaciji gde treba da stoji. Esc otkazuje.
status-inserted = { $type } dodata u aplikaciju, još nije u kodu
status-not-inserted = { $type } nije dodata
status-app-refused = Aplikacija je odbila: { $reason }
status-removed = { $type } uklonjena iz aplikacije
status-removed-code = Uklonjeno iz { $file } i iz aplikacije
row-added = dodato ovde, još nije u kodu
row-added-written = dodato ovde, u kodu
help-controls = Dodaj kontrolu
help-controls-1 = Kontrole, ispod Ekrana, navode šta tvoja aplikacija može da napravi: kontrole biblioteke po vrsti, tvoje sopstvene klase pod Ova aplikacija. Kucaj u polje za pretragu da nađeš neku.
help-controls-2 = Klikni na kontrolu, pa u aplikaciji gde treba da stoji; Esc otkazuje. Ili je prevuci na Slojeve: između dva reda, ili na raspored da je staviš unutra.
help-controls-3 = Nova kontrola postoji samo u aplikaciji, sa tačkom u svom redu u Slojevima: Upiši je stavlja u kod, Poništi je uklanja.
selection-color = Boja izbora
selection-color-hint = Okvir oko izabrane kontrole u tvojoj aplikaciji.
animate-changes = Animiraj promenu teme i jezika
animate-changes-hint = Nova tema ili jezik otvara se preko prozora u krugu. Isključeno: primenjuje se odmah.
color-magenta = Magenta
color-lime = Limeta
color-cyan = Cijan
color-orange = Narandžasta
color-white = Bela
selection-color-other = Druga boja

## Layers menu

layers-menu = Radnje nad kontrolom
menu-copy = Kopiraj
menu-paste = Nalepi
menu-duplicate = Dupliraj
menu-copy-style = Kopiraj stil
menu-paste-style = Nalepi stil
menu-delete = Obriši
menubar = Traka menija
menubar-file = &Datoteka
menubar-edit = &Uređivanje
menubar-view = &Prikaz
menubar-help = Po&moć
menu-run-app = Pokreni aplikaciju
menu-restart-app = Ponovo pokreni aplikaciju
menu-settings = Podešavanja
menu-exit = Izlaz
menu-undo = Opozovi
menu-redo = Ponovi
menu-undo-step = Opozovi: { $step }
menu-redo-step = Ponovi: { $step }
menu-props-order = Redosled svojstava
menu-library-parts = Prikaži delove biblioteke
menu-about = O programu
step-edit = Izmeni { $prop }
step-paste-style = Nalepi stil
step-write = Upiši { $name }
step-write-all = Upiši sve
step-add = Dodaj { $type }
step-paste = Nalepi { $type }
step-duplicate = Dupliraj { $type }
step-delete = Obriši { $type }
step-write-delete = Obriši { $type } iz koda
status-undone = Opozvano: { $step }
status-redone = Ponovljeno: { $step }
write-all = Upiši sve
write-all-label = Upiši sve izmene u kod: { $count }
status-copied = { $type } kopirana, { $count ->
    [one] { $count } svojstvo
    [few] { $count } svojstva
   *[other] { $count } svojstava
}
status-style-copied = Stil kontrole { $type } kopiran, { $count ->
    [one] { $count } svojstvo
    [few] { $count } svojstva
   *[other] { $count } svojstava
}
status-style-pasted = Svojstva stila u aplikaciji, još nisu u kodu: { $count }
status-style-none = Ništa od kopiranog stila ne odgovara kontroli { $type }
status-removed-live = { $type } uklonjena iz aplikacije, još ne iz koda
status-removed-written = { $type } uklonjena iz aplikacije i iz koda
status-restored = { $type } je ponovo u aplikaciji
row-removed = uklonjena iz aplikacije, još ne iz koda
row-removed-written = uklonjena iz aplikacije i iz koda
row-removed-tag = uklonjena
help-copy = Kopiranje, lepljenje i stilovi
help-copy-1 = Desni klik na kontrolu u Slojevima, ili taster Meni ili Shift+F10, za Kopiraj (Ctrl+C), Nalepi (Ctrl+V), Dupliraj (Ctrl+D), Kopiraj stil (Ctrl+Shift+C), Nalepi stil (Ctrl+Shift+V) i Obriši (Del).
help-copy-2 = Nalepi stavlja kopiju u izabrani raspored, ili odmah posle izabrane kontrole. Nalepi stil postavlja kopirane boje, fontove, ivice, uglove, senke i unutrašnje razmake na izabranu kontrolu; Upiši sve ih stavlja u kod.
help-copy-3 = Nalepljena ili obrisana kontrola menja samo aplikaciju, sa tačkom u svom redu u Slojevima: Upiši menja i kod, Poništi je vraća.

## Hub: writes while other live changes wait

hub-not-in-code-yet = { $type } još nije u kodu: prvo je upiši u kod
hub-parent-not-in-code = njen roditelj { $type } još nije u kodu: prvo upiši njega
hub-in-code-already = { $type } je već u kodu

## Copy with what is inside

status-copied-inside = { $type } kopirana, unutra { $count ->
    [one] { $count } kontrola
    [few] { $count } kontrole
   *[other] { $count } kontrola
}
status-copy-gone = Nije kopirano: aplikacija više ne prikazuje tu kontrolu

## Live changes kept across a rebuild

hub-lost-prop = { $type } { $prop }: te kontrole više nema u kodu
hub-lost-insert = { $type } dodata ovde: njenog mesta više nema u kodu
hub-lost-remove = { $type } uklonjena ovde: više je nema u kodu
status-not-kept = Izmene koje nisu mogle da se zadrže: { $count }. { $what }
status-moved = { $type } premeštena u aplikaciji, još nije u kodu
status-not-moved = { $type } nije premeštena
step-move = Premesti { $type }
row-moved = premešteno ovde, još nije u kodu
row-moved-written = premešteno ovde, u kodu
hub-lost-move = { $type } premeštena ovde: njeno mesto više nije u kodu
help-move = Premeštanje kontrola
help-move-1 = Prevuci red kontrole u Slojevima: redovi otvaraju razmak gde će pasti. Između dva reda ide tamo; na raspored ide unutra, poslednja. Blizu gornje ili donje ivice lista se pomera; Esc otkazuje.
help-move-2 = Alt+Gore i Alt+Dole pomeraju izabranu kontrolu za jedno mesto među susednim.
help-move-3 = Premeštena kontrola menja samo aplikaciju, sa tačkom u svom redu u Slojevima: Upiši je premešta i u kodu, Poništi je vraća.
pick-label-keys = Izaberi kontrolu u aplikaciji (Ctrl+P)
section-code = Kod
code-hint = Kako izmene napravljene ovde stižu u kod aplikacije.
write-mode = Upisuj izmene u kod
write-mode-hint = Ručno: vi ih upisujete, dugmetom Upiši u redu ili Upiši sve. Automatski: upisuju se kada je kod potreban, pre ponovnog pokretanja i pokretanja. Uvek: svaka završena izmena se upisuje odmah.
write-mode-manual = Ručno
write-mode-auto = Automatski
write-mode-always = Uvek
changes-not-in-code = { $count ->
    [one] { $count } izmena nije u kodu
    [few] { $count } izmene nisu u kodu
   *[other] { $count } izmena nije u kodu
}
write-changes-label = Upiši sve izmene u kod ({ $count })
step-write-changes = Upiši sve izmene
unwritten-title = { $count ->
    [one] { $count } izmena još nije u kodu.
    [few] { $count } izmene još nisu u kodu.
   *[other] { $count } izmena još nije u kodu.
}
unwritten-write-restart = Upiši ih, pa pokreni ponovo
unwritten-skip-restart = Pokreni ponovo bez upisa
unwritten-write-run = Upiši ih, pa pokreni
unwritten-skip-run = Pokreni bez upisa
unwritten-write-close = Upiši ih, pa zatvori
unwritten-skip-close = Zatvori bez upisa
unwritten-cancel = Otkaži
