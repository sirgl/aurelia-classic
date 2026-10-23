# Экран настройки новой игры модуля Civ V: список наций, карточка нации, форма мира, экран загрузки.

## Frame
classic.ui.match-setup.roster-heading = НАЦИИ
classic.ui.match-setup.tab-world = Мир
classic.ui.match-setup.tab-rules = Правила и зерно
classic.ui.match-setup.footer-hint = Те же настройки — тот же мир. Щёлкните правой кнопкой по нации, чтобы открыть её страницу в энциклопедии.
classic.ui.match-setup.start = Начать игру
classic.ui.match-setup.world-title = Ваш мир
classic.ui.match-setup.world-hint = Выберите нацию, затем настройте её мир.
classic.ui.match-setup.rules-hint = Сохраните зерно, чтобы воссоздать этот мир. Недоступные скорости и условия победы объясняют причину в подсказке.
classic.ui.match-setup.randomize = Случайное
classic.ui.match-setup.more = Больше
classic.ui.match-setup.fewer = Меньше

## Section headings
classic.ui.match-setup.section-map-type = ТИП КАРТЫ
classic.ui.match-setup.section-map-size = РАЗМЕР КАРТЫ
classic.ui.match-setup.section-difficulty = СЛОЖНОСТЬ
classic.ui.match-setup.section-speed = СКОРОСТЬ ИГРЫ
classic.ui.match-setup.section-rivals = СОПЕРНИКИ
classic.ui.match-setup.section-rival-nations = НАЦИИ-СОПЕРНИЦЫ
classic.ui.match-setup.section-city-states = ГОРОДА-ГОСУДАРСТВА
classic.ui.match-setup.section-sea-level = УРОВЕНЬ МОРЯ
classic.ui.match-setup.section-victories = УСЛОВИЯ ПОБЕДЫ
classic.ui.match-setup.section-seed = ЗЕРНО МИРА

## Map types
classic.ui.match-setup.map-continents = Материки
classic.ui.match-setup.map-pangaea = Пангея
classic.ui.match-setup.map-archipelago = Архипелаг
classic.ui.match-setup.map-fractal = Фрактал
classic.ui.match-setup.map-small-continents = Малые мат.
classic.ui.match-setup.map-hint-continents = Два крупных материка, разделённых океаном.
classic.ui.match-setup.map-hint-pangaea = Один суперконтинент: любая нация может дойти до любой другой.
classic.ui.match-setup.map-hint-archipelago = Множество островов. Сухопутные отряды садятся на корабли после изучения технологий «{ $coast }» (побережье) и «{ $deep }» (глубокая вода).
classic.ui.match-setup.map-hint-fractal = Неровная суша и вода, созданные одним лишь шумом.
classic.ui.match-setup.map-hint-small-continents = Несколько материков среднего размера.

## Map sizes, sea level, speeds
classic.ui.match-setup.size-duel = Дуэль
classic.ui.match-setup.size-tiny = Крошечная
classic.ui.match-setup.size-small = Малая
classic.ui.match-setup.size-standard = Обычная
classic.ui.match-setup.size-large = Большая
classic.ui.match-setup.size-huge = Огромная
classic.ui.match-setup.size-custom = Особая
classic.ui.match-setup.sea-low = Низкий
classic.ui.match-setup.sea-medium = Средний
classic.ui.match-setup.sea-high = Высокий
classic.ui.match-setup.speed-quick = Быстрая
classic.ui.match-setup.speed-standard = Обычная
classic.ui.match-setup.speed-epic = Эпическая
classic.ui.match-setup.speed-marathon = Марафон
classic.ui.match-setup.speed-unavailable = Ядро правил пока не масштабирует скорость игры: доступна только обычная.
classic.ui.match-setup.city-states-hint = Города-государства на карте: подарки, задания, союзы и их голоса во Всемирном конгрессе.

## Victory conditions
classic.ui.match-setup.victory-domination = Господство
classic.ui.match-setup.victory-science = Наука
classic.ui.match-setup.victory-culture = Культура
classic.ui.match-setup.victory-diplomacy = Дипломатия
classic.ui.match-setup.victory-time = Время
classic.ui.match-setup.victory-domination-tip = Удержать все исходные столицы. В этой версии всегда включена.
classic.ui.match-setup.victory-science-tip = Постройте { $project } и все части космического корабля.
classic.ui.match-setup.victory-culture-tip = Добейтесь влияния на каждую нацию.
classic.ui.match-setup.victory-diplomacy-tip = Выиграйте голосование за Мирового лидера в ООН.
classic.ui.match-setup.victory-time-tip = Наибольший счёт на последнем ходу.

## Seed
classic.ui.match-setup.seed-tip = Число от 0 до 18446744073709551615. Сохраните его, чтобы воссоздать этот мир.

## Nation card
classic.ui.match-setup.ability-heading = Особая способность
classic.ui.match-setup.ability-inactive = { $line } (пока не действует)
classic.ui.match-setup.unique-unit = УНИКАЛЬНЫЙ ОТРЯД: { $name }
classic.ui.match-setup.unique-unit-replaces = УНИКАЛЬНЫЙ ОТРЯД: { $name } · заменяет: { $old }
classic.ui.match-setup.unique-building = УНИКАЛЬНОЕ ЗДАНИЕ: { $name }
classic.ui.match-setup.unique-building-replaces = УНИКАЛЬНОЕ ЗДАНИЕ: { $name } · заменяет: { $old }
classic.ui.match-setup.unique-improvement = УНИКАЛЬНОЕ УЛУЧШЕНИЕ: { $name }
classic.ui.match-setup.unique-improvement-replaces = УНИКАЛЬНОЕ УЛУЧШЕНИЕ: { $name } · заменяет: { $old }

## Loading screen
classic.ui.match-setup.summary = { $map } · { $size } ({ $width } × { $height }) · { $players ->
    [one] { $players } нация
    [few] { $players } нации
    [many] { $players } наций
   *[other] { $players } нации
} · { $difficulty }
classic.ui.match-setup.creating = Создаём мир{ $dots }

## Errors
classic.ui.match-setup.error-nation = Выберите доступную нацию
classic.ui.match-setup.error-world-settings = Некорректные настройки мира
classic.ui.match-setup.error-difficulty = Выберите уровень сложности
classic.ui.match-setup.error-speed = Эта скорость игры пока недоступна
classic.ui.match-setup.error-create = Не удалось создать этот мир: { $error }
classic.ui.match-setup.error-seed = Введите целое число от 0 до 18446744073709551615 в качестве зерна
