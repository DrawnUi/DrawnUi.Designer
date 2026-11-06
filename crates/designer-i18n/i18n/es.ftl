# DrawnUI Designer, español. Fuente: en.ftl (mismas claves).

## Window

app-title = DrawnUI Designer
apps = Apps
help = Ayuda
settings = Ajustes
back-to-designer = { $page }, volver al diseñador

## Panels

panel-screens = Pantallas
panel-layers = Capas
panel-properties = Propiedades
resize-screens = Cambiar el ancho de Pantallas
resize-properties = Cambiar el ancho de Propiedades
screen-open = Abrir { $title }
controls-count = { $count ->
    [one] { $count } control
   *[other] { $count } controles
}
empty-screens-title = No hay pantallas
empty-screens-hint = Aquí aparecen las rutas del shell de la app.
empty-app-title = Ninguna app conectada
empty-app-hint = Inicia una app DrawnUI con el agente del diseñador, o ejecuta la última.
empty-selection-title = Nada seleccionado
empty-selection-hint = Selecciona un control en Capas para ver y editar sus propiedades.

## Title bar apps

app-building = compilando…
app-starting = iniciando…
app-failed = falló
app-restart = Reiniciar { $name }
app-run = Ejecutar { $name }
app-starting-name = Iniciando { $name }…
app-shown = { $name }, { $detail }, mostrada
app-show = Mostrar { $name }, { $detail }
app-run-detail = Ejecutar { $name }, { $detail }

## Layers

pick = Elegir
pick-active = Haz clic o toca en la app
pick-label = Elegir un control en la app
fold = Plegar { $name }
unfold = Desplegar { $name }
row-level = nivel { $level }
row-folded = plegado
row-open = desplegado
row-selected = seleccionado

## Properties

order = Orden de las propiedades
order-set = Fijadas
order-groups = Tipo
group-changed = Fijadas
group-others = Sin fijar
group-layout = Diseño
group-text = Texto
group-look = Aspecto
group-transform = Transformación
group-behavior = Comportamiento
group-row = { $group }, { $count ->
    [one] { $count } propiedad
   *[other] { $count } propiedades
}, { $state }
group-open = desplegado
group-folded = plegado
prop-changed = cambiada en la app
prop-set = fijada
value = Valor
decrease = Disminuir
increase = Aumentar
undo-prop = Deshacer { $name }
write-prop = Escribir { $name } en el código

## Status line

status-connected = { $count ->
    [one] Hub conectado · { $count } app
   *[other] Hub conectado · { $count } apps
}
status-connecting = Conectando con el hub…
status-run-failed = { $name } falló: { $message }
status-live = { $prop } = { $value } en la app, aún no en el código
status-writing = Escribiendo { $prop } en el código…
status-cancelled = { $prop }: cambio descartado
status-written = Escrito en { $file }, línea { $line }
status-not-written = No se escribió en el código: { $reason }
status-not-undone = No se deshizo en el código: { $reason }
status-undoing = Deshaciendo { $prop } en el código…
status-undone-code = { $prop } vuelve a { $value } en { $file }
status-undone-app = { $prop } vuelve a { $value } en la app

## Settings

settings-sections = Secciones de ajustes
section-appearance = Apariencia
section-layers = Capas
section-connection = Conexión
section-about = Acerca de
appearance-hint = Cómo se ve el diseñador.
theme = Tema
theme-hint = Los colores de toda la ventana. El nuevo se abre desde donde haces clic.
theme-dark = Oscuro
theme-midnight = Medianoche
theme-graphite = Grafito
theme-light = Claro
theme-paper = Papel
theme-high-contrast = Alto contraste
language = Idioma
language-hint = El idioma de toda la ventana.
language-system = Sistema ({ $language })
layers-hint = El árbol de controles de la pantalla que muestra la app.
library-parts = Partes de la biblioteca
library-parts-hint = Mostrar también los controles que DrawnUI crea por sí mismo, como la etiqueta interna de un botón. Ningún código tuyo los crea, así que sus valores no se pueden escribir en el código.
connection-hint = Dónde las apps y los agentes de IA encuentran el diseñador. Las apps en marcha se conectan solas.
connection-apps = Apps
connection-mcp = Servidor MCP para agentes de IA
connection-claude = Añadirlo a Claude Code
copy = Copiar
copied = Copiado

## About

about-logo = Logotipo de DrawnUI
about-version = Versión { $version }
about-description = Mira las pantallas, los controles y las propiedades de una app DrawnUI en marcha, cámbialos en vivo y escribe cada cambio en su código.
made-with = Hecho con
uses = Usa
credit-drawnui-rust = El motor de interfaz dibujada sobre Skia: un solo código para escritorio, teléfonos y la web.
credit-nx = Su protocolo de lenguaje: el código de la interfaz leído como un árbol de nodos y editado nodo a nodo.
credit-tree-sitter = Lee y edita el código C# de la app y deja tal cual cada byte que no cambia.
credit-mcp = El servidor de Model Context Protocol con el que los agentes de IA ven y cambian una app en marcha.
credit-lucide = Los iconos.

## Help

help-title = Usar el diseñador
help-intro = Mira las pantallas, los controles y las propiedades de una app DrawnUI en marcha, cámbialos en vivo y escribe cada cambio en el código C# de la app.
help-connect = Conectar una app
help-connect-1 = Ejecuta tu app con el agente del diseñador: se conecta sola y aparece en la barra de título.
help-connect-2 = Una app que ya se ejecutó también se queda ahí: haz clic para ejecutarla de nuevo. La flecha redonda de una app en marcha la reinicia.
help-connect-3 = Con varias apps conectadas, haz clic en una para mostrarla aquí; en un ordenador su ventana pasa al frente.
help-phones = Teléfonos
help-phones-1 = Un iPhone o iPad conectado por cable USB se conecta solo: ejecuta en él una compilación Debug de tu app con el agente del diseñador. No hace falta Wi-Fi.
help-phones-2 = Elegir y luego toca un control en el teléfono para seleccionarlo aquí. Pulsa Elegir otra vez para parar.
help-phones-3 = Sin cable: Ajustes, Conexión, Apps en otros dispositivos da la dirección con la que compilar la app.
help-screens = Pantallas
help-screens-1 = Las páginas que puede abrir el shell de la app. Haz clic en una para abrirla en la app.
help-layers = Capas
help-layers-1 = Los controles de la pantalla que muestra la app, como un árbol. Haz clic en uno para seleccionarlo: aparece un recuadro a su alrededor en la app.
help-layers-2 = Haz doble clic en una fila, o clic en su casilla de la línea del árbol, para plegarla o desplegarla.
help-layers-3 = Elegir y luego haz clic en un control de la app, o tócalo en un teléfono, para seleccionarlo aquí.
help-properties = Propiedades
help-properties-1 = Haz clic en un valor para cambiarlo: escríbelo, usa - y + con los números, elige de una lista o elige un color.
help-properties-2 = Enter lo aplica en la app en marcha. Esc lo deja como estaba.
help-properties-3 = Las propiedades cambiadas se ven más claras. Fijadas las muestra primero (Fijadas, luego Sin fijar), Tipo por clase.
help-write = Escribir en el código
help-write-1 = El botón de código de una fila escribe el valor en el archivo C# que crea el control: solo cambia ese valor, el resto del archivo queda igual.
help-write-2 = Deshacer revierte el último cambio de una fila: en la app y, tras escribirlo, también en el archivo.
help-write-3 = La línea de abajo dice qué archivo cambió o por qué no se escribió nada.
help-write-4 = Ctrl+Z deshace tu última acción, sea cual sea: un cambio, una escritura, un pegado, un borrado. Ctrl+Y la rehace. El menú Editar nombra ambas.
help-keyboard = Teclado
help-keyboard-1 = Tab pasa de un grupo a otro, las flechas se mueven dentro de una lista o un menú, Enter o Espacio actúa.
help-keyboard-2 = Esc cierra un menú, sale de una edición y vuelve desde Ajustes o Ayuda.
help-keyboard-3 = La barra de menús: haz clic en un título o pulsa Alt con su letra subrayada. Sus elementos muestran sus teclas, que también funcionan con los menús cerrados.
help-agents = Agentes de IA
help-agents-1 = Los agentes usan las mismas funciones por MCP en { $address }.
help-agents-2 = Ajustes, Conexión tiene la dirección y el comando que la añade a Claude Code.

## Color picker

picker = Color
picker-plane = Saturación y brillo
picker-plane-value = saturación { $saturation } %, brillo { $brightness } %
picker-hue = Tono
picker-opacity = Opacidad
picker-hex = Color hexadecimal
picker-old-new = Antes: { $old }, el nuevo a la derecha
picker-keys = Las flechas mueven la barra o el cuadro elegido (Mayús: diez pasos). Esc devuelve el color anterior.

## Hub

src-not-number = `{ $text }` no es un número
src-not-bool = `{ $text }` no es true ni false
src-not-color = `{ $text }` no es un color #RGB, #RRGGBB o #AARRGGBB
src-not-thickness = `{ $text }` no es un grosor ("todos", "h,v" o "i,s,d,inf")
src-type-unsupported = los valores { $type } aún no se pueden escribir en el código
src-not-in-tree = el control no está en el último árbol de la app
src-library-part = la biblioteca creó este control para sus propias partes: ningún código lo declara
src-no-code = no se encontró código que cree este control
src-arg-no-value = { $prop } viene del argumento `{ $param }` de `{ $call }`, que aquí no tiene valor
src-several-args = { $prop } se alimenta de varios argumentos de `{ $call }`
src-key-missing = `{ $key }` no está en el código
src-no-value = la propiedad no tiene valor
src-not-literal = `{ $key }` no es un literal que el diseñador pueda cambiar (una expresión o una llamada)
src-value-misfit = el valor no encaja en `{ $key }` (otro tipo de literal)
src-not-single = el cambio no es un único cambio
src-empty-parens = paréntesis vacíos
src-rewrites-colors = { $prop } se fija con `{ $code }`; el diseñador solo reescribe Color.Parse("#..") y Colors.X
src-rewrites-thickness-numbers = { $prop } se fija con `{ $code }`; el diseñador solo reescribe argumentos de Thickness que sean números
src-rewrites-new-thickness = { $prop } se fija con `{ $code }`; el diseñador solo reescribe new Thickness(..) con argumentos numéricos
src-no-arguments = sin argumentos
src-with-not-numbers = With{ $prop }(..) tiene argumentos que no son números
src-with-misfit = With{ $prop }(..) no acepta este valor
src-insert-no-list = el código que crea los hijos de este control no es una lista a la que el diseñador pueda añadir (una lista `Children`, una `new List<SkiaControl>` o `.WithChildren(..)`)
src-insert-no-initializer = `{ $key }` se crea sin inicializador de objeto, así que el diseñador no puede darle hijos
src-insert-templated = este control crea sus hijos con una plantilla: añade un elemento a su ItemsSource
src-insert-unmapped = los hijos de este control no corresponden uno a uno con su código (un bucle o una función auxiliar crea varios), así que el diseñador no sabe dónde va el nuevo control
src-shared-helper = { $helper } crea un control así en cada uno de sus { $count } usos, así que un valor escrito ahí los cambiaría todos: cámbialo en { $helper } o pásalo como argumento
src-component-no-prop = { $component } no acepta { $prop }: edita el código de { $component } o pasa sus props a este control ({"{"}...props{"}"})
src-insert-vec-type = los hijos son un vec![..] de { $type }, que no admite un { $new }
src-insert-tuple-full = la tupla de hijos ya tiene { $count } controles, el máximo que admite
src-insert-type-unknown = el tipo de { $prop } es desconocido, así que no se puede escribir su valor inicial
src-remove-not-in-list = `{ $key }` no es un elemento de una lista de hijos, así que el diseñador no puede quitarlo del código
src-move-not-in-list = `{ $key }` no es un elemento de una lista de hijos, así que el diseñador no puede moverlo en el código
src-move-into-itself = un control no se puede mover dentro de sí mismo ni de uno de sus hijos
src-move-other-file = el nuevo padre se crea en otro archivo; el diseñador solo mueve un control dentro de un mismo archivo
hub-read-failed = no se pudo leer { $file }: { $error }
hub-changed-since = { $file } cambió en ese punto desde que se escribió
hub-write-failed = no se pudo escribir { $file }: { $error }
hub-nothing-written = esa petición no escribió nada
hub-not-connected = la app no está conectada
hub-no-project = la app no dijo con qué proyecto se compiló
hub-no-tree = la app aún no ha enviado su árbol
hub-type-unknown = el tipo de { $prop } es desconocido: la app no lo envió entre las propiedades de este control
hub-no-sources = no hay archivos C# en { $folder }
hub-changed-during = { $file } cambió mientras se hacía el cambio
hub-failed = el hub falló: { $error }
hub-no-app = no hay ninguna app conectada con el id { $app }
run-no-way = aún no se puede ejecutar una app { $engine }
run-agent-unknown = se desconoce la ruta del agente de la app: iníciala una vez con el agente para que el diseñador la conozca
run-already = { $project } ya está en marcha (app { $app }): reiníciala en su lugar
run-not-recent = { $project } no es una app reciente
run-cannot-start = no se pudo iniciar { $program }: { $error }
run-build-failed = la compilación falló
run-no-connect = ninguna app se conectó en { $seconds } s
run-ended-code = el proceso terminó con el código { $code } antes de que la app se conectara
run-ended = el proceso terminó antes de que la app se conectara
connection-devices = Apps en otros dispositivos
connection-devices-hint = Permite que una app en un teléfono u otro ordenador de esta red se conecte con la dirección de abajo. La dirección lleva una clave secreta: quien la tenga puede conectarse.
connection-devices-address = Dirección para apps en otros dispositivos
status-devices-failed = Las apps en otros dispositivos no pueden conectarse: { $reason }
hub-no-network = este ordenador no está en ninguna red
hub-port-taken = el puerto { $port } está ocupado: { $error }
app-not-running = no está en marcha
status-wifi-on = Wi-Fi activado
status-wifi-off = Wi-Fi desactivado
status-usb = USB: { $phones }
apps-not-running = Recientes
status-hot-applied = Cambio de código aplicado en la app
status-hot-reloaded = Hot reload: la pantalla se reconstruyó
gradient-summary = { $count ->
    [one] { $type } · { $count } color
   *[other] { $type } · { $count } colores
}
gradient-none = ninguno
gradient-editor = Degradado
gradient-type = Tipo
gradient-bar = Colores del degradado
gradient-bar-hint = Haz clic en la barra para añadir un color, arrastra un tirador para moverlo, Supr elimina el seleccionado.
gradient-stop-value = Color { $index } de { $count } en { $position } %
gradient-angle = Ángulo
gradient-angle-less = Girar 15° a la izquierda
gradient-angle-more = Girar 15° a la derecha
row-item = elemento { $index }
status-live-cell = { $prop } = { $value } solo en esta celda (elemento { $index }). Escribir en el código cambia todos los elementos
empty-tree-title = Aún no hay controles
empty-tree-hint = Los controles aparecen aquí en cuanto la app los muestra.
empty-tree-hint-page = Escribe código en Source y sus controles aparecen aquí.
choice-empty = (vacío)
choice-empty-label = Vacío, borra el valor

## Controls panel

panel-controls = Controles
resize-controls = Cambiar el alto de Pantallas y Controles
controls-search = Buscar controles
control-row-hint = Haz clic y luego haz clic en la app donde va. O arrástralo a Capas.
empty-controls-title = No hay controles
empty-controls-hint = Conecta una app: los controles que puede crear aparecen aquí.
empty-controls-wait = La app aún no ha listado los controles que puede crear.
empty-controls-match-title = Sin resultados
empty-controls-match-hint = Ningún control tiene esto en su nombre.
group-media = Multimedia
group-input = Entrada
group-shapes = Formas
group-other = Otros
group-own = Esta app
place-title = Colocando { $type }
place-hint = Haz clic en la app donde va. Esc cancela.
status-inserted = { $type } añadido en la app, aún no en el código
status-not-inserted = { $type } no se añadió
status-app-refused = La app lo rechazó: { $reason }
status-removed = { $type } quitado de la app
status-removed-code = Quitado de { $file } y de la app
row-added = añadido aquí, aún no en el código
row-added-written = añadido aquí, en el código
help-controls = Añadir un control
help-controls-1 = Controles, bajo Pantallas, lista lo que tu app puede crear: los controles de la biblioteca por tipo, tus propias clases en Esta app. Escribe en su campo de búsqueda para encontrar uno.
help-controls-2 = Haz clic en un control y luego en la app donde va; Esc cancela. O arrástralo a Capas: entre dos filas, o sobre un layout para ponerlo dentro.
help-controls-3 = Un control nuevo está solo en la app, con un punto en su fila de Capas: Escribir lo pone en el código, Deshacer lo quita.
selection-color = Color de selección
selection-color-hint = El marco alrededor del control seleccionado en tu app.
animate-changes = Animar los cambios de tema e idioma
animate-changes-hint = Un tema o idioma nuevo se abre sobre la ventana en un círculo. Desactivado: se aplica al instante.
color-magenta = Magenta
color-lime = Lima
color-cyan = Cian
color-orange = Naranja
color-white = Blanco
selection-color-other = Otro color

## Layers menu

layers-menu = Acciones del control
menu-copy = Copiar
menu-paste = Pegar
menu-duplicate = Duplicar
menu-copy-style = Copiar estilo
menu-paste-style = Pegar estilo
menu-delete = Eliminar
menubar = Barra de menús
menubar-file = &Archivo
menubar-edit = &Editar
menubar-view = &Ver
menubar-help = Ay&uda
menu-run-app = Ejecutar app
menu-restart-app = Reiniciar app
menu-settings = Ajustes
menu-exit = Salir
menu-undo = Deshacer
menu-redo = Rehacer
menu-undo-step = Deshacer: { $step }
menu-redo-step = Rehacer: { $step }
menu-props-order = Orden de propiedades
menu-library-parts = Mostrar partes de la biblioteca
menu-about = Acerca de
step-edit = Cambiar { $prop }
step-paste-style = Pegar estilo
step-write = Escribir { $name }
step-write-all = Escribir todo
step-add = Añadir { $type }
step-paste = Pegar { $type }
step-duplicate = Duplicar { $type }
step-delete = Eliminar { $type }
step-write-delete = Eliminar { $type } del código
status-undone = Deshecho: { $step }
status-redone = Rehecho: { $step }
write-all = Escribir todo
write-all-label = Escribir los { $count } cambios en el código
status-copied = { $type } copiado con { $count ->
    [one] { $count } propiedad
   *[other] { $count } propiedades
}
status-style-copied = Estilo de { $type } copiado: { $count ->
    [one] { $count } propiedad
   *[other] { $count } propiedades
}
status-style-pasted = { $count ->
    [one] { $count } propiedad de estilo puesta en la app, aún no en el código
   *[other] { $count } propiedades de estilo puestas en la app, aún no en el código
}
status-style-none = Nada del estilo copiado sirve para { $type }
status-removed-live = { $type } quitado de la app, aún no del código
status-removed-written = { $type } quitado de la app y del código
status-restored = { $type } vuelve a estar en la app
row-removed = quitado de la app, aún no del código
row-removed-written = quitado de la app y del código
row-removed-tag = quitado
help-copy = Copiar, pegar y estilos
help-copy-1 = Haz clic derecho en un control de Capas, o pulsa la tecla Menú o Mayús+F10, para Copiar (Ctrl+C), Pegar (Ctrl+V), Duplicar (Ctrl+D), Copiar estilo (Ctrl+Mayús+C), Pegar estilo (Ctrl+Mayús+V) y Eliminar (Supr).
help-copy-2 = Pegar pone la copia dentro del layout seleccionado, o justo después del control seleccionado. Pegar estilo pone los colores, fuentes, trazos, esquinas, sombras y relleno copiados en el control seleccionado; Escribir todo los pone en el código.
help-copy-3 = Un control pegado o eliminado cambia solo la app, con un punto en su fila de Capas: Escribir cambia también el código, Deshacer lo revierte.

## Hub: writes while other live changes wait

hub-not-in-code-yet = { $type } aún no está en el código: escríbelo primero en el código
hub-parent-not-in-code = su padre { $type } aún no está en el código: escribe primero ese
hub-in-code-already = { $type } ya está en el código

## Copy with what is inside

status-copied-inside = { $type } copiado con { $count ->
    [one] { $count } control dentro
   *[other] { $count } controles dentro
}
status-copy-gone = No copiado: la app ya no muestra ese control

## Live changes kept across a rebuild

hub-lost-prop = { $type } { $prop }: su control ya no está en el código
hub-lost-insert = { $type } añadido aquí: su lugar ya no está en el código
hub-lost-remove = { $type } quitado aquí: ya no está en el código
status-not-kept = { $count ->
    [one] 1 cambio no se pudo conservar: { $what }
   *[other] { $count } cambios no se pudieron conservar: { $what }
}
status-moved = { $type } movido en la app, aún no en el código
status-not-moved = { $type } no se movió
step-move = Mover { $type }
row-moved = movido aquí, aún no en el código
row-moved-written = movido aquí, en el código
hub-lost-move = { $type } movido aquí: su lugar ya no está en el código
help-move = Mover controles
help-move-1 = Arrastra la fila de un control en Capas: las filas abren un hueco donde caerá. Entre dos filas va ahí; sobre un layout va dentro, al final. Cerca del borde superior o inferior la lista se desplaza; Esc cancela.
help-move-2 = Alt+Arriba y Alt+Abajo mueven el control seleccionado un lugar entre sus hermanos.
help-move-3 = Un control movido cambia solo la app, con un punto en su fila de Capas: Escribir lo mueve también en el código, Deshacer lo devuelve.
pick-label-keys = Elegir un control en la app (Ctrl+P)
section-code = Código
code-hint = Cómo llegan al código de la app los cambios que haces aquí.
write-mode = Escribir los cambios en el código
write-mode-hint = Manual: los escribes tú, con Escribir de una fila o Escribir todo. Automático: se escriben cuando se necesita el código, antes de Reiniciar y Ejecutar. Siempre: cada cambio terminado se escribe al momento.
write-mode-manual = Manual
write-mode-auto = Automático
write-mode-always = Siempre
changes-not-in-code = { $count ->
    [one] { $count } cambio sin escribir en el código
   *[other] { $count } cambios sin escribir en el código
}
write-changes-label = Escribir los { $count } cambios en el código
step-write-changes = Escribir todos los cambios
unwritten-title = { $count ->
    [one] { $count } cambio aún no está en el código.
   *[other] { $count } cambios aún no están en el código.
}
unwritten-write-restart = Escribirlos y reiniciar
unwritten-skip-restart = Reiniciar sin escribir
unwritten-write-run = Escribirlos y ejecutar
unwritten-skip-run = Ejecutar sin escribir
unwritten-write-close = Escribirlos y cerrar
unwritten-skip-close = Cerrar sin escribir
unwritten-cancel = Cancelar
