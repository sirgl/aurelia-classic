# Translation of classic-pedia-describe.ftl into ru. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.pedia.describe.join = { $a }, { $b }
classic.pedia.describe.join-or = { $a } или { $b }
classic.pedia.describe.yield-amount = { $amount } { $yield }
classic.pedia.describe.yield-output = выработки
classic.pedia.describe.yield-food = еды
classic.pedia.describe.yield-production = производства
classic.pedia.describe.yield-gold = золота
classic.pedia.describe.yield-science = науки
classic.pedia.describe.yield-culture = культуры
classic.pedia.describe.yield-faith = веры
classic.pedia.describe.with-trigger = { $trigger }: { $text }
classic.pedia.describe.with-selector = { $text } { $selector }
classic.pedia.describe.with-condition = { $text } { $condition }
classic.pedia.describe.with-second-condition = { $text } и { $condition }
classic.pedia.describe.with-duration = { $text } на { $turns ->
    [one] { $turns } ход
    [few] { $turns } хода
    [many] { $turns } ходов
   *[other] { $turns } хода
}
classic.pedia.describe.trigger-on-built = при постройке
classic.pedia.describe.trigger-on-turn-start = каждый ход
classic.pedia.describe.trigger-on-technology-unlocked = при изучении
classic.pedia.describe.trigger-on-city-founded = при основании города
classic.pedia.describe.trigger-on-unit-killed = при убийстве врага
classic.pedia.describe.trigger-on-unit-trained = при обучении отряда здесь
classic.pedia.describe.trigger-on-city-conquered = при захвате города
classic.pedia.describe.trigger-on-era-entered = при вступлении в новую эпоху
classic.pedia.describe.selector-source-city = в этом городе
classic.pedia.describe.selector-owner-cities = в каждом городе
classic.pedia.describe.selector-event-city = в том городе
classic.pedia.describe.selector-owner-units = для каждого отряда
classic.pedia.describe.selector-event-unit = для того отряда
classic.pedia.describe.selector-source-city-tiles = на клетках, которые обрабатывает этот город
classic.pedia.describe.selector-owner-city-tiles = на обрабатываемых клетках
classic.pedia.describe.selector-capital = в столице
classic.pedia.describe.condition-has-technology = при технологии «{ $name }»
classic.pedia.describe.condition-lacks-technology = пока не изучена технология «{ $name }»
classic.pedia.describe.condition-min-population = при населении от { $n ->
    [one] { $n } жителя
    [few] { $n } жителей
    [many] { $n } жителей
   *[other] { $n } жителя
}
classic.pedia.describe.condition-has-building = при наличии здания «{ $name }»
classic.pedia.describe.condition-on-tile = на клетке «{ $name }»
classic.pedia.describe.condition-terrain-until = на клетке «{ $terrain }», пока не изучена технология «{ $technology }»
classic.pedia.describe.condition-against-role = против отрядов { $role }
classic.pedia.describe.condition-against-class = против отрядов, подобных «{ $name }»
classic.pedia.describe.condition-for-units = для отрядов { $kind }
classic.pedia.describe.condition-near-city = в пределах { $n ->
    [one] { $n } клетки
    [few] { $n } клеток
    [many] { $n } клеток
   *[other] { $n } клетки
} от города
classic.pedia.describe.condition-resource-class = на ресурсах класса «{ $class }»
classic.pedia.describe.condition-unimproved-feature = на клетке «{ $name }» без улучшения
classic.pedia.describe.condition-adjacent-unimproved-feature = при { $n } и более клетках «{ $name }» без улучшения рядом с городом
classic.pedia.describe.condition-producing = при производстве { $what }
classic.pedia.describe.condition-garrisoned = при наличии гарнизона
classic.pedia.describe.condition-coastal = в прибрежном городе
classic.pedia.describe.condition-learned-technology = при изучении технологии «{ $name }»
classic.pedia.describe.condition-on-river = если город стоит на реке
classic.pedia.describe.condition-at-peace = пока вы в мире со всеми нациями
classic.pedia.describe.condition-connected = если город соединён со столицей
classic.pedia.describe.condition-near-improvement = в пределах { $n ->
    [one] { $n } клетки
    [few] { $n } клеток
    [many] { $n } клеток
   *[other] { $n } клетки
} от улучшения «{ $name }»
classic.pedia.describe.plot-river = на клетках у реки
classic.pedia.describe.plot-sea = на морских клетках
classic.pedia.describe.plot-lake = на клетках озера
classic.pedia.describe.plot-sea-resource = на морских ресурсах
classic.pedia.describe.plot-natural-wonder = на природных чудесах
classic.pedia.describe.production-units = отрядов
classic.pedia.describe.production-kind-units = отрядов { $kind }
classic.pedia.describe.production-a = «{ $name }»
classic.pedia.describe.production-military-units = боевых отрядов
classic.pedia.describe.production-spaceship-parts = деталей космического корабля
classic.pedia.describe.production-buildings = зданий
classic.pedia.describe.production-world-wonders = чудес света
classic.pedia.describe.production-world-wonders-before = чудес света до эпохи «{ $era }»
classic.pedia.describe.production-built-in-capital = зданий, которые уже есть в столице
classic.pedia.describe.role-great-person = великих людей
classic.pedia.describe.role-religious = религиозного назначения
classic.pedia.describe.role-civilian = мирного назначения
classic.pedia.describe.role-spaceship-part = деталей космического корабля
classic.pedia.describe.role-recon = разведки
classic.pedia.describe.role-melee = ближнего боя
classic.pedia.describe.role-archer = лучников
classic.pedia.describe.role-mounted = конницы
classic.pedia.describe.role-siege = осадного типа
classic.pedia.describe.role-gun = огнестрельного оружия
classic.pedia.describe.role-armor = бронетехники
classic.pedia.describe.role-naval-melee = морского ближнего боя
classic.pedia.describe.role-naval-ranged = морского дальнего боя
classic.pedia.describe.role-submarine = подводных лодок
classic.pedia.describe.role-carrier = авианосцев
classic.pedia.describe.role-fighter = истребителей
classic.pedia.describe.role-bomber = бомбардировщиков
classic.pedia.describe.role-helicopter = вертолётов
classic.pedia.describe.role-missile = ракет
classic.pedia.describe.domain-land = наземного типа
classic.pedia.describe.domain-sea = морского типа
classic.pedia.describe.domain-air = воздушного типа
classic.pedia.describe.resource-class-bonus = бонусный
classic.pedia.describe.resource-class-strategic = стратегический
classic.pedia.describe.resource-class-luxury = предмет роскоши
classic.pedia.describe.resource-class-artifact = артефакт
classic.pedia.describe.modifier-add-yield = { $amount } { $yield }
classic.pedia.describe.modifier-add-yield-percent = { $percent }% { $yield }
classic.pedia.describe.modifier-multiply-yield = ×{ $factor } { $yield }
classic.pedia.describe.modifier-growth = { $percent }% к росту
classic.pedia.describe.modifier-cost = { $percent }% { $target }
classic.pedia.describe.modifier-combat = { $percent }% боевой силы
classic.pedia.describe.modifier-combat-in = { $text } { $context }
classic.pedia.describe.modifier-movement = { $amount } движения
classic.pedia.describe.modifier-unlocks = открывает «{ $name }»
classic.pedia.describe.modifier-free-unit = бесплатно: { $count } × «{ $name }»
classic.pedia.describe.modifier-free-building = бесплатное здание «{ $name }»
classic.pedia.describe.modifier-promotion = повышение «{ $name }»
classic.pedia.describe.modifier-free-building-in-cities = { $count ->
    [one] бесплатное здание «{ $name }» в первом городе
    [few] бесплатное здание «{ $name }» в первых { $count } городах
    [many] бесплатное здание «{ $name }» в первых { $count } городах
   *[other] бесплатное здание «{ $name }» в первых { $count } городах
}
classic.pedia.describe.modifier-yield-per-count = { $amount } { $yield } { $per }
classic.pedia.describe.modifier-yield-percent-per-count = { $percent }% { $yield } { $per }
classic.pedia.describe.modifier-yield-from-kill = { $yield } за каждого уничтоженного вражеского отряда, больше за более сильные отряды
classic.pedia.describe.modifier-forbid-unit = нельзя обучать и покупать отряды «{ $name }»
classic.pedia.describe.modifier-forbid-annexing = нельзя аннексировать города
classic.pedia.describe.modifier-post-combat-promotion = может получить повышение «{ $name }» после первого боя
classic.pedia.describe.modifier-passive-promotion = подходящие отряды получают эффекты повышения «{ $name }»
classic.pedia.describe.modifier-experience = +{ $amount } опыта
classic.pedia.describe.per-count-one = за { $unit }
classic.pedia.describe.per-count-many = за каждые { $every } { $unit }
classic.pedia.describe.per-count-capped = { $per } (максимум { $max })
classic.pedia.describe.count-population = { $n ->
    [one] жителя
    [few] жителя
    [many] жителей
   *[other] жителя
}
classic.pedia.describe.count-world-wonders = { $n ->
    [one] чудо света
    [few] чуда света
    [many] чудес света
   *[other] чуда света
}
classic.pedia.describe.count-connected-cities = { $n ->
    [one] город, соединённый со столицей
    [few] города, соединённых со столицей
    [many] городов, соединённых со столицей
   *[other] города, соединённых со столицей
}
classic.pedia.describe.per-population-step = { $n ->
    [one] +1 { $yield } за жителя
    [few] +1 { $yield } за каждые { $n } жителя
    [many] +1 { $yield } за каждые { $n } жителей
   *[other] +1 { $yield } за каждые { $n } жителя
}
classic.pedia.describe.per-population = { $amount } { $yield } за жителя
classic.pedia.describe.cost-units = стоимости производства отрядов
classic.pedia.describe.cost-buildings = стоимости производства зданий
classic.pedia.describe.cost-technologies = стоимости технологий
classic.pedia.describe.cost-tile-purchase = стоимости покупки клеток
classic.pedia.describe.cost-hurry = стоимости покупки
classic.pedia.describe.cost-border-growth = культуры, нужной для роста границ
classic.pedia.describe.cost-border-growth-exponent = роста стоимости расширения границ
classic.pedia.describe.cost-unit-upgrade = стоимости улучшения отрядов
classic.pedia.describe.cost-unit-purchase = золотой стоимости покупки отрядов
classic.pedia.describe.cost-building-purchase = золотой стоимости покупки зданий
classic.pedia.describe.context-attack = при атаке
classic.pedia.describe.context-defense = при обороне
classic.pedia.describe.context-vs-city = против городов
classic.pedia.describe.context-attack-open = при атаке на открытой местности
classic.pedia.describe.context-attack-rough = при атаке на пересечённой местности
classic.pedia.describe.context-defense-open = при обороне на открытой местности
classic.pedia.describe.context-defense-rough = при обороне на пересечённой местности
classic.pedia.describe.context-ranged-attack-open = при дальней атаке на открытой местности
classic.pedia.describe.context-ranged-attack-rough = при дальней атаке на пересечённой местности
classic.pedia.describe.context-defense-vs-ranged = при обороне от дальних атак
classic.pedia.describe.context-ranged-attack = при дальних атаках
classic.pedia.describe.context-vs-wounded = против раненых отрядов
classic.pedia.describe.context-vs-fortified = против укреплённых отрядов
classic.pedia.describe.context-friendly-land = на своей территории
classic.pedia.describe.context-outside-friendly-land = вне своей территории
classic.pedia.describe.context-attack-friendly-land = при атаке на своей территории
classic.pedia.describe.context-flanking = за каждый отряд, зашедший во фланг
classic.pedia.describe.context-adjacent-friendly = рядом с дружественным отрядом
classic.pedia.describe.context-near-capital = рядом со столицей
classic.pedia.describe.context-embarked-defense = при обороне на корабле
classic.pedia.describe.context-interception = при перехвате
classic.pedia.describe.context-air-sweep = при воздушной зачистке
classic.pedia.describe.city-combat-defense = { $amount } к боевой силе города от укреплений
classic.pedia.describe.city-combat-hit-points = { $amount } к здоровью города
classic.pedia.describe.city-combat-defense-percent = { $amount }% к защите от зданий
classic.pedia.describe.city-combat-garrison-ranged = { $amount }% к силе дальней атаки города при наличии гарнизона
classic.pedia.describe.city-combat-nuke-damage = { $amount }% к урону от ядерных атак
classic.pedia.describe.city-combat-air-capacity = { $amount } к вместимости самолётов
classic.pedia.describe.city-combat-ranged = { $amount }% к силе дальней атаки города
classic.pedia.describe.espionage-enemy-steal = { $amount }% к скорости кражи технологий вражескими шпионами
classic.pedia.describe.espionage-steal = { $amount }% к скорости кражи технологий вашими шпионами
classic.pedia.describe.espionage-catch = { $amount }% к шансу поймать вражеских шпионов
classic.pedia.describe.espionage-rig-election = { $amount }% голосов при фальсификации выборов в городах-государствах
classic.pedia.describe.espionage-extra-spies = { $amount } к числу шпионов вместе с первым шпионом
classic.pedia.describe.espionage-starting-rank = { $amount } к начальному рангу новых шпионов
classic.pedia.describe.espionage-spy-pressure = шпионы усиливают религиозное давление в своём городе (×{ $amount })
classic.pedia.describe.league-delegates = { $amount } к числу делегатов во Всемирном конгрессе
classic.pedia.describe.league-delegates-per-diplomat = { $amount } к числу делегатов за каждого дипломата в чужой столице
classic.pedia.describe.stat-resource-copies = даёт ресурс «{ $resource }»: { $amount }
classic.pedia.describe.stat-resource-quantity = { $amount }% к добыче ресурса «{ $resource }» на ваших клетках
classic.pedia.describe.stat-strategic-quantity = { $amount }% к добыче стратегических ресурсов на ваших клетках
classic.pedia.describe.stat-extra-luxury-copies = { $amount } к количеству каждого предмета роскоши вокруг города
classic.pedia.describe.stat-great-people-rate = { $amount }% к скорости появления великих людей
classic.pedia.describe.stat-great-person-rate = { $amount }% к очкам: { $kind }
classic.pedia.describe.stat-food-kept = { $amount }% еды сохраняется при росте
classic.pedia.describe.stat-specialist-yield = { $amount } { $yield } за специалиста
classic.pedia.describe.stat-happiness-per-city = { $amount } счастья за город
classic.pedia.describe.stat-happiness-per-luxury = { $amount } счастья за предмет роскоши
classic.pedia.describe.stat-natural-wonder-happiness = { $amount }% счастья от природных чудес
classic.pedia.describe.stat-city-count-unhappiness = { $amount }% недовольства от числа городов
classic.pedia.describe.stat-population-unhappiness = { $amount }% недовольства от населения
classic.pedia.describe.stat-golden-age-length = { $amount }% к длительности золотого века
classic.pedia.describe.stat-connection-gold = { $amount }% золота от соединения городов
classic.pedia.describe.stat-research-agreement = { $amount }% науки от соглашений об исследованиях
classic.pedia.describe.stat-great-scientist-science = { $amount }% науки от великих учёных
classic.pedia.describe.stat-great-people-per-friend = { $amount }% к скорости появления великих людей за каждое заявление о дружбе
classic.pedia.describe.stat-free-maintenance-units = { $amount } к числу отрядов, не требующих золота на содержание
classic.pedia.describe.stat-barbarian-conversion = шанс { $amount }%, что побеждённый в своём лагере сухопутный отряд варваров перейдёт на вашу сторону и принесёт 25 золота
classic.pedia.describe.stat-plunder-gold = { $amount }% золота от лагерей варваров и захваченных городов
classic.pedia.describe.stat-captured-gold = { $amount }% золота тому, кто захватит этот город
classic.pedia.describe.stat-worker-speed = { $amount }% к скорости работников
classic.pedia.describe.stat-route-maintenance = { $amount }% к содержанию дорог и железных дорог
classic.pedia.describe.stat-any-specialist-yield = { $amount } { $yield } в городе со специалистом
classic.pedia.describe.rule-garrison-free-maintenance = отряды в гарнизонах городов не требуют содержания
classic.pedia.describe.rule-free-hills-route-maintenance = дороги и железные дороги на холмах не требуют содержания
classic.pedia.describe.rule-spaceship-part-purchase = детали космического корабля можно покупать за золото
classic.pedia.describe.rule-spaceship-part-hurry = великие инженеры могут ускорять производство деталей космического корабля
classic.pedia.describe.rule-see-barbarian-camps = новые лагеря варваров видны на исследованной земле
classic.pedia.describe.society-specialist-food = { $amount }% еды, которую едят специалисты
classic.pedia.describe.society-specialist-unhappiness = { $amount }% недовольства от специалистов
classic.pedia.describe.society-no-occupied-unhappiness = нет дополнительного недовольства в оккупированном городе
classic.pedia.describe.society-wonder-culture = { $amount }% культуры от чудес света
classic.pedia.describe.society-happiness-to-culture = избыток счастья превращается в культуру
classic.pedia.describe.society-science-while-happy = { $amount }% науки, пока империя счастлива
classic.pedia.describe.society-luxury-retention = сохраняет { $amount }% счастья от предмета роскоши, когда его последний экземпляр передан по сделке
classic.pedia.describe.society-raze-speed = { $amount }% к скорости разрушения городов
classic.pedia.describe.society-founding-territory = новые города захватывают дополнительные клетки: { $amount }
classic.pedia.describe.society-capital-science-boost = научные здания, построенные в столице, ускоряют исследования
classic.pedia.describe.society-unique-luxury-cities = первые города на других материках получают уникальный предмет роскоши (число городов: { $amount })
classic.pedia.describe.society-unique-luxury-quantity = количество каждого уникального предмета роскоши: { $amount }
classic.pedia.describe.society-unique-luxury-new-landmass = уникальные предметы роскоши только на материках вдали от столицы
classic.pedia.describe.trade-routes = { $amount } к числу торговых путей
classic.pedia.describe.trade-range-tiles = { $amount } к дальности торговых путей { $domain } (в клетках)
classic.pedia.describe.trade-range-percent = { $amount }% к дальности торговых путей { $domain }
classic.pedia.describe.trade-route-gold = { $amount } золота от торговых путей { $domain }
classic.pedia.describe.trade-target-gold = { $amount } золота за торговые пути в этот город
classic.pedia.describe.trade-recipient-gold = { $amount } золота этому городу от иностранных торговых путей
classic.pedia.describe.trade-internal-percent = { $amount }% еды и производства от внутренних торговых путей
classic.pedia.describe.trade-routes-percent = { $amount }% к числу торговых путей
classic.pedia.describe.trade-resource-gold-percent = { $amount }% золота за разнообразие ресурсов на торговых путях
classic.pedia.describe.trade-incoming-route-gold = { $amount } золота тому, кто отправит вам торговый путь
classic.pedia.describe.trade-city-state-route-gold = { $amount } золота от торговых путей в города-государства
classic.pedia.describe.trade-shared-ideology-route-gold = { $amount } золота от торговых путей к нациям вашей идеологии
classic.pedia.describe.trade-religious-pressure = { $amount }% религиозного давления по торговым путям
classic.pedia.describe.placement-land = Размещается на суше
classic.pedia.describe.placement-water = Поднимается из моря
classic.pedia.describe.placement-reef = Размещается на открытой воде у берега
classic.pedia.describe.placement-peninsula-tip = Размещается на оконечности полуострова
classic.pedia.describe.placement-of-terrains = { $text }, местность: { $terrains }
classic.pedia.describe.placement-away-from-coast = { $text }, вдали от берега
classic.pedia.describe.placement-off-largest-landmass = { $text }, вне крупнейшего материка
classic.pedia.describe.placement-sentence = { $text }.
classic.pedia.describe.adjacent-hills = холмы
classic.pedia.describe.adjacent-mountains = горы
classic.pedia.describe.adjacent-hills-or-mountains = холмы или горы
classic.pedia.describe.adjacent-land = клетки суши
classic.pedia.describe.adjacent-water = клетки воды
classic.pedia.describe.adjacent-at-least = Рядом не менее { $n }: { $what }.
classic.pedia.describe.adjacent-none = Рядом нет: { $what }.
classic.pedia.describe.adjacent-at-most = Рядом не более { $n }: { $what }.
classic.pedia.describe.shape-mountain = Клетка становится горой.
classic.pedia.describe.shape-flat = Клетка становится ровной землёй.
classic.pedia.describe.surrounded-by-sea = Её окружает море.
classic.pedia.describe.grant-free-technologies = { $n ->
    [one] бесплатная технология на ваш выбор
    [few] { $n } бесплатные технологии на ваш выбор
    [many] { $n } бесплатных технологий на ваш выбор
   *[other] { $n } бесплатной технологии на ваш выбор
}
classic.pedia.describe.grant-conquest-technology = бесплатная технология, известная прежнему владельцу города (один раз на город)
classic.pedia.describe.grant-military-unit-copies = копия каждого вашего сухопутного боевого отряда
classic.pedia.describe.grant-population = { $n ->
    [one] { $amount } житель
    [few] { $amount } жителя
    [many] { $amount } жителей
   *[other] { $amount } жителя
}
classic.pedia.describe.unit-stat-sight = { $amount } к обзору (в клетках)
classic.pedia.describe.unit-stat-embarked-sight = { $amount } к обзору на корабле (в клетках)
classic.pedia.describe.unit-stat-range = { $amount } к дальности дальней атаки (в клетках)
classic.pedia.describe.unit-stat-attacks-per-turn = { $amount } к числу атак за ход
classic.pedia.describe.unit-stat-cargo = { $amount } к вместимости самолётов и ракет
classic.pedia.describe.unit-stat-intercept-chance = { $amount } п. п. к шансу перехвата
classic.pedia.describe.unit-stat-interceptions-per-turn = { $amount } к числу перехватов за ход
classic.pedia.describe.unit-stat-evasion = { $amount } п. п. к шансу уклониться от перехвата
classic.pedia.describe.unit-stat-interception-damage = { $amount }% к урону от перехватов
classic.pedia.describe.unit-stat-paradrop-range = { $amount } к дальности десантирования (в клетках)
classic.pedia.describe.unit-stat-naval-moves = { $amount } к движению на море
classic.pedia.describe.unit-stat-experience = { $amount }% к получаемому опыту
classic.pedia.describe.unit-stat-withdraw-chance = { $amount } п. п. к шансу отступить до ближнего боя
classic.pedia.describe.unit-stat-city-attack-plunder = приносит золото при атаке городов, тем больше, чем больше урон
classic.pedia.describe.unit-stat-heal-enemy-land = { $amount } здоровья восстанавливается за ход на вражеской территории
classic.pedia.describe.unit-stat-heal-neutral-land = { $amount } здоровья восстанавливается за ход на нейтральной территории
classic.pedia.describe.unit-stat-heal-friendly-land = { $amount } здоровья восстанавливается за ход на своей земле
classic.pedia.describe.unit-stat-heal-adjacent = соседние дружественные отряды восстанавливают { $amount } здоровья за ход
classic.pedia.describe.unit-stat-heal-on-kill = { $amount } здоровья восстанавливается после уничтожения вражеского отряда
classic.pedia.describe.unit-stat-enemy-aura = { $amount }% к боевой силе ближайших вражеских отрядов
classic.pedia.describe.unit-stat-aura-range = { $amount } к радиусу эффекта ближайших врагов (в клетках)
classic.pedia.describe.unit-stat-adjacent-damage = { $amount } урона соседним вражеским отрядам после атаки
classic.pedia.describe.unit-stat-capital-defense-falloff = меняет скорость, с которой бонус обороны столицы ослабевает с расстоянием
classic.pedia.describe.unit-stat-embarked-moves = { $amount } к движению на корабле
classic.pedia.describe.unit-stat-wounded-damage = { $amount } п. п. к модификатору урона по раненым отрядам
classic.pedia.describe.ability-found-religion = Может основать или усилить религию
classic.pedia.describe.ability-spread-religion = Может распространить свою религию в городе
classic.pedia.describe.ability-religion-spreads = Может распространять религию { $n ->
    [one] { $n } раз
    [few] { $n } раза
    [many] { $n } раз
   *[other] { $n } раза
}
classic.pedia.describe.ability-religious-strength = Религиозная сила: { $n }
classic.pedia.describe.ability-remove-heresy = Может изгнать другие религии из города
classic.pedia.describe.ability-prohibits-spread = Его присутствие блокирует распространение других религий
classic.pedia.describe.ability-requires-enhanced-religion = Можно купить, только когда ваша религия усилена
classic.pedia.describe.ability-one-shot-tourism = Концерт даёт туризм, который растёт вместе с вашим текущим туризмом
classic.pedia.describe.ability-one-shot-tourism-others = Другие нации получают { $n }% туризма, который даёт концерт
classic.pedia.describe.ability-culture-turns = Даёт культуру пропорционально вашей недавней выработке культуры
classic.pedia.describe.ability-excavates = Может раскапывать памятники древности
classic.pedia.describe.ability-food = Пока отряд обучается, избыток еды города прибавляется к производству
classic.pedia.describe.ability-golden-age-turns = Может начать золотой век с базовой длительностью в { $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
}
classic.pedia.describe.ability-beakers-turns = Даёт всплеск науки пропорционально вашей недавней выработке
classic.pedia.describe.ability-base-hurry = Может добавить производство к тому, что строит город
classic.pedia.describe.ability-hurry-multiplier = Чем больше город, тем больше производства он получает таким способом
classic.pedia.describe.ability-base-gold = Торговые миссии приносят золото
classic.pedia.describe.ability-gold-per-era = В поздние эпохи торговые миссии приносят больше
classic.pedia.describe.ability-no-maintenance = Не требует золота на содержание
classic.pedia.describe.ability-trade = Может устанавливать торговые пути
classic.pedia.describe.ability-faith-purchase-needs-belief = Можно купить за веру, только если верование открывает его эпоху
classic.pedia.describe.ability-mechanized = Машина, а не люди
classic.pedia.describe.ability-priority-low = Враги нападают на него в последнюю очередь
classic.pedia.describe.ability-priority-early = Враги нападают на него в первую очередь в начале игры
classic.pedia.describe.ability-priority-high = Враги нападают на него в первую очередь
classic.pedia.describe.ability-conscription = Значение призыва: { $n }
classic.pedia.describe.ability-great-person = Великий человек
classic.pedia.describe.ability-stealth = Самолёт-невидимка: не размещается на авианосце
classic.pedia.describe.ability-air-slots = Занимает в городе { $n ->
    [one] { $n } место для самолётов
    [few] { $n } места для самолётов
    [many] { $n } мест для самолётов
   *[other] { $n } места для самолётов
}
classic.pedia.describe.ability-work-rate = Скорость работы: { $n }
classic.pedia.describe.ability-move-after-purchase = Может двигаться в ход покупки
classic.pedia.describe.ability-no-bad-goodies = Древние руины никогда ему не вредят
classic.pedia.describe.ability-purchase-only = Можно только купить, обучить нельзя
classic.pedia.describe.ability-buy-city-state = Превращает город-государство, в котором стоит, в марионетку своего владельца
classic.pedia.describe.ability-found-abroad = Основывает города только вне материка столицы
classic.pedia.describe.ability-exotic-goods = Продаёт экзотические товары (груз: { $n }) у чужой земли
classic.pedia.describe.combat-ability-move-after-attacking = может двигаться после атаки
classic.pedia.describe.combat-ability-always-heal = может лечиться после движения или атаки
classic.pedia.describe.combat-ability-embarkation = может садиться на корабль, чтобы пересекать воду
classic.pedia.describe.combat-ability-embarked-all-water = может заходить на глубокую воду на корабле
classic.pedia.describe.combat-ability-move-impassable = может входить на обычно непроходимую местность
classic.pedia.describe.combat-ability-ranged-support-fire = наносит поддерживающий дальний удар перед ближним боем
classic.pedia.describe.combat-ability-hills-double-move = тратит вдвое меньше движения на холмах
classic.pedia.describe.combat-ability-double-move = тратит вдвое меньше обычного движения на указанной местности
classic.pedia.describe.combat-ability-impassable = не может входить на указанную местность
classic.pedia.describe.combat-ability-insta-heal = мгновенно лечит отряд вместо постоянного повышения
classic.pedia.describe.combat-ability-amphibious = нет штрафа к атаке при высадке с воды или переходе реки
classic.pedia.describe.combat-ability-river-crossing = нет штрафа к атаке при переходе реки
classic.pedia.describe.combat-ability-indirect-fire = дальним атакам не нужна прямая видимость
classic.pedia.describe.combat-ability-heal-outside-friendly = может лечиться в море вне своей территории
classic.pedia.describe.combat-ability-air-sweep = может проводить воздушную зачистку
classic.pedia.describe.combat-ability-rival-territory = может входить на территорию соперника без открытых границ
classic.pedia.describe.combat-ability-rough-terrain-ends-turn = вход на пересечённую местность завершает движение отряда
classic.pedia.describe.combat-ability-ignore-terrain-cost = игнорирует дополнительную стоимость движения по местности
classic.pedia.describe.combat-ability-flat-movement-cost = каждая клетка стоит одинаково движения, включая дороги
classic.pedia.describe.combat-ability-hovering = может двигаться над сушей и водой
classic.pedia.describe.combat-ability-no-capture = не может захватывать города и отряды
classic.pedia.describe.combat-ability-only-defensive = не может атаковать в ближнем бою
classic.pedia.describe.combat-ability-no-defensive-bonus = не получает бонусов защиты от местности и укрепления
classic.pedia.describe.combat-ability-must-set-up = перед дальней атакой нужно развернуться
classic.pedia.describe.combat-ability-free-pillage-moves = разграбление не тратит движение
classic.pedia.describe.combat-ability-city-attack-only = может атаковать только города
classic.pedia.describe.combat-ability-capture-defeated-enemy = может захватывать побеждённые вражеские отряды
classic.pedia.describe.combat-ability-heavy-charge = атакует сильнее отряды, которые не могут отступить
classic.pedia.describe.combat-ability-sapper = помогает соседним дружественным отрядам атаковать город рядом с этим отрядом
classic.pedia.describe.combat-ability-ignore-zone-of-control = игнорирует вражеские зоны контроля
classic.pedia.describe.combat-ability-embarked-to-land-flat-cost = высадка с воды стоит одно очко движения
classic.pedia.describe.combat-ability-hills-ignore-terrain-cost = игнорирует стоимость местности при входе на холмы
classic.pedia.describe.combat-ability-cross-mountains = может пересекать горы
classic.pedia.describe.combat-ability-road-in-friendly-land = на своей территории движется по указанной местности как по дороге
classic.pedia.describe.combat-ability-invisible = виден только рядом с вражескими отрядами и городами или отрядам, видящим подводные лодки
classic.pedia.describe.combat-ability-see-invisible = видит подводные лодки в пределах обзора
classic.pedia.describe.inert-amount = { $subject }: { $amount }
classic.pedia.describe.inert-refs = { $text } ({ $names })
classic.pedia.describe.inert-trigger = { $trigger }: { $text }
classic.pedia.describe.building-ideology-choice = Имея { $n } таких зданий, вы можете выбрать идеологию
classic.pedia.describe.building-free-great-work = Включает бесплатное великое произведение искусства
classic.pedia.describe.building-nullify-influence = Защищает вашу нацию от туристического бонуса, который другие нации получают от Интернета
classic.pedia.describe.building-missionary-extra-spreads = Купленные здесь миссионеры могут распространять религию ещё { $n ->
    [one] { $n } раз
    [few] { $n } раза
    [many] { $n } раз
   *[other] { $n } раза
}
classic.pedia.describe.building-religious-pressure = +{ $n }% религиозного давления от этого города
classic.pedia.describe.building-food-trade-routes = Этот город может отправлять торговые пути с едой в другие ваши города
classic.pedia.describe.building-production-trade-routes = Этот город может отправлять торговые пути с производством в другие ваши города
classic.pedia.describe.building-counter-espionage = Обеспечивает контрразведку в этом городе
classic.pedia.describe.building-affect-spies-now = Шпионы, уже собирающие здесь сведения, сразу ощущают его шпионские эффекты
classic.pedia.describe.building-border-obstacle = Вражеские сухопутные отряды тратят на 1 очко движения больше за клетку на вашей территории
classic.pedia.describe.building-airlift = Сухопутные отряды можно перебрасывать по воздуху в этот город и из него
classic.pedia.describe.building-allows-range-strike = Позволяет этому городу наносить дальние удары
classic.pedia.describe.unknown = Неизвестно
classic.pedia.describe-classic.trigger-on-adopt = при принятии
classic.pedia.describe-classic.trigger-wonder-found-first = при первом открытии природного чуда
classic.pedia.describe-classic.trigger-wonder-found = при открытии природного чуда, уже найденного другой нацией
classic.pedia.describe-classic.trigger-long-count = в конце каждого цикла Долгого счёта
classic.pedia.describe-classic.trigger-great-person-expended = при расходовании великого человека
classic.pedia.describe-classic.trigger-city-converted = когда город впервые принимает религию
classic.pedia.describe-classic.trigger-religion-spread = когда миссионер или пророк распространяет религию
classic.pedia.describe-classic.trigger-great-person-born = при появлении великого человека
classic.pedia.describe-classic.selector-following-cities = в городах, исповедующих религию
classic.pedia.describe-classic.selector-following-city-tiles = на клетках городов, исповедующих религию
classic.pedia.describe-classic.selector-holy-city = в святом городе
classic.pedia.describe-classic.selector-founder = для основателя
classic.pedia.describe-classic.selector-own-religion = для религии
classic.pedia.describe-classic.condition-barbarian = против варваров
classic.pedia.describe-classic.condition-city-state = против городов-государств
classic.pedia.describe-classic.condition-more-cities = против наций с большим числом городов
classic.pedia.describe-classic.condition-min-followers = при { $n ->
    [one] { $n } последователе
    [few] { $n } последователях
    [many] { $n } последователях
   *[other] { $n } последователях
} и более
classic.pedia.describe-classic.condition-near-own-following-city = рядом со своими городами, исповедующими религию
classic.pedia.describe-classic.condition-near-enemy-following-city = рядом с вражескими городами, исповедующими религию
classic.pedia.describe-classic.condition-golden-age = во время золотого века
classic.pedia.describe-classic.condition-great-general-earned = после появления великого полководца
classic.pedia.describe-classic.condition-great-person-is = если это { $kind }
classic.pedia.describe-classic.modifier-happiness = { $amount } счастья
classic.pedia.describe-classic.modifier-great-person-points = { $amount } к очкам: { $kind }
classic.pedia.describe-classic.modifier-free-policies = { $count ->
    [one] { $count } бесплатный социальный институт
    [few] { $count } бесплатных социальных института
    [many] { $count } бесплатных социальных институтов
   *[other] { $count } бесплатного социального института
}
classic.pedia.describe-classic.modifier-yield-per-following-city = { $foreign ->
    [true] { $amount } { $yield } за каждый иностранный город, исповедующий религию
   *[false] { $amount } { $yield } за каждый город, исповедующий религию
}
classic.pedia.describe-classic.modifier-yield-per-followers = { $foreign ->
    [true] +1 { $yield } за каждую группу из { $per } иностранных последователей
   *[false] +1 { $yield } за каждую группу из { $per } последователей
}
classic.pedia.describe-classic.modifier-happiness-per-following-city = { $amount } счастья за каждый город, исповедующий религию
classic.pedia.describe-classic.modifier-religion = { $percent }% { $stat }
classic.pedia.describe-classic.modifier-faith-building = «{ $name }» можно купить за веру
classic.pedia.describe-classic.modifier-faith-units = сухопутные отряды эпохи «{ $era }» можно покупать за веру
classic.pedia.describe-classic.modifier-reformation-belief = можно добавить верование Реформации
classic.pedia.describe-classic.modifier-tourism-toward = { $percent }% туризма для наций, { $when }
classic.pedia.describe-classic.modifier-happiness-per-count = { $amount } счастья { $per }
classic.pedia.describe-classic.modifier-grant-spies = { $count ->
    [one] бесплатный шпион
    [few] { $count } бесплатных шпиона
    [many] { $count } бесплатных шпионов
   *[other] { $count } бесплатного шпиона
}
classic.pedia.describe-classic.modifier-promote-spies = { $ranks ->
    [one] каждый шпион повышается на один ранг
    [few] каждый шпион повышается на { $ranks } ранга
    [many] каждый шпион повышается на { $ranks } рангов
   *[other] каждый шпион повышается на { $ranks } ранга
}
classic.pedia.describe-classic.modifier-extra-founding-belief = ещё одно верование при основании религии
classic.pedia.describe-classic.modifier-second-religion-pantheon = города также получают верование пантеона своей второй религии
classic.pedia.describe-classic.modifier-instant-tourism = +{ $amount } туризма со всеми известными нациями
classic.pedia.describe-classic.modifier-experience-per-great-work = обученные здесь отряды начинают с опытом { $amount } за каждое великое произведение в городе
classic.pedia.describe-classic.grant-free-great-people = { $n ->
    [one] бесплатный великий человек на ваш выбор
    [few] { $n } бесплатных великих человека на ваш выбор
    [many] { $n } бесплатных великих людей на ваш выбор
   *[other] { $n } бесплатного великого человека на ваш выбор
}
classic.pedia.describe-classic.grant-long-count-great-person = бесплатный великий человек ещё не выбиравшегося типа
classic.pedia.describe-classic.grant-golden-age = золотой век на { $turns ->
    [one] { $turns } ход
    [few] { $turns } хода
    [many] { $turns } ходов
   *[other] { $turns } хода
}
classic.pedia.describe-classic.unit-stat-general-points = { $amount }% к очкам великого полководца за бои
classic.pedia.describe-classic.unit-stat-stacked-general = { $amount }% к боевой силе отряда, стоящего на одной клетке с этим великим полководцем
classic.pedia.describe-classic.unit-stat-maintenance = { $amount }% к содержанию в золоте
classic.pedia.describe-classic.unit-stat-experience-needed = { $amount }% к опыту, нужному для повышения
classic.pedia.describe-classic.unit-stat-general-aura = { $amount }% к боевому бонусу великих полководцев и адмиралов
classic.pedia.describe-classic.unit-stat-mission-gold = { $amount }% золота от торговой миссии
classic.pedia.describe-classic.unit-stat-mission-influence = { $amount }% влияния от торговой миссии
classic.pedia.describe-classic.unit-stat-adjacent-damage-chance = шанс { $amount }% нанести урон соседним врагам после атаки
classic.pedia.describe-classic.unit-stat-rival-land-loss = теряет религиозную силу за каждый ход, закончившийся на чужой территории
classic.pedia.describe-classic.unit-stat-golden-age-on-kill = победы приносят очки золотого века, больше за более сильных противников
classic.pedia.describe-classic.ability-great-general = даёт ближайшим дружественным сухопутным отрядам боевой бонус великого полководца
classic.pedia.describe-classic.ability-post-combat-promotions = после первого боя заменяется случайным повышением
classic.pedia.describe-classic.ability-lends-movement = великий полководец, стоящий с ним на одной клетке в начале хода, двигается с его скоростью
classic.pedia.describe-classic.ability-ruins-reward = при входе в древние руины берёт лучшую из наград
classic.pedia.describe-classic.ability-exotic-cargo = везёт экзотические товары, которые можно один раз продать у чужой земли
classic.pedia.describe-classic.ability-great-admiral = даёт ближайшим дружественным морским отрядам боевой бонус великого адмирала
classic.pedia.describe-classic.ability-heal-not-barbarians = лечение после убийства не действует на варваров
classic.pedia.describe-classic.cost-policies = стоимости социальных институтов
classic.pedia.describe-classic.cost-policy-city-penalty = стоимости социальных институтов за каждый город
classic.pedia.describe-classic.cost-faith-purchase = стоимости покупки за веру
classic.pedia.describe-classic.count-policies = { $n ->
    [one] социальный институт
    [few] социальных института
    [many] социальных институтов
   *[other] социального института
}
classic.pedia.describe-classic.count-majority-followers = { $n ->
    [one] последователя
    [few] последователя
    [many] последователей
   *[other] последователя
}
classic.pedia.describe-classic.count-religions-present = { $n ->
    [one] присутствующую религию
    [few] присутствующие религии
    [many] присутствующих религий
   *[other] присутствующей религии
}
classic.pedia.describe-classic.count-peaceful-foreign-followers = { $n ->
    [one] последователя в иностранном городе, с которым нет войны
    [few] последователя в иностранных городах, с которыми нет войны
    [many] последователей в иностранных городах, с которыми нет войны
   *[other] последователя в иностранных городах, с которыми нет войны
}
classic.pedia.describe-classic.count-other-religion-followers = { $n ->
    [one] последователя других религий
    [few] последователя других религий
    [many] последователей других религий
   *[other] последователя других религий
}
classic.pedia.describe-classic.count-trade-partners = { $n ->
    [one] торгового партнёра
    [few] торгового партнёра
    [many] торговых партнёров
   *[other] торгового партнёра
}
classic.pedia.describe-classic.count-city-state-routes = { $n ->
    [one] торговый путь в город-государство
    [few] торговых пути в города-государства
    [many] торговых путей в города-государства
   *[other] торгового пути в города-государства
}
classic.pedia.describe-classic.league-happiness = +{ $n } счастья
classic.pedia.describe-classic.league-free-policies = { $n ->
    [one] { $n } бесплатный социальный институт
    [few] { $n } бесплатных социальных института
    [many] { $n } бесплатных социальных институтов
   *[other] { $n } бесплатного социального института
}
classic.pedia.describe-classic.league-golden-age-points = +{ $n } очков золотого века
classic.pedia.describe-classic.league-culture-boost = +100% культуры на { $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
}
classic.pedia.describe-classic.league-tourism-boost = +100% туризма на { $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
}
classic.pedia.describe-classic.league-city-state-influence = +{ $n } влияния у каждого известного города-государства
classic.pedia.describe-classic.league-research-turns = науки за { $n ->
    [one] последний ход
    [few] последние { $n } хода
    [many] последние { $n } ходов
   *[other] последние { $n } хода
}
classic.pedia.describe-classic.league-free-unit = бесплатный отряд «{ $name }» в столице
classic.pedia.describe-classic.league-building = «{ $name }» в столице
classic.pedia.describe-classic.city-state-resting-point = { $amount } к уровню влияния в покое у городов-государств
classic.pedia.describe-classic.city-state-decay = { $amount }% к убыванию влияния у городов-государств
classic.pedia.describe-classic.city-state-gold-gift = { $amount }% влияния от подарков золота городам-государствам
classic.pedia.describe-classic.city-state-bully-score = { $amount }% военной мощи при требовании дани
classic.pedia.describe-classic.city-state-unit-gift-influence = { $amount } влияния за каждый отряд, подаренный городу-государству
classic.pedia.describe-classic.city-state-friend-bonus = { $amount }% еды, культуры и веры от городов-государств, ставших друзьями
classic.pedia.describe-classic.city-state-friendship-rate = { $amount }% к скорости восстановления влияния у городов-государств; { $decay }% к убыванию влияния
classic.pedia.describe-classic.city-state-annex = может выкупать города-государства, остающиеся союзными пять ходов
classic.pedia.describe-classic.city-state-resting-point-religion = { $amount } к уровню влияния в покое у городов-государств, исповедующих религию
classic.pedia.describe-classic.city-state-afraid-influence = { $amount } влияния за ход у городов-государств, которые вас боятся
classic.pedia.describe-classic.city-state-trade-route-influence = { $amount } влияния за ход у городов-государств, с которыми у вас есть торговый путь
classic.pedia.describe-classic.city-state-unit-gift-rate = воинственные города-государства дарят отряды на { $amount }% быстрее, пока у вас общий враг
classic.pedia.describe-classic.city-state-ally-resources = союзные города-государства дают вдвое больше ресурсов, а их предметы роскоши — на 50% больше счастья
classic.pedia.describe-classic.city-state-ally-great-people = союзные города-государства время от времени дарят великого человека
classic.pedia.describe-classic.city-state-ally-science = четверть науки союзных городов-государств
classic.pedia.describe-classic.city-state-great-person-gift = { $amount } влияния за каждого великого человека, подаренного городу-государству
classic.pedia.describe-classic.city-state-mission-gold = { $amount }% золота от торговых миссий
classic.pedia.describe-classic.great-artist = великий художник
classic.pedia.describe-classic.great-engineer = великий инженер
classic.pedia.describe-classic.great-merchant = великий купец
classic.pedia.describe-classic.great-scientist = великий учёный
classic.pedia.describe-classic.great-writer = великий писатель
classic.pedia.describe-classic.great-musician = великий музыкант
classic.pedia.describe-classic.great-general = великий полководец
classic.pedia.describe-classic.great-admiral = великий адмирал
classic.pedia.describe-classic.great-prophet = великий пророк
classic.pedia.describe-classic.religion-pressure-out = религиозного давления
classic.pedia.describe-classic.religion-spread-distance = дальности распространения религии
classic.pedia.describe-classic.religion-missionary-strength = силы миссионеров
classic.pedia.describe-classic.religion-prophet-strength = силы великого пророка
classic.pedia.describe-classic.religion-prophet-cost = стоимости великого пророка в вере
classic.pedia.describe-classic.religion-missionary-cost = стоимости миссионера в вере
classic.pedia.describe-classic.religion-friendly-city-state-pressure = религиозного давления на дружественные города-государства
classic.pedia.describe-classic.religion-other-erosion = давления миссионера, отнимаемого у других религий
classic.pedia.describe-classic.religion-pressure-retention = давления, сохраняемого после удаления религии инквизиторами или пророками
classic.pedia.describe-classic.religion-converts-barbarians = миссионеры обращают соседние отряды варваров
classic.pedia.describe-classic.religion-faith-great-people = любого великого человека можно купить за веру начиная с Индустриальной эпохи
classic.pedia.describe-classic.tourism-flat = { $amount } туризма
classic.pedia.describe-classic.tourism-percent = { $amount }% туризма
classic.pedia.describe-classic.tourism-great-works = { $amount }% туризма от великих произведений
classic.pedia.describe-classic.tourism-landmarks = { $amount }% туризма от достопримечательностей и чудес
classic.pedia.describe-classic.tourism-per-faith-building = { $amount } туризма за каждое здание веры
classic.pedia.describe-classic.tourism-spread = { $amount }% к распространению туризма
classic.pedia.describe-classic.tourism-theming = { $amount }% к бонусам тематизации
classic.pedia.describe-classic.tourism-great-work-culture = { $amount } культуры за каждое великое произведение
classic.pedia.describe-classic.tourism-shared-religion = которые исповедуют вашу религию
classic.pedia.describe-classic.tourism-trade-route = с которыми у вас есть торговый путь
classic.pedia.describe-classic.tourism-open-borders = с которыми у вас открытые границы
classic.pedia.describe-classic.tourism-same-ideology = которые придерживаются вашей идеологии
classic.pedia.describe-classic.tourism-less-happy = которые менее счастливы, чем вы
classic.pedia.describe-classic.tourism-common-enemy = которые воюют с вашими врагами
