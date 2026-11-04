# Translation of classic-ui-hud-game.ftl into de. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.ui.hud-game.top-gold = { $gold } ({ $delta }) Gold
classic.ui.hud-game.top-food = { $food } Nahrung
classic.ui.hud-game.top-production = { $production } Produktion
classic.ui.hud-game.top-science = { $science } ({ $delta }) Wissenschaft
classic.ui.hud-game.top-happiness = { $happiness } Zufriedenheit
classic.ui.hud-game.top-culture = { $culture } ({ $delta }) Kultur
classic.ui.hud-game.top-faith = { $faith } ({ $delta }) Glaube

## Obere Leiste: Kultur und Glaube bis zum nächsten Schritt ("24/45"), Klickhinweis und Tooltip-Sätze
classic.ui.hud-game.top-culture-goal = { $have }/{ $cost } Kultur
classic.ui.hud-game.top-faith-goal = { $have }/{ $cost } Glaube
classic.ui.hud-game.click-to-open = Klicken zum Öffnen: { $screen }
classic.ui.hud-game.goal-research-ready = Eine Technologie kann jetzt gewählt werden: nichts wird erforscht
classic.ui.hud-game.goal-policy-ready = Eine Sozialpolitik kann jetzt angenommen werden
classic.ui.hud-game.goal-policy-turns = Noch { $missing } Kultur bis zur nächsten Sozialpolitik: { $turns ->
    [one] 1 Zug
   *[other] { $turns } Züge
} bei +{ $rate } pro Zug
classic.ui.hud-game.goal-policy-stalled = Noch { $missing } Kultur bis zur nächsten Sozialpolitik: — (kein Kultureinkommen)
classic.ui.hud-game.goal-pantheon-ready = Ein Pantheon kann jetzt gegründet werden
classic.ui.hud-game.goal-pantheon-turns = Noch { $missing } Glaube bis zum Pantheon: { $turns ->
    [one] 1 Zug
   *[other] { $turns } Züge
} bei +{ $rate } pro Zug
classic.ui.hud-game.goal-pantheon-stalled = Noch { $missing } Glaube bis zum Pantheon: — (kein Glaubenseinkommen)
classic.ui.hud-game.goal-reformation-ready = Ein Reformations-Glaubenssatz kann jetzt hinzugefügt werden
classic.ui.hud-game.goal-prophet-turns = Noch { $missing } Glaube bis zum nächsten Großen Propheten: { $turns ->
    [one] 1 Zug
   *[other] { $turns } Züge
} bei +{ $rate } pro Zug
classic.ui.hud-game.goal-prophet-stalled = Noch { $missing } Glaube bis zum nächsten Großen Propheten: — (kein Glaubenseinkommen)
classic.ui.hud-game.open-research = Forschung öffnen
classic.ui.hud-game.research-kicker = FORSCHUNG
classic.ui.hud-game.research-choose = Forschung wählen
classic.ui.hud-game.research-idle = Es wird nichts erforscht
classic.ui.hud-game.research-detail = { $stored }/{ $cost }  ·  { $n ->
    [one] { $n } Zug
   *[other] { $n } Züge
}
classic.ui.hud-game.research-detail-stalled = { $stored }/{ $cost }  ·  noch keine Wissenschaft
classic.ui.hud-game.rival-war = { $name } · Krieg
classic.ui.hud-game.rival-peace = { $name } · Frieden
classic.ui.hud-game.make-peace = Frieden schließen
classic.ui.hud-game.open-borders = Offene Grenzen
classic.ui.hud-game.blocked-reason = { $action }: { $reason }
classic.ui.hud-game.notice-line = Z{ $turn }  { $text }
classic.ui.hud-game.notice-hint = Auf der Karte zeigen · F7 für das ganze Protokoll
classic.ui.hud-game.capture-title = { $city } gehört Ihnen
classic.ui.hud-game.capture-line = Gegründet von { $founder }. Entscheiden Sie, was daraus wird.
classic.ui.hud-game.unavailable = Nicht verfügbar
classic.ui.hud-game.ruins-title = Antike Ruinen
classic.ui.hud-game.ruins-line = Ihre Einheit hat Ruinen gefunden. Wählen Sie eine von drei Belohnungen.
classic.ui.hud-game.ruins-later = Später
classic.ui.hud-game.ruins-later-hint = Die Ruinen bleiben auf der Karte
classic.ui.hud-game.victory = Sieg
classic.ui.hud-game.victory-line = Ihre Nation hat das Spiel gewonnen.
classic.ui.hud-game.defeat = Niederlage
classic.ui.hud-game.defeat-line = Ihre Nation hat das Spiel verloren.
classic.ui.hud-game.see-results = Ergebnisse ansehen

## city-state card over the map
classic.ui.hud-game.city-state-gold = Schatzkammer: { $gold } Gold
classic.ui.hud-game.city-state-close = Schließen (Esc)
