# DrawnUI Designer, français. Source : en.ftl (mêmes clés).

## Window

app-title = DrawnUI Designer
apps = Apps
help = Aide
settings = Réglages
back-to-designer = { $page }, retour au designer

## Panels

panel-screens = Écrans
panel-layers = Calques
panel-properties = Propriétés
resize-screens = Changer la largeur d’Écrans
resize-properties = Changer la largeur de Propriétés
screen-open = Ouvrir { $title }
controls-count = { $count ->
    [one] { $count } contrôle
   *[other] { $count } contrôles
}
empty-screens-title = Aucun écran
empty-screens-hint = Les routes du shell de l’app s’affichent ici.
empty-app-title = Aucune app connectée
empty-app-hint = Lancez une app DrawnUI avec l’agent du designer, ou relancez la dernière.
empty-selection-title = Rien de sélectionné
empty-selection-hint = Sélectionnez un contrôle dans Calques pour voir et modifier ses propriétés.

## Title bar apps

app-building = compilation…
app-starting = démarrage…
app-failed = échec
app-restart = Redémarrer { $name }
app-run = Lancer { $name }
app-starting-name = Démarrage de { $name }…
app-shown = { $name }, { $detail }, affichée
app-show = Afficher { $name }, { $detail }
app-run-detail = Lancer { $name }, { $detail }

## Layers

pick = Choisir
pick-active = Cliquez ou touchez dans l’app
pick-label = Choisir un contrôle dans l’app
fold = Replier { $name }
unfold = Déplier { $name }
row-level = niveau { $level }
row-folded = replié
row-open = déplié
row-selected = sélectionné

## Properties

order = Ordre des propriétés
order-set = Définies
order-groups = Type
group-changed = Définies
group-others = Non définies
group-layout = Disposition
group-text = Texte
group-look = Apparence
group-transform = Transformation
group-behavior = Comportement
group-row = { $group }, { $count ->
    [one] { $count } propriété
   *[other] { $count } propriétés
}, { $state }
group-open = déplié
group-folded = replié
prop-changed = modifiée dans l’app
prop-set = définie
value = Valeur
decrease = Diminuer
increase = Augmenter
undo-prop = Annuler { $name }
write-prop = Écrire { $name } dans le code

## Status line

status-connected = { $count ->
    [one] Hub connecté · { $count } app
   *[other] Hub connecté · { $count } apps
}
status-connecting = Connexion au hub…
status-run-failed = { $name } a échoué : { $message }
status-live = { $prop } = { $value } dans l’app, pas encore dans le code
status-writing = Écriture de { $prop } dans le code…
status-cancelled = { $prop } : modification annulée
status-written = Écrit dans { $file }, ligne { $line }
status-not-written = Pas écrit dans le code : { $reason }
status-not-undone = Pas annulé dans le code : { $reason }
status-undoing = Annulation de { $prop } dans le code…
status-undone-code = { $prop } revient à { $value } dans { $file }
status-undone-app = { $prop } revient à { $value } dans l’app

## Settings

settings-sections = Sections des réglages
section-appearance = Apparence
section-layers = Calques
section-connection = Connexion
section-about = À propos
appearance-hint = L’apparence du designer.
theme = Thème
theme-hint = Les couleurs de toute la fenêtre. Le nouveau s’ouvre depuis l’endroit où vous cliquez.
theme-dark = Sombre
theme-midnight = Minuit
theme-graphite = Graphite
theme-light = Clair
theme-paper = Papier
theme-high-contrast = Contraste élevé
language = Langue
language-hint = La langue de toute la fenêtre.
language-system = Système ({ $language })
layers-hint = L’arbre des contrôles de l’écran que l’app affiche.
library-parts = Éléments de la bibliothèque
library-parts-hint = Afficher aussi les contrôles que DrawnUI crée lui-même, comme le libellé interne d’un bouton. Aucun code à vous ne les crée, leurs valeurs ne peuvent donc pas être écrites dans le code.
connection-hint = Où les apps et les agents IA joignent le designer. Les apps lancées se connectent d’elles-mêmes.
connection-apps = Apps
connection-mcp = Serveur MCP pour les agents IA
connection-claude = L’ajouter à Claude Code
copy = Copier
copied = Copié

## About

about-logo = Logo DrawnUI
about-version = Version { $version }
about-description = Voyez les écrans, les contrôles et les propriétés d’une app DrawnUI lancée, modifiez-les en direct et écrivez chaque modification dans son code.
made-with = Fait avec
uses = Utilise
credit-drawnui-rust = Le moteur d’interface dessinée sur Skia : un seul code pour le bureau, les téléphones et le web.
credit-nx = Son protocole de langage : le code de l’interface lu comme un arbre de nœuds et modifié nœud par nœud.
credit-tree-sitter = Lit et modifie le code C# de l’app en laissant tel quel chaque octet qui ne change pas.
credit-mcp = Le serveur Model Context Protocol qui permet aux agents IA de voir et de modifier une app lancée.
credit-lucide = Les icônes.

## Help

help-title = Utiliser le designer
help-intro = Voyez les écrans, les contrôles et les propriétés d’une app DrawnUI lancée, modifiez-les en direct et écrivez chaque modification dans le code C# de l’app.
help-connect = Connecter une app
help-connect-1 = Lancez votre app avec l’agent du designer : elle se connecte d’elle-même et apparaît dans la barre de titre.
help-connect-2 = Une app déjà lancée y reste aussi : cliquez dessus pour la relancer. La flèche ronde d’une app en cours la redémarre.
help-connect-3 = Avec plusieurs apps connectées, cliquez sur l’une pour l’afficher ici ; sur un ordinateur, sa fenêtre passe au premier plan.
help-phones = Téléphones
help-phones-1 = Un iPhone ou un iPad branché en USB se connecte tout seul : lancez-y une version Debug de votre app avec l’agent du designer. Pas besoin de Wi-Fi.
help-phones-2 = Choisir, puis touchez un contrôle sur le téléphone pour le sélectionner ici. Appuyez de nouveau sur Choisir pour arrêter.
help-phones-3 = Sans câble : Réglages, Connexion, Apps sur d’autres appareils donne l’adresse avec laquelle compiler l’app.
help-screens = Écrans
help-screens-1 = Les pages que le shell de l’app peut ouvrir. Cliquez sur l’une pour l’ouvrir dans l’app.
help-layers = Calques
help-layers-1 = Les contrôles de l’écran que l’app affiche, en arbre. Cliquez sur l’un pour le sélectionner : un cadre l’entoure dans l’app.
help-layers-2 = Double-cliquez sur une ligne, ou cliquez sur sa case sur la ligne de l’arbre, pour la replier ou la déplier.
help-layers-3 = Choisir, puis cliquez sur un contrôle dans l’app, ou touchez-le sur un téléphone, pour le sélectionner ici.
help-properties = Propriétés
help-properties-1 = Cliquez sur une valeur pour la modifier : tapez-la, utilisez - et + pour les nombres, choisissez dans une liste ou choisissez une couleur.
help-properties-2 = Entrée l’applique dans l’app lancée. Échap la remet comme avant.
help-properties-3 = Les propriétés modifiées sont plus claires. Définies les montre en premier (Définies, puis Non définies), Type par sorte.
help-write = Écrire dans le code
help-write-1 = Le bouton de code d’une ligne écrit la valeur dans le fichier C# qui crée le contrôle : seule cette valeur change, le reste du fichier reste tel quel.
help-write-2 = Annuler reprend la dernière modification d’une ligne : dans l’app, et dans le fichier après une écriture.
help-write-3 = La ligne du bas indique quel fichier a changé, ou pourquoi rien n’a été écrit.
help-write-4 = Ctrl+Z annule votre dernière action, quelle qu’elle soit : une modification, une écriture, un collage, une suppression. Ctrl+Y la rétablit. Le menu Édition nomme les deux.
help-keyboard = Clavier
help-keyboard-1 = Tab passe d’un groupe à l’autre, les flèches se déplacent dans une liste ou un menu, Entrée ou Espace agit.
help-keyboard-2 = Échap ferme un menu, quitte une modification et revient depuis Réglages ou Aide.
help-keyboard-3 = La barre de menus : cliquez sur un titre ou appuyez sur Alt avec sa lettre soulignée. Ses éléments montrent leurs touches, qui marchent aussi menus fermés.
help-agents = Agents IA
help-agents-1 = Les agents utilisent les mêmes fonctions par MCP à { $address }.
help-agents-2 = Réglages, Connexion donne l’adresse et la commande qui l’ajoute à Claude Code.

## Color picker

picker = Couleur
picker-plane = Saturation et luminosité
picker-plane-value = saturation { $saturation } %, luminosité { $brightness } %
picker-hue = Teinte
picker-opacity = Opacité
picker-hex = Couleur hexadécimale
picker-old-new = Avant : { $old }, la nouvelle à droite
picker-keys = Les flèches déplacent la barre ou le carré choisi (Maj : dix pas). Échap remet l’ancienne couleur.

## Hub

src-not-number = `{ $text }` n’est pas un nombre
src-not-bool = `{ $text }` n’est ni true ni false
src-not-color = `{ $text }` n’est pas une couleur #RGB, #RRGGBB ou #AARRGGBB
src-not-thickness = `{ $text }` n’est pas une épaisseur ("tout", "h,v" ou "g,h,d,b")
src-type-unsupported = les valeurs { $type } ne peuvent pas encore être écrites dans le code
src-not-in-tree = le contrôle n’est pas dans le dernier arbre de l’app
src-library-part = la bibliothèque a créé ce contrôle pour ses propres éléments : aucun code ne le déclare
src-no-code = aucun code trouvé qui crée ce contrôle
src-arg-no-value = { $prop } vient de l’argument `{ $param }` de `{ $call }`, qui n’a pas de valeur ici
src-several-args = { $prop } est alimentée par plusieurs arguments de `{ $call }`
src-key-missing = `{ $key }` n’est pas dans le code
src-no-value = la propriété n’a pas de valeur
src-not-literal = `{ $key }` n’est pas un littéral que le designer peut modifier (une expression ou un appel)
src-value-misfit = la valeur ne convient pas à `{ $key }` (un autre type de littéral)
src-not-single = la modification n’est pas une seule modification
src-empty-parens = parenthèses vides
src-rewrites-colors = { $prop } est définie par `{ $code }` ; le designer ne réécrit que Color.Parse("#..") et Colors.X
src-rewrites-thickness-numbers = { $prop } est définie par `{ $code }` ; le designer ne réécrit que des arguments de Thickness qui sont des nombres
src-rewrites-new-thickness = { $prop } est définie par `{ $code }` ; le designer ne réécrit que new Thickness(..) avec des arguments numériques
src-no-arguments = aucun argument
src-with-not-numbers = With{ $prop }(..) a des arguments qui ne sont pas des nombres
src-with-misfit = With{ $prop }(..) n’accepte pas cette valeur
src-insert-no-list = le code qui crée les enfants de ce contrôle n’est pas une liste que le designer peut compléter (une liste `Children`, une `new List<SkiaControl>` ou `.WithChildren(..)`)
src-insert-no-initializer = `{ $key }` est créé sans initialiseur d’objet, le designer ne peut donc pas lui donner d’enfants
src-insert-templated = ce contrôle crée ses enfants à partir d’un modèle : ajoutez plutôt un élément à son ItemsSource
src-insert-unmapped = les enfants de ce contrôle ne correspondent pas un à un à son code (une boucle ou une fonction d’aide en crée plusieurs), le designer ne sait donc pas où placer le nouveau contrôle
src-shared-helper = { $helper } crée un tel contrôle pour chacune de ses { $count } utilisations, une valeur écrite là les changerait toutes : changez-la dans { $helper } ou passez-la en argument
src-component-no-prop = { $component } ne prend pas { $prop } : modifiez le code de { $component } ou transmettez ses props à ce contrôle ({"{"}...props{"}"})
src-insert-vec-type = les enfants sont un vec![..] de { $type }, qui ne prend pas de { $new }
src-insert-tuple-full = le tuple des enfants a déjà { $count } contrôles, le maximum qu’il accepte
src-insert-type-unknown = le type de { $prop } est inconnu, sa valeur de départ ne peut donc pas être écrite
src-remove-not-in-list = `{ $key }` n’est pas un élément d’une liste d’enfants, le designer ne peut donc pas le retirer du code
src-move-not-in-list = `{ $key }` n’est pas un élément d’une liste d’enfants, le designer ne peut donc pas le déplacer dans le code
src-move-into-itself = un contrôle ne peut pas être déplacé dans lui-même ni dans l’un de ses enfants
src-move-other-file = le nouveau parent est créé dans un autre fichier ; le designer ne déplace un contrôle qu’à l’intérieur d’un même fichier
hub-read-failed = impossible de lire { $file } : { $error }
hub-changed-since = { $file } a changé à cet endroit depuis l’écriture
hub-write-failed = impossible d’écrire { $file } : { $error }
hub-nothing-written = rien n’a été écrit par cette demande
hub-not-connected = l’app n’est pas connectée
hub-no-project = l’app n’a pas dit de quel projet elle a été compilée
hub-no-tree = l’app n’a pas encore envoyé son arbre
hub-type-unknown = le type de { $prop } est inconnu : l’app ne l’a pas envoyé parmi les propriétés de ce contrôle
hub-no-sources = aucun fichier C# sous { $folder }
hub-changed-during = { $file } a changé pendant la modification
hub-failed = le hub a échoué : { $error }
hub-no-app = aucune app avec l’id { $app } n’est connectée
run-no-way = impossible de lancer une app { $engine } pour l’instant
run-agent-unknown = le chemin de l’agent de l’app est inconnu : lancez-la une fois avec l’agent pour que le designer l’apprenne
run-already = { $project } est déjà lancée (app { $app }) : redémarrez-la plutôt
run-not-recent = { $project } n’est pas une app récente
run-cannot-start = impossible de démarrer { $program } : { $error }
run-build-failed = la compilation a échoué
run-no-connect = aucune app ne s’est connectée en { $seconds } s
run-ended-code = le processus s’est terminé avec le code { $code } avant que l’app se connecte
run-ended = le processus s’est terminé avant que l’app se connecte
connection-devices = Apps sur d’autres appareils
connection-devices-hint = Permet à une app sur un téléphone ou un autre ordinateur de ce réseau de se connecter avec l’adresse ci-dessous. L’adresse contient une clé secrète : toute personne qui l’a peut se connecter.
connection-devices-address = Adresse pour les apps sur d’autres appareils
status-devices-failed = Les apps sur d’autres appareils ne peuvent pas se connecter : { $reason }
hub-no-network = cet ordinateur n’est sur aucun réseau
hub-port-taken = le port { $port } est occupé : { $error }
app-not-running = pas lancée
status-wifi-on = Wi-Fi activé
status-wifi-off = Wi-Fi désactivé
status-usb = USB : { $phones }
apps-not-running = Récentes
status-hot-applied = Modification du code appliquée dans l’app
status-hot-reloaded = Hot reload : l’écran a été reconstruit
gradient-summary = { $count ->
    [one] { $type } · { $count } couleur
   *[other] { $type } · { $count } couleurs
}
gradient-none = aucun
gradient-editor = Dégradé
gradient-type = Type
gradient-bar = Couleurs du dégradé
gradient-bar-hint = Cliquez sur la barre pour ajouter une couleur, faites glisser une poignée pour la déplacer, Suppr retire celle choisie.
gradient-stop-value = Couleur { $index } sur { $count } à { $position } %
gradient-angle = Angle
gradient-angle-less = Tourner de 15° à gauche
gradient-angle-more = Tourner de 15° à droite
row-item = élément { $index }
status-live-cell = { $prop } = { $value } dans cette cellule seulement (élément { $index }). Écrire dans le code change tous les éléments
empty-tree-title = Pas encore de contrôles
empty-tree-hint = Les contrôles apparaissent ici dès que l’app les affiche.
empty-tree-hint-page = Écrivez du code dans Source et ses contrôles apparaissent ici.
choice-empty = (vide)
choice-empty-label = Vide, efface la valeur

## Controls panel

panel-controls = Contrôles
resize-controls = Changer la hauteur d’Écrans et de Contrôles
controls-search = Rechercher un contrôle
control-row-hint = Cliquez, puis cliquez dans l’app à l’endroit voulu. Ou faites-le glisser sur Calques.
empty-controls-title = Aucun contrôle
empty-controls-hint = Connectez une app : les contrôles qu’elle peut créer s’affichent ici.
empty-controls-wait = L’app n’a pas encore listé les contrôles qu’elle peut créer.
empty-controls-match-title = Aucun résultat
empty-controls-match-hint = Aucun contrôle n’a ceci dans son nom.
group-media = Médias
group-input = Saisie
group-shapes = Formes
group-other = Autres
group-own = Cette app
place-title = Placement de { $type }
place-hint = Cliquez dans l’app à l’endroit voulu. Échap annule.
status-inserted = { $type } ajouté dans l’app, pas encore dans le code
status-not-inserted = { $type } n’a pas été ajouté
status-app-refused = L’app a refusé : { $reason }
status-removed = { $type } retiré de l’app
status-removed-code = Retiré de { $file } et de l’app
row-added = ajouté ici, pas encore dans le code
row-added-written = ajouté ici, dans le code
help-controls = Ajouter un contrôle
help-controls-1 = Contrôles, sous Écrans, liste ce que votre app peut créer : les contrôles de la bibliothèque par type, vos propres classes sous Cette app. Tapez dans son champ de recherche pour en trouver un.
help-controls-2 = Cliquez sur un contrôle, puis dans l’app à l’endroit voulu ; Échap annule. Ou faites-le glisser sur Calques : entre deux lignes, ou sur un layout pour le placer dedans.
help-controls-3 = Un nouveau contrôle n’existe que dans l’app, avec un point sur sa ligne de Calques : Écrire le met dans le code, Annuler le retire.
selection-color = Couleur de sélection
selection-color-hint = Le cadre autour du contrôle sélectionné dans votre app.
animate-changes = Animer les changements de thème et de langue
animate-changes-hint = Un nouveau thème ou une nouvelle langue s’ouvre sur la fenêtre en cercle. Désactivé : appliqué tout de suite.
color-magenta = Magenta
color-lime = Citron vert
color-cyan = Cyan
color-orange = Orange
color-white = Blanc
selection-color-other = Autre couleur

## Layers menu

layers-menu = Actions du contrôle
menu-copy = Copier
menu-paste = Coller
menu-duplicate = Dupliquer
menu-copy-style = Copier le style
menu-paste-style = Coller le style
menu-delete = Supprimer
menubar = Barre de menus
menubar-file = &Fichier
menubar-edit = É&dition
menubar-view = &Affichage
menubar-help = A&ide
menu-run-app = Lancer l’app
menu-restart-app = Redémarrer l’app
menu-settings = Réglages
menu-exit = Quitter
menu-undo = Annuler
menu-redo = Rétablir
menu-undo-step = Annuler : { $step }
menu-redo-step = Rétablir : { $step }
menu-props-order = Ordre des propriétés
menu-library-parts = Afficher les parties de la bibliothèque
menu-about = À propos
step-edit = Modifier { $prop }
step-paste-style = Coller le style
step-write = Écrire { $name }
step-write-all = Tout écrire
step-add = Ajouter { $type }
step-paste = Coller { $type }
step-duplicate = Dupliquer { $type }
step-delete = Supprimer { $type }
step-write-delete = Supprimer { $type } du code
status-undone = Annulé : { $step }
status-redone = Rétabli : { $step }
write-all = Tout écrire
write-all-label = Écrire les { $count } modifications dans le code
status-copied = { $type } copié avec { $count ->
    [one] { $count } propriété
   *[other] { $count } propriétés
}
status-style-copied = Style de { $type } copié : { $count ->
    [one] { $count } propriété
   *[other] { $count } propriétés
}
status-style-pasted = { $count ->
    [one] { $count } propriété de style appliquée dans l’app, pas encore dans le code
   *[other] { $count } propriétés de style appliquées dans l’app, pas encore dans le code
}
status-style-none = Rien du style copié ne convient à { $type }
status-removed-live = { $type } retiré de l’app, pas encore du code
status-removed-written = { $type } retiré de l’app et du code
status-restored = { $type } est de retour dans l’app
row-removed = retiré de l’app, pas encore du code
row-removed-written = retiré de l’app et du code
row-removed-tag = retiré
help-copy = Copier, coller et styles
help-copy-1 = Clic droit sur un contrôle dans Calques, ou la touche Menu ou Maj+F10, pour Copier (Ctrl+C), Coller (Ctrl+V), Dupliquer (Ctrl+D), Copier le style (Ctrl+Maj+C), Coller le style (Ctrl+Maj+V) et Supprimer (Suppr).
help-copy-2 = Coller place la copie dans le layout sélectionné, ou juste après le contrôle sélectionné. Coller le style applique les couleurs, polices, contours, coins, ombres et marges intérieures copiés au contrôle sélectionné ; Tout écrire les met dans le code.
help-copy-3 = Un contrôle collé ou supprimé ne change que l’app, avec un point sur sa ligne de Calques : Écrire change aussi le code, Annuler revient en arrière.

## Hub: writes while other live changes wait

hub-not-in-code-yet = { $type } n’est pas encore dans le code : écrivez-le d’abord dans le code
hub-parent-not-in-code = son parent { $type } n’est pas encore dans le code : écrivez-le d’abord
hub-in-code-already = { $type } est déjà dans le code

## Copy with what is inside

status-copied-inside = { $type } copié avec { $count ->
    [one] { $count } contrôle à l’intérieur
   *[other] { $count } contrôles à l’intérieur
}
status-copy-gone = Pas copié : l’app n’affiche plus ce contrôle

## Live changes kept across a rebuild

hub-lost-prop = { $type } { $prop } : son contrôle n’est plus dans le code
hub-lost-insert = { $type } ajouté ici : sa place n’est plus dans le code
hub-lost-remove = { $type } retiré ici : il n’est plus dans le code
status-not-kept = { $count ->
    [one] 1 modification n’a pas pu être gardée : { $what }
   *[other] { $count } modifications n’ont pas pu être gardées : { $what }
}
status-moved = { $type } déplacé dans l’app, pas encore dans le code
status-not-moved = { $type } n’a pas été déplacé
step-move = Déplacer { $type }
row-moved = déplacé ici, pas encore dans le code
row-moved-written = déplacé ici, dans le code
hub-lost-move = { $type } déplacé ici : sa place n’est plus dans le code
help-move = Déplacer des contrôles
help-move-1 = Faites glisser la ligne d’un contrôle dans Calques : les lignes s’écartent là où il arrivera. Entre deux lignes il va là ; sur un layout il va dedans, en dernier. Près du haut ou du bas la liste défile ; Échap annule.
help-move-2 = Alt+Haut et Alt+Bas déplacent le contrôle sélectionné d’une place parmi ses voisins.
help-move-3 = Un contrôle déplacé ne change que l’app, avec un point sur sa ligne de Calques : Écrire le déplace aussi dans le code, Annuler le remet en place.
pick-label-keys = Choisir un contrôle dans l'app (Ctrl+P)
section-code = Code
code-hint = Comment les modifications faites ici arrivent dans le code de l'app.
write-mode = Écrire les modifications dans le code
write-mode-hint = Manuel : vous les écrivez, avec Écrire d'une ligne ou Tout écrire. Auto : elles sont écrites quand le code est nécessaire, avant Redémarrer et Lancer. Toujours : chaque modification terminée est écrite aussitôt.
write-mode-manual = Manuel
write-mode-auto = Auto
write-mode-always = Toujours
changes-not-in-code = { $count ->
    [one] { $count } modification hors du code
   *[other] { $count } modifications hors du code
}
write-changes-label = Écrire les { $count } modifications dans le code
step-write-changes = Tout écrire
unwritten-title = { $count ->
    [one] { $count } modification n'est pas encore dans le code.
   *[other] { $count } modifications ne sont pas encore dans le code.
}
unwritten-write-restart = Les écrire, puis redémarrer
unwritten-skip-restart = Redémarrer sans écrire
unwritten-write-run = Les écrire, puis lancer
unwritten-skip-run = Lancer sans écrire
unwritten-write-close = Les écrire, puis fermer
unwritten-skip-close = Fermer sans écrire
unwritten-cancel = Annuler
