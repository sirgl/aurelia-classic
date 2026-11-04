# Translation of classic-game-camps.ftl into ja. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.game.camp.cleared = 蛮族の野営地を制圧：ゴールド +{ NUMBER($gold) }
classic.game.camp.cleared-joined = 蛮族の野営地を制圧：ゴールド +{ NUMBER($gold) }。蛮族が仲間になった（ゴールド +{ NUMBER($joined) }）
classic.game.camp.joined = 野営地の蛮族が仲間になった：ゴールド +{ NUMBER($gold) }

classic.game.income.gold = ゴールド +{ NUMBER($n) }
classic.game.income.culture = 文化 +{ NUMBER($n) }
classic.game.income.science = 科学力 +{ NUMBER($n) }
classic.game.income.food = 食料 +{ NUMBER($n) }
classic.game.income.production = 生産力 +{ NUMBER($n) }
classic.game.income.faith = 信仰 +{ NUMBER($n) }
classic.game.income.and = { $a }、{ $b }
classic.game.kill.notice = { $source }：{ $parts }（{ $barbarian ->
    [yes] 蛮族を倒した報酬
   *[no] 撃破報酬
}）
classic.game.kill.notice-plain = { $parts }（{ $barbarian ->
    [yes] 蛮族を倒した報酬
   *[no] 撃破報酬
}）
classic.game.plunder.improvement = 改良の略奪：ゴールド +{ NUMBER($gold) }
classic.game.plunder.city = 都市の略奪：ゴールド +{ NUMBER($gold) }
classic.game.plunder.route = 交易路の略奪：ゴールド +{ NUMBER($gold) }

classic.game.wonder.found = 自然の驚異を発見：{ $name }
classic.game.wonder.found-with = 自然の驚異を発見：{ $name }：{ $parts }
classic.game.wonder.gold-first = ゴールド +{ NUMBER($n) }（最初の発見者）
classic.game.wonder.happiness = 幸福度 +{ NUMBER($n) }
