# DrawnUI Designer, русский. Источник: en.ftl (те же ключи).

## Window

app-title = DrawnUI Designer
apps = Приложения
help = Справка
settings = Настройки
back-to-designer = { $page }, назад в дизайнер

## Panels

panel-screens = Экраны
panel-layers = Слои
panel-properties = Свойства
resize-screens = Изменить ширину Экранов
resize-properties = Изменить ширину Свойств
screen-open = Открыть { $title }
controls-count = { $count ->
    [one] { $count } элемент
    [few] { $count } элемента
   *[many] { $count } элементов
}
empty-screens-title = Нет экранов
empty-screens-hint = Здесь появятся маршруты оболочки приложения.
empty-app-title = Нет подключённого приложения
empty-app-hint = Запустите приложение DrawnUI с агентом дизайнера или запустите последнее.
empty-selection-title = Ничего не выбрано
empty-selection-hint = Выберите элемент в Слоях, чтобы увидеть и изменить его свойства.

## Title bar apps

app-building = сборка…
app-starting = запуск…
app-failed = ошибка
app-restart = Перезапустить { $name }
app-run = Запустить { $name }
app-starting-name = Запуск { $name }…
app-shown = { $name }, { $detail }, показано
app-show = Показать { $name }, { $detail }
app-run-detail = Запустить { $name }, { $detail }

## Layers

pick = Выбрать
pick-active = Нажмите в приложении
pick-label = Выбрать элемент в приложении
fold = Свернуть { $name }
unfold = Развернуть { $name }
row-level = уровень { $level }
row-folded = свёрнуто
row-open = развёрнуто
row-selected = выбрано

## Properties

order = Порядок свойств
order-set = Заданные
order-groups = Тип
group-changed = Заданные
group-others = Не заданные
group-layout = Разметка
group-text = Текст
group-look = Вид
group-transform = Трансформация
group-behavior = Поведение
group-row = { $group }, { $count ->
    [one] { $count } свойство
    [few] { $count } свойства
   *[many] { $count } свойств
}, { $state }
group-open = развёрнуто
group-folded = свёрнуто
prop-changed = изменено в приложении
prop-set = задано
value = Значение
decrease = Уменьшить
increase = Увеличить
undo-prop = Отменить { $name }
write-prop = Записать { $name } в код

## Status line

status-connected = { $count ->
    [one] Хаб подключён · { $count } приложение
    [few] Хаб подключён · { $count } приложения
   *[many] Хаб подключён · { $count } приложений
}
status-connecting = Подключение к хабу…
status-run-failed = { $name }: ошибка: { $message }
status-live = { $prop } = { $value } в приложении, ещё не в коде
status-writing = { $prop } записывается в код…
status-cancelled = { $prop }: изменение отменено
status-written = Записано в { $file }, строка { $line }
status-not-written = Не записано в код: { $reason }
status-not-undone = Не отменено в коде: { $reason }
status-undoing = Отмена { $prop } в коде…
status-undone-code = { $prop } снова { $value } в { $file }
status-undone-app = { $prop } снова { $value } в приложении

## Settings

settings-sections = Разделы настроек
section-appearance = Оформление
section-layers = Слои
section-connection = Подключение
section-about = О программе
appearance-hint = Как выглядит дизайнер.
theme = Тема
theme-hint = Цвета всего окна. Новая тема раскрывается с места щелчка.
theme-dark = Тёмная
theme-midnight = Полночь
theme-graphite = Графит
theme-light = Светлая
theme-paper = Бумага
theme-high-contrast = Высокий контраст
language = Язык
language-hint = Язык всего окна.
language-system = Системный ({ $language })
layers-hint = Дерево элементов экрана, который показывает приложение.
library-parts = Части библиотеки
library-parts-hint = Показывать и элементы, которые DrawnUI создаёт сам, например внутреннюю надпись кнопки. Их не создаёт ваш код, поэтому их значения нельзя записать в код.
connection-hint = Где приложения и ИИ-агенты находят дизайнер. Запущенные приложения подключаются сами.
connection-apps = Приложения
connection-mcp = MCP-сервер для ИИ-агентов
connection-claude = Добавить в Claude Code
copy = Копировать
copied = Скопировано

## About

about-logo = Логотип DrawnUI
about-version = Версия { $version }
about-description = Смотрите экраны, элементы и свойства запущенного приложения DrawnUI, меняйте их вживую и записывайте каждое изменение обратно в его код.
made-with = Сделано на
uses = Использует
credit-drawnui-rust = Движок рисованного интерфейса на Skia: один код для компьютера, телефонов и веба.
credit-nx = Его языковой протокол: код интерфейса читается как дерево узлов и правится узел за узлом.
credit-tree-sitter = Читает и правит C#-код приложения, оставляя как было каждый байт, который не меняется.
credit-mcp = Сервер Model Context Protocol, через который ИИ-агенты видят и меняют запущенное приложение.
credit-lucide = Значки.

## Help

help-title = Как пользоваться дизайнером
help-intro = Смотрите экраны, элементы и свойства запущенного приложения DrawnUI, меняйте их вживую и записывайте каждое изменение в C#-код приложения.
help-connect = Подключить приложение
help-connect-1 = Запустите приложение с агентом дизайнера: оно подключится само и появится в заголовке окна.
help-connect-2 = Приложение, которое уже запускалось, тоже остаётся там: щёлкните его, чтобы запустить снова. Круглая стрелка запущенного приложения перезапускает его.
help-connect-3 = Если подключено несколько приложений, нажмите на одно, чтобы показать его здесь; на компьютере его окно выйдет вперёд.
help-phones = Телефоны
help-phones-1 = iPhone или iPad, подключённый по USB-кабелю, подключается сам: запустите на нём Debug-сборку приложения с агентом дизайнера. Wi-Fi не нужен.
help-phones-2 = Нажмите «Выбрать», затем коснитесь элемента на телефоне, чтобы выбрать его здесь. Нажмите «Выбрать» ещё раз, чтобы остановить.
help-phones-3 = Без кабеля: Настройки, Подключение, Приложения на других устройствах дают адрес, с которым собирать приложение.
help-screens = Экраны
help-screens-1 = Страницы, которые может открыть оболочка приложения. Щёлкните одну, чтобы открыть её в приложении.
help-layers = Слои
help-layers-1 = Элементы экрана, который показывает приложение, в виде дерева. Щёлкните элемент, чтобы выбрать его: в приложении вокруг него появится рамка.
help-layers-2 = Дважды щёлкните строку или щёлкните её квадратик на линии дерева, чтобы свернуть или развернуть её.
help-layers-3 = Нажмите «Выбрать», затем щёлкните элемент в приложении или коснитесь его на телефоне, чтобы выбрать его здесь.
help-properties = Свойства
help-properties-1 = Щёлкните значение, чтобы изменить его: введите его, используйте - и + для чисел, выберите из списка или выберите цвет.
help-properties-2 = Enter применяет его в запущенном приложении. Esc возвращает как было.
help-properties-3 = Изменённые свойства написаны ярче. Заданные показывает их первыми (Заданные, затем Не заданные), Тип по виду.
help-write = Запись в код
help-write-1 = Кнопка кода в строке записывает значение в C#-файл, который создаёт элемент: меняется только это значение, остальной файл остаётся как был.
help-write-2 = Отменить возвращает последнее изменение строки: в приложении и, после записи, в файле.
help-write-3 = Строка внизу говорит, какой файл изменён или почему ничего не записано.
help-write-4 = Ctrl+Z отменяет ваше последнее действие, каким бы оно ни было: правку, запись, вставку, удаление. Ctrl+Y повторяет его. Меню Правка называет оба.
help-keyboard = Клавиатура
help-keyboard-1 = Tab переходит между группами, стрелки двигаются внутри списка или меню, Enter или пробел выполняет.
help-keyboard-2 = Esc закрывает меню, выходит из правки и возвращает из Настроек или Справки.
help-keyboard-3 = Строка меню: щёлкните заголовок или нажмите Alt с его подчёркнутой буквой. Пункты показывают свои клавиши, они работают и при закрытых меню.
help-agents = ИИ-агенты
help-agents-1 = Агенты пользуются теми же возможностями через MCP по адресу { $address }.
help-agents-2 = В Настройках, раздел Подключение, есть адрес и команда, которая добавляет его в Claude Code.

## Color picker

picker = Цвет
picker-plane = Насыщенность и яркость
picker-plane-value = насыщенность { $saturation } %, яркость { $brightness } %
picker-hue = Оттенок
picker-opacity = Непрозрачность
picker-hex = Цвет в HEX
picker-old-new = Было: { $old }, новый справа
picker-keys = Стрелки двигают выбранную полосу или квадрат (Shift: десять шагов). Esc возвращает прежний цвет.

## Hub

src-not-number = `{ $text }` — не число
src-not-bool = `{ $text }` — не true и не false
src-not-color = `{ $text }` — не цвет #RGB, #RRGGBB или #AARRGGBB
src-not-thickness = `{ $text }` — не отступ ("все", "г,в" или "л,в,п,н")
src-type-unsupported = значения типа { $type } пока нельзя записать в код
src-not-in-tree = элемента нет в последнем дереве приложения
src-library-part = этот элемент библиотека создала для своих частей: его не объявляет никакой код
src-no-code = не найден код, который создаёт этот элемент
src-arg-no-value = { $prop } берётся из аргумента `{ $param }` вызова `{ $call }`, у которого здесь нет значения
src-several-args = { $prop } задаётся несколькими аргументами `{ $call }`
src-key-missing = `{ $key }` нет в коде
src-no-value = у свойства нет значения
src-not-literal = `{ $key }` — не литерал, который дизайнер может изменить (выражение или вызов)
src-value-misfit = значение не подходит для `{ $key }` (литерал другого вида)
src-not-single = правка — не одно изменение
src-empty-parens = пустые скобки
src-rewrites-colors = { $prop } задаётся через `{ $code }`; дизайнер переписывает только Color.Parse("#..") и Colors.X
src-rewrites-thickness-numbers = { $prop } задаётся через `{ $code }`; дизайнер переписывает только числовые аргументы Thickness
src-rewrites-new-thickness = { $prop } задаётся через `{ $code }`; дизайнер переписывает только new Thickness(..) с числовыми аргументами
src-no-arguments = нет аргументов
src-with-not-numbers = у With{ $prop }(..) есть аргументы, которые не числа
src-with-misfit = With{ $prop }(..) не принимает это значение
src-insert-no-list = код, который создаёт дочерние элементы этого элемента, не является списком, в который дизайнер может добавить (список `Children`, `new List<SkiaControl>` или `.WithChildren(..)`)
src-insert-no-initializer = `{ $key }` создаётся без инициализатора объекта, поэтому дизайнер не может добавить в него дочерние элементы
src-insert-templated = этот элемент создаёт дочерние элементы по шаблону: добавьте элемент в его ItemsSource
src-insert-unmapped = дочерние элементы этого элемента не соответствуют его коду один к одному (цикл или вспомогательная функция создаёт несколько), поэтому дизайнер не знает, куда поставить новый элемент
src-shared-helper = { $helper } создаёт такой элемент при каждом из { $count } вызовов, значение, записанное там, изменило бы их все: измените его в { $helper } или передайте аргументом
src-component-no-prop = { $component } не принимает { $prop }: измените код { $component } или передайте его props этому элементу ({"{"}...props{"}"})
src-insert-vec-type = дочерние элементы — это vec![..] из { $type }, в него нельзя добавить { $new }
src-insert-tuple-full = в кортеже дочерних элементов уже { $count } элементов, больше он не принимает
src-insert-type-unknown = тип { $prop } неизвестен, поэтому его начальное значение нельзя записать
src-remove-not-in-list = `{ $key }` не является пунктом списка дочерних элементов, поэтому дизайнер не может удалить его из кода
src-move-not-in-list = `{ $key }` не является пунктом списка дочерних элементов, поэтому дизайнер не может переместить его в коде
src-move-into-itself = элемент нельзя переместить в него самого или в один из его дочерних элементов
src-move-other-file = новый родитель создаётся в другом файле; дизайнер перемещает элемент только внутри одного файла
hub-read-failed = не удалось прочитать { $file }: { $error }
hub-changed-since = { $file } изменился в этом месте после записи
hub-write-failed = не удалось записать { $file }: { $error }
hub-nothing-written = этим запросом ничего не записано
hub-not-connected = приложение не подключено
hub-no-project = приложение не сообщило, из какого проекта оно собрано
hub-no-tree = приложение ещё не прислало своё дерево
hub-type-unknown = тип { $prop } неизвестен: приложение не прислало его среди свойств этого элемента
hub-no-sources = нет C#-файлов в { $folder }
hub-changed-during = { $file } изменился во время правки
hub-failed = ошибка хаба: { $error }
hub-no-app = приложение с id { $app } не подключено
run-no-way = приложение { $engine } пока нельзя запустить
run-agent-unknown = путь к агенту приложения неизвестен: запустите его один раз с агентом, чтобы дизайнер его узнал
run-already = { $project } уже запущено (приложение { $app }): лучше перезапустите его
run-not-recent = { $project } нет среди недавних приложений
run-cannot-start = не удалось запустить { $program }: { $error }
run-build-failed = сборка не удалась
run-no-connect = ни одно приложение не подключилось за { $seconds } с
run-ended-code = процесс завершился с кодом { $code } до подключения приложения
run-ended = процесс завершился до подключения приложения
connection-devices = Приложения на других устройствах
connection-devices-hint = Позволяет приложению на телефоне или другом компьютере в этой сети подключиться по адресу ниже. В адресе есть секретный ключ: подключиться может любой, у кого он есть.
connection-devices-address = Адрес для приложений на других устройствах
status-devices-failed = Приложения на других устройствах не могут подключиться: { $reason }
hub-no-network = этот компьютер не подключён к сети
hub-port-taken = порт { $port } занят: { $error }
app-not-running = не запущено
status-wifi-on = Wi-Fi вкл.
status-wifi-off = Wi-Fi выкл.
status-usb = USB: { $phones }
apps-not-running = Недавние
status-hot-applied = Изменение кода применено в приложении
status-hot-reloaded = Hot reload: экран перестроен
gradient-summary = { $count ->
    [one] { $type } · { $count } цвет
    [few] { $type } · { $count } цвета
   *[other] { $type } · { $count } цветов
}
gradient-none = нет
gradient-editor = Градиент
gradient-type = Тип
gradient-bar = Цвета градиента
gradient-bar-hint = Нажмите на полосу, чтобы добавить цвет, перетащите маркер, чтобы сдвинуть его, Delete удаляет выбранный.
gradient-stop-value = Цвет { $index } из { $count } на { $position } %
gradient-angle = Угол
gradient-angle-less = Повернуть на 15° влево
gradient-angle-more = Повернуть на 15° вправо
row-item = элемент { $index }
status-live-cell = { $prop } = { $value } только в этой ячейке (элемент { $index }). Запись в код меняет все элементы
empty-tree-title = Пока нет элементов
empty-tree-hint = Элементы появятся здесь, как только приложение их покажет.
empty-tree-hint-page = Напишите код в Source, и его элементы появятся здесь.
choice-empty = (пусто)
choice-empty-label = Пусто, очищает значение

## Controls panel

panel-controls = Элементы
resize-controls = Изменить высоту Экранов и Элементов
controls-search = Поиск элементов
control-row-hint = Нажмите, затем нажмите в приложении там, где он должен быть. Или перетащите его на Слои.
empty-controls-title = Нет элементов
empty-controls-hint = Подключите приложение: здесь появятся элементы, которые оно может создать.
empty-controls-wait = Приложение ещё не сообщило, какие элементы оно может создать.
empty-controls-match-title = Ничего не найдено
empty-controls-match-hint = Ни у одного элемента нет этого в названии.
group-media = Медиа
group-input = Ввод
group-shapes = Фигуры
group-other = Прочее
group-own = Это приложение
place-title = Размещение { $type }
place-hint = Нажмите в приложении там, где он должен быть. Esc отменяет.
status-inserted = { $type } добавлен в приложение, в коде пока нет
status-not-inserted = { $type } не добавлен
status-app-refused = Приложение отказало: { $reason }
status-removed = { $type } убран из приложения
status-removed-code = Убран из { $file } и из приложения
row-added = добавлен здесь, в коде пока нет
row-added-written = добавлен здесь, в коде
help-controls = Добавить элемент
help-controls-1 = Элементы, под Экранами, перечисляет то, что может создать ваше приложение: элементы библиотеки по видам, ваши собственные классы в группе Это приложение. Введите название в поле поиска, чтобы найти элемент.
help-controls-2 = Нажмите на элемент, затем в приложении там, где он должен быть; Esc отменяет. Или перетащите его на Слои: между двумя строками или на разметку, чтобы положить его внутрь.
help-controls-3 = Новый элемент есть только в приложении, с точкой в его строке Слоёв: Записать добавляет его в код, Отменить убирает.
selection-color = Цвет выделения
selection-color-hint = Рамка вокруг выбранного элемента в вашем приложении.
animate-changes = Анимировать смену темы и языка
animate-changes-hint = Новая тема или язык открывается поверх окна кругом. Выключено: применяется сразу.
color-magenta = Пурпурный
color-lime = Лаймовый
color-cyan = Голубой
color-orange = Оранжевый
color-white = Белый
selection-color-other = Другой цвет

## Layers menu

layers-menu = Действия с элементом
menu-copy = Копировать
menu-paste = Вставить
menu-duplicate = Дублировать
menu-copy-style = Копировать стиль
menu-paste-style = Вставить стиль
menu-delete = Удалить
menubar = Строка меню
menubar-file = &Файл
menubar-edit = &Правка
menubar-view = &Вид
menubar-help = &Справка
menu-run-app = Запустить приложение
menu-restart-app = Перезапустить приложение
menu-settings = Настройки
menu-exit = Выход
menu-undo = Отменить
menu-redo = Повторить
menu-undo-step = Отменить: { $step }
menu-redo-step = Повторить: { $step }
menu-props-order = Порядок свойств
menu-library-parts = Показывать части библиотеки
menu-about = О программе
step-edit = Изменить { $prop }
step-paste-style = Вставить стиль
step-write = Записать { $name }
step-write-all = Записать все
step-add = Добавить { $type }
step-paste = Вставить { $type }
step-duplicate = Дублировать { $type }
step-delete = Удалить { $type }
step-write-delete = Удалить { $type } из кода
status-undone = Отменено: { $step }
status-redone = Повторено: { $step }
write-all = Записать все
write-all-label = Записать все изменения в код: { $count }
status-copied = { $type } скопирован, { $count ->
    [one] { $count } свойство
    [few] { $count } свойства
   *[many] { $count } свойств
}
status-style-copied = Стиль { $type } скопирован, { $count ->
    [one] { $count } свойство
    [few] { $count } свойства
   *[many] { $count } свойств
}
status-style-pasted = Свойства стиля в приложении, в коде пока нет: { $count }
status-style-none = Скопированный стиль не подходит к { $type }
status-removed-live = { $type } убран из приложения, из кода пока нет
status-removed-written = { $type } убран из приложения и из кода
status-restored = { $type } снова в приложении
row-removed = убран из приложения, из кода пока нет
row-removed-written = убран из приложения и из кода
row-removed-tag = убран
help-copy = Копирование, вставка и стили
help-copy-1 = Щёлкните правой кнопкой по элементу в Слоях или нажмите клавишу меню или Shift+F10: Копировать (Ctrl+C), Вставить (Ctrl+V), Дублировать (Ctrl+D), Копировать стиль (Ctrl+Shift+C), Вставить стиль (Ctrl+Shift+V) и Удалить (Del).
help-copy-2 = Вставить кладёт копию внутрь выбранной разметки или сразу после выбранного элемента. Вставить стиль задаёт выбранному элементу скопированные цвета, шрифты, обводки, углы, тени и отступы; Записать все добавляет их в код.
help-copy-3 = Вставленный или удалённый элемент меняет только приложение, с точкой в его строке Слоёв: Записать меняет и код, Отменить возвращает как было.

## Hub: writes while other live changes wait

hub-not-in-code-yet = { $type } ещё нет в коде: сначала запишите его в код
hub-parent-not-in-code = его родителя { $type } ещё нет в коде: сначала запишите его
hub-in-code-already = { $type } уже есть в коде

## Copy with what is inside

status-copied-inside = { $type } скопирован, внутри { $count ->
    [one] { $count } элемент
    [few] { $count } элемента
   *[many] { $count } элементов
}
status-copy-gone = Не скопировано: приложение больше не показывает этот элемент

## Live changes kept across a rebuild

hub-lost-prop = { $type } { $prop }: этого элемента больше нет в коде
hub-lost-insert = { $type }, добавленный здесь: его места больше нет в коде
hub-lost-remove = { $type }, убранный здесь: его больше нет в коде
status-not-kept = Не удалось сохранить изменений: { $count }. { $what }
status-moved = { $type } перемещён в приложении, в коде пока нет
status-not-moved = { $type } не перемещён
step-move = Переместить { $type }
row-moved = перемещён здесь, в коде пока нет
row-moved-written = перемещён здесь, в коде
hub-lost-move = { $type }, перемещённый здесь: его места больше нет в коде
help-move = Перемещение элементов
help-move-1 = Перетащите строку элемента в Слоях: строки раздвигаются там, куда он попадёт. Между двумя строками он встаёт туда, на макет — внутрь, последним. У верхнего или нижнего края список прокручивается; Esc отменяет.
help-move-2 = Alt+Вверх и Alt+Вниз перемещают выбранный элемент на одно место среди соседних.
help-move-3 = Перемещённый элемент меняет только приложение, с точкой в его строке Слоёв: Записать перемещает его и в коде, Отменить возвращает на место.
pick-label-keys = Выбрать элемент в приложении (Ctrl+P)
section-code = Код
code-hint = Как изменения, сделанные здесь, попадают в код приложения.
write-mode = Записывать изменения в код
write-mode-hint = Вручную: вы записываете их сами, кнопкой «Записать» строки или «Записать все». Авто: они записываются, когда нужен код, перед перезапуском и запуском. Всегда: каждое законченное изменение записывается сразу.
write-mode-manual = Вручную
write-mode-auto = Авто
write-mode-always = Всегда
changes-not-in-code = { $count ->
    [one] { $count } изменение не в коде
    [few] { $count } изменения не в коде
   *[many] { $count } изменений не в коде
}
write-changes-label = Записать все изменения в код ({ $count })
step-write-changes = Записать все изменения
unwritten-title = { $count ->
    [one] { $count } изменение ещё не в коде.
    [few] { $count } изменения ещё не в коде.
   *[many] { $count } изменений ещё не в коде.
}
unwritten-write-restart = Записать и перезапустить
unwritten-skip-restart = Перезапустить без записи
unwritten-write-run = Записать и запустить
unwritten-skip-run = Запустить без записи
unwritten-write-close = Записать и закрыть
unwritten-skip-close = Закрыть без записи
unwritten-cancel = Отмена
