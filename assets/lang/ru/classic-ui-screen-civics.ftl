# Translation of classic-ui-screen-civics.ftl into ru. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.ui.screen-civics.title = Социальные институты
classic.ui.screen-civics.status-ready = культура: { $culture }  ·  +{ $per_turn } за ход  ·  следующий институт: { $cost } культуры, можно принять сейчас
classic.ui.screen-civics.status-turns = культура: { $culture }  ·  +{ $per_turn } за ход  ·  следующий институт: { $cost } культуры, { $n ->
    [one] через { $n } ход
    [few] через { $n } хода
    [many] через { $n } ходов
   *[other] через { $n } хода
}
classic.ui.screen-civics.status-stalled = культура: { $culture }  ·  +{ $per_turn } за ход  ·  следующий институт: { $cost } культуры, дохода культуры нет
classic.ui.screen-civics.free-policies = { $n ->
    [one] { $n } бесплатный институт
    [few] { $n } бесплатных института
    [many] { $n } бесплатных институтов
   *[other] { $n } бесплатных института
}
classic.ui.screen-civics.free-tenets = { $n ->
    [one] { $n } бесплатный догмат
    [few] { $n } бесплатных догмата
    [many] { $n } бесплатных догматов
   *[other] { $n } бесплатных догмата
}
classic.ui.screen-civics.branches = ВЕТВИ
classic.ui.screen-civics.ideologies = ИДЕОЛОГИИ
classic.ui.screen-civics.status-adopted-tenets = { $n ->
    [one] ПРИНЯТО · { $n } догмат
    [few] ПРИНЯТО · { $n } догмата
    [many] ПРИНЯТО · { $n } догматов
   *[other] ПРИНЯТО · { $n } догмата
}
classic.ui.screen-civics.status-policies = ИНСТИТУТОВ: { $adopted } / { $total }
classic.ui.screen-civics.can-be-chosen = МОЖНО ВЫБРАТЬ
classic.ui.screen-civics.can-be-unlocked = МОЖНО ОТКРЫТЬ
classic.ui.screen-civics.from-era = ОТКРОЕТСЯ: { $era }
classic.ui.screen-civics.choose-ideology = Выбрать идеологию
classic.ui.screen-civics.choose-ideology-free = Выбрать идеологию (+{ $n } догм.)
classic.ui.screen-civics.level = УРОВЕНЬ { $tier }
classic.ui.screen-civics.adopted = ПРИНЯТО
classic.ui.screen-civics.available = ДОСТУПНО
classic.ui.screen-civics.branch-completed = ВЕТВЬ ЗАВЕРШЕНА
classic.ui.screen-civics.complete-branch = ЗАВЕРШИТЕ ВЕТВЬ
classic.ui.screen-civics.role-opener = { $branch } · ВХОДНОЙ
classic.ui.screen-civics.role-policy = { $branch } · ИНСТИТУТ
classic.ui.screen-civics.tab-available = Доступно · { $n }
classic.ui.screen-civics.branches-tab = Ветви
classic.ui.screen-civics.opener = ВХОДНОЙ ИНСТИТУТ
classic.ui.screen-civics.finisher = ЗАВЕРШАЮЩИЙ
classic.ui.screen-civics.needs-culture = Нужно { $cost } культуры (сейчас { $have })
classic.ui.screen-civics.needs-policy = Сначала нужен: { $name }
classic.ui.screen-civics.adopt-culture = Принять · { $n }
classic.ui.screen-civics.adopt-free = Принять · бесплатно
classic.ui.screen-civics.unlock-culture = Открыть · { $n }
classic.ui.screen-civics.unlock-free = Открыть · бесплатно
classic.ui.screen-civics.choose = Выбрать
classic.ui.screen-civics.tag-tenets = Догматы: { $n }
classic.ui.screen-civics.tag-price = { $n } культуры
classic.ui.screen-civics.tag-free = бесплатно
classic.ui.screen-civics.included = ИДЁТ С ВЕТВЬЮ
classic.ui.screen-civics.available-title = ДОСТУПНО СЕЙЧАС
classic.ui.screen-civics.nothing-now = Сейчас нечего принимать
classic.ui.screen-civics.culture-bar = { $culture } из { $cost } культуры до следующего института
classic.ui.screen-civics.keys-hint = Стрелки: ветвь и институт · Enter: принять · Пробел: сменить список
classic.ui.screen-civics.price-rises = Каждый принятый институт удорожает следующий: сейчас { $now }, потом { $after }
classic.ui.screen-civics.role-ideology = { $branch } · ИДЕОЛОГИЯ
classic.ui.screen-civics.tag-progress = { $adopted }/{ $total }
