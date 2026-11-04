# Translation of classic-ui-hud-game.ftl into fr. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.ui.hud-game.top-gold = { $gold } ({ $delta }) or
classic.ui.hud-game.top-food = { $food } nourriture
classic.ui.hud-game.top-production = { $production } production
classic.ui.hud-game.top-science = { $science } ({ $delta }) science
classic.ui.hud-game.top-happiness = { $happiness } bonheur
classic.ui.hud-game.top-culture = { $culture } ({ $delta }) culture
classic.ui.hud-game.top-faith = { $faith } ({ $delta }) foi

## barre supérieure : culture et foi jusqu'à la prochaine étape ("24/45"), indication du clic et phrases de l'infobulle
classic.ui.hud-game.top-culture-goal = { $have }/{ $cost } culture
classic.ui.hud-game.top-faith-goal = { $have }/{ $cost } foi
classic.ui.hud-game.click-to-open = Cliquer pour ouvrir : { $screen }
classic.ui.hud-game.goal-research-ready = Une technologie peut être choisie maintenant : aucune recherche en cours
classic.ui.hud-game.goal-policy-ready = Une politique peut être adoptée maintenant
classic.ui.hud-game.goal-policy-turns = Encore { $missing } culture pour la prochaine politique : { $turns ->
    [one] 1 tour
   *[other] { $turns } tours
} à +{ $rate } par tour
classic.ui.hud-game.goal-policy-stalled = Encore { $missing } culture pour la prochaine politique : — (aucun revenu de culture)
classic.ui.hud-game.goal-pantheon-ready = Un panthéon peut être fondé maintenant
classic.ui.hud-game.goal-pantheon-turns = Encore { $missing } foi pour le panthéon : { $turns ->
    [one] 1 tour
   *[other] { $turns } tours
} à +{ $rate } par tour
classic.ui.hud-game.goal-pantheon-stalled = Encore { $missing } foi pour le panthéon : — (aucun revenu de foi)
classic.ui.hud-game.goal-reformation-ready = Une croyance de réforme peut être ajoutée maintenant
classic.ui.hud-game.goal-prophet-turns = Encore { $missing } foi pour le prochain grand prophète : { $turns ->
    [one] 1 tour
   *[other] { $turns } tours
} à +{ $rate } par tour
classic.ui.hud-game.goal-prophet-stalled = Encore { $missing } foi pour le prochain grand prophète : — (aucun revenu de foi)
classic.ui.hud-game.open-research = Ouvrir la recherche
classic.ui.hud-game.research-kicker = RECHERCHE
classic.ui.hud-game.research-choose = Choisir une recherche
classic.ui.hud-game.research-idle = Aucune recherche en cours
classic.ui.hud-game.research-detail = { $stored }/{ $cost }  ·  { $n ->
    [one] { $n } tour
   *[other] { $n } tours
}
classic.ui.hud-game.research-detail-stalled = { $stored }/{ $cost }  ·  pas encore de science
classic.ui.hud-game.rival-war = { $name } · Guerre
classic.ui.hud-game.rival-peace = { $name } · Paix
classic.ui.hud-game.make-peace = Faire la paix
classic.ui.hud-game.open-borders = Ouvrir les frontières
classic.ui.hud-game.blocked-reason = { $action } : { $reason }
classic.ui.hud-game.notice-line = T{ $turn }  { $text }
classic.ui.hud-game.notice-hint = Afficher sur la carte · F7 pour le journal complet
classic.ui.hud-game.capture-title = { $city } est à vous
classic.ui.hud-game.capture-line = Fondée par { $founder }. Décidez de son sort.
classic.ui.hud-game.unavailable = Indisponible
classic.ui.hud-game.ruins-title = Ruines antiques
classic.ui.hud-game.ruins-line = Votre unité a découvert des ruines. Choisissez l'une des trois récompenses.
classic.ui.hud-game.ruins-later = Plus tard
classic.ui.hud-game.ruins-later-hint = Les ruines restent sur la carte
classic.ui.hud-game.victory = Victoire
classic.ui.hud-game.victory-line = Votre nation a remporté la partie.
classic.ui.hud-game.defeat = Défaite
classic.ui.hud-game.defeat-line = Votre nation a perdu la partie.
classic.ui.hud-game.see-results = Voir les résultats

## city-state card over the map
classic.ui.hud-game.city-state-gold = Trésor : { $gold } or
classic.ui.hud-game.city-state-close = Fermer (Échap)
