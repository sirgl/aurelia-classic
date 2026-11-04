# Translation of classic-pedia-describe.ftl into de. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.pedia.describe.join = { $a }, { $b }
classic.pedia.describe.join-or = { $a } oder { $b }
classic.pedia.describe.yield-amount = { $amount } { $yield }
classic.pedia.describe.yield-output = Ertrag
classic.pedia.describe.yield-food = Nahrung
classic.pedia.describe.yield-production = Produktion
classic.pedia.describe.yield-gold = Gold
classic.pedia.describe.yield-science = Wissenschaft
classic.pedia.describe.yield-culture = Kultur
classic.pedia.describe.yield-faith = Glaube
classic.pedia.describe.with-trigger = { $trigger }: { $text }
classic.pedia.describe.with-selector = { $text } { $selector }
classic.pedia.describe.with-condition = { $text } { $condition }
classic.pedia.describe.with-second-condition = { $text } und { $condition }
classic.pedia.describe.with-duration = { $text } für { $turns ->
    [one] { $turns } Zug
   *[other] { $turns } Züge
}
classic.pedia.describe.trigger-on-built = beim Bau
classic.pedia.describe.trigger-on-turn-start = jeden Zug
classic.pedia.describe.trigger-on-technology-unlocked = beim Erforschen
classic.pedia.describe.trigger-on-city-founded = bei Gründung einer Stadt
classic.pedia.describe.trigger-on-unit-killed = beim Besiegen einer Einheit
classic.pedia.describe.trigger-on-unit-trained = bei hier ausgebildeten Einheiten
classic.pedia.describe.trigger-on-city-conquered = bei Eroberung einer Stadt
classic.pedia.describe.trigger-on-era-entered = beim Eintritt in ein neues Zeitalter
classic.pedia.describe.selector-source-city = in dieser Stadt
classic.pedia.describe.selector-owner-cities = in jeder Stadt
classic.pedia.describe.selector-event-city = in der betroffenen Stadt
classic.pedia.describe.selector-owner-units = für jede Einheit
classic.pedia.describe.selector-event-unit = für die betroffene Einheit
classic.pedia.describe.selector-source-city-tiles = auf Feldern, die diese Stadt bearbeitet
classic.pedia.describe.selector-owner-city-tiles = auf bearbeiteten Feldern
classic.pedia.describe.selector-capital = in der Hauptstadt
classic.pedia.describe.condition-has-technology = mit { $name }
classic.pedia.describe.condition-lacks-technology = bis { $name }
classic.pedia.describe.condition-min-population = ab { $n ->
    [one] { $n } Bürger
   *[other] { $n } Bürgern
}
classic.pedia.describe.condition-has-building = mit { $name }
classic.pedia.describe.condition-on-tile = auf { $name }
classic.pedia.describe.condition-terrain-until = auf { $terrain } bis { $technology }
classic.pedia.describe.condition-against-role = gegen { $role }-Einheiten
classic.pedia.describe.condition-against-class = gegen Einheiten wie { $name }
classic.pedia.describe.condition-for-units = für { $kind }-Einheiten
classic.pedia.describe.condition-near-city = im Umkreis von { $n ->
    [one] { $n } Feld
   *[other] { $n } Feldern
} um die Stadt
classic.pedia.describe.condition-resource-class = auf Ressourcen der Klasse { $class }
classic.pedia.describe.condition-unimproved-feature = auf { $name } ohne Verbesserung
classic.pedia.describe.condition-adjacent-unimproved-feature = mit { $name } ohne Verbesserung neben der Stadt (mindestens { $n })
classic.pedia.describe.condition-producing = bei Produktion: { $what }
classic.pedia.describe.condition-garrisoned = mit Garnison
classic.pedia.describe.condition-coastal = bei Küstenlage
classic.pedia.describe.condition-learned-technology = beim Erforschen von { $name }
classic.pedia.describe.condition-on-river = wenn die Stadt an einem Fluss liegt
classic.pedia.describe.condition-at-peace = solange Frieden mit allen Nationen herrscht
classic.pedia.describe.condition-connected = wenn die Stadt mit der Hauptstadt verbunden ist
classic.pedia.describe.condition-near-improvement = im Umkreis von { $n ->
    [one] { $n } Feld
   *[other] { $n } Feldern
} um { $name }
classic.pedia.describe.plot-river = auf Flussfeldern
classic.pedia.describe.plot-sea = auf Meeresfeldern
classic.pedia.describe.plot-lake = auf Seefeldern
classic.pedia.describe.plot-sea-resource = auf Meeresressourcen
classic.pedia.describe.plot-natural-wonder = auf Naturwundern
classic.pedia.describe.production-units = Einheiten
classic.pedia.describe.production-kind-units = { $kind }-Einheiten
classic.pedia.describe.production-a = { $name }
classic.pedia.describe.production-military-units = Militäreinheiten
classic.pedia.describe.production-spaceship-parts = Raumschiffteile
classic.pedia.describe.production-buildings = Gebäude
classic.pedia.describe.production-world-wonders = Weltwunder
classic.pedia.describe.production-world-wonders-before = Weltwunder vor { $era }
classic.pedia.describe.production-built-in-capital = Gebäude, die die Hauptstadt bereits hat
classic.pedia.describe.role-great-person = Persönlichkeits
classic.pedia.describe.role-religious = Religions
classic.pedia.describe.role-civilian = Zivil
classic.pedia.describe.role-spaceship-part = Raumschiffteil
classic.pedia.describe.role-recon = Aufklärungs
classic.pedia.describe.role-melee = Nahkampf
classic.pedia.describe.role-archer = Bogenschützen
classic.pedia.describe.role-mounted = Reiter
classic.pedia.describe.role-siege = Belagerungs
classic.pedia.describe.role-gun = Schusswaffen
classic.pedia.describe.role-armor = Panzer
classic.pedia.describe.role-naval-melee = Marine-Nahkampf
classic.pedia.describe.role-naval-ranged = Marine-Fernkampf
classic.pedia.describe.role-submarine = U-Boot
classic.pedia.describe.role-carrier = Flugzeugträger
classic.pedia.describe.role-fighter = Jagdflugzeug
classic.pedia.describe.role-bomber = Bomber
classic.pedia.describe.role-helicopter = Hubschrauber
classic.pedia.describe.role-missile = Raketen
classic.pedia.describe.domain-land = Land
classic.pedia.describe.domain-sea = See
classic.pedia.describe.domain-air = Luft
classic.pedia.describe.resource-class-bonus = Bonus
classic.pedia.describe.resource-class-strategic = Strategisch
classic.pedia.describe.resource-class-luxury = Luxus
classic.pedia.describe.resource-class-artifact = Artefakt
classic.pedia.describe.modifier-add-yield = { $amount } { $yield }
classic.pedia.describe.modifier-add-yield-percent = { $percent } % { $yield }
classic.pedia.describe.modifier-multiply-yield = ×{ $factor } { $yield }
classic.pedia.describe.modifier-growth = { $percent } % Wachstum
classic.pedia.describe.modifier-cost = { $percent } % { $target }
classic.pedia.describe.modifier-combat = { $percent } % Kampfstärke
classic.pedia.describe.modifier-combat-in = { $text } { $context }
classic.pedia.describe.modifier-movement = { $amount } Bewegung
classic.pedia.describe.modifier-unlocks = schaltet { $name } frei
classic.pedia.describe.modifier-free-unit = { $count }× { $name } gratis
classic.pedia.describe.modifier-free-building = { $name } gratis
classic.pedia.describe.modifier-promotion = Beförderung { $name }
classic.pedia.describe.modifier-free-building-in-cities = { $count ->
    [one] { $name } gratis in der ersten Stadt
   *[other] { $name } gratis in den ersten { $count } Städten
}
classic.pedia.describe.modifier-yield-per-count = { $amount } { $yield } { $per }
classic.pedia.describe.modifier-yield-percent-per-count = { $percent } % { $yield } { $per }
classic.pedia.describe.modifier-yield-from-kill = { $yield } für jede besiegte feindliche Einheit, mehr für stärkere
classic.pedia.describe.modifier-forbid-unit = Einheit { $name } kann weder ausgebildet noch gekauft werden
classic.pedia.describe.modifier-forbid-annexing = kann keine Städte annektieren
classic.pedia.describe.modifier-post-combat-promotion = kann nach dem ersten Kampf die Beförderung { $name } erhalten
classic.pedia.describe.modifier-passive-promotion = geeignete Einheiten erhalten die Wirkung der Beförderung { $name }
classic.pedia.describe.modifier-experience = +{ $amount } Erfahrungspunkte
classic.pedia.describe.per-count-one = pro { $unit }
classic.pedia.describe.per-count-many = pro { $every } { $unit }
classic.pedia.describe.per-count-capped = { $per } (höchstens { $max })
classic.pedia.describe.count-population = { $n ->
    [one] Bürger
   *[other] Bürger
}
classic.pedia.describe.count-world-wonders = { $n ->
    [one] Weltwunder
   *[other] Weltwunder
}
classic.pedia.describe.count-connected-cities = { $n ->
    [one] mit der Hauptstadt verbundene Stadt
   *[other] mit der Hauptstadt verbundene Städte
}
classic.pedia.describe.per-population-step = { $n ->
    [one] +1 { $yield } pro Bürger
   *[other] +1 { $yield } pro { $n } Bürger
}
classic.pedia.describe.per-population = { $amount } { $yield } pro Bürger
classic.pedia.describe.cost-units = Produktionskosten von Einheiten
classic.pedia.describe.cost-buildings = Produktionskosten von Gebäuden
classic.pedia.describe.cost-technologies = Technologiekosten
classic.pedia.describe.cost-tile-purchase = Kosten für den Kauf von Feldern
classic.pedia.describe.cost-hurry = Kaufkosten
classic.pedia.describe.cost-border-growth = für das Grenzwachstum benötigte Kultur
classic.pedia.describe.cost-border-growth-exponent = Kostensteigerung des Grenzwachstums
classic.pedia.describe.cost-unit-upgrade = Aufrüstkosten von Einheiten
classic.pedia.describe.cost-unit-purchase = Goldkosten für den Kauf von Einheiten
classic.pedia.describe.cost-building-purchase = Goldkosten für den Kauf von Gebäuden
classic.pedia.describe.context-attack = beim Angriff
classic.pedia.describe.context-defense = bei der Verteidigung
classic.pedia.describe.context-vs-city = gegen Städte
classic.pedia.describe.context-attack-open = beim Angriff auf offenem Gelände
classic.pedia.describe.context-attack-rough = beim Angriff in unwegsamem Gelände
classic.pedia.describe.context-defense-open = bei der Verteidigung auf offenem Gelände
classic.pedia.describe.context-defense-rough = bei der Verteidigung in unwegsamem Gelände
classic.pedia.describe.context-ranged-attack-open = bei Fernangriffen auf offenem Gelände
classic.pedia.describe.context-ranged-attack-rough = bei Fernangriffen in unwegsamem Gelände
classic.pedia.describe.context-defense-vs-ranged = bei der Verteidigung gegen Fernangriffe
classic.pedia.describe.context-ranged-attack = bei Fernangriffen
classic.pedia.describe.context-vs-wounded = gegen verwundete Einheiten
classic.pedia.describe.context-vs-fortified = gegen befestigte Einheiten
classic.pedia.describe.context-friendly-land = im eigenen Gebiet
classic.pedia.describe.context-outside-friendly-land = außerhalb des eigenen Gebiets
classic.pedia.describe.context-attack-friendly-land = beim Angriff im eigenen Gebiet
classic.pedia.describe.context-flanking = pro flankierender Einheit
classic.pedia.describe.context-adjacent-friendly = neben einer eigenen Einheit
classic.pedia.describe.context-near-capital = in der Nähe der Hauptstadt
classic.pedia.describe.context-embarked-defense = bei der Verteidigung im eingeschifften Zustand
classic.pedia.describe.context-interception = beim Abfangen
classic.pedia.describe.context-air-sweep = bei Luftsäuberungen
classic.pedia.describe.city-combat-defense = { $amount } Stadtkampfstärke durch Verteidigungsanlagen
classic.pedia.describe.city-combat-hit-points = { $amount } TP der Stadt
classic.pedia.describe.city-combat-defense-percent = { $amount } % Verteidigungsstärke durch Gebäude
classic.pedia.describe.city-combat-garrison-ranged = { $amount } % Fernkampfstärke der Stadt bei Garnison
classic.pedia.describe.city-combat-nuke-damage = { $amount } % Schaden durch Atomangriffe
classic.pedia.describe.city-combat-air-capacity = { $amount } Flugzeugkapazität
classic.pedia.describe.city-combat-ranged = { $amount } % Fernkampfstärke der Stadt
classic.pedia.describe.espionage-enemy-steal = { $amount } % Tempo, mit dem feindliche Spione Technologien stehlen
classic.pedia.describe.espionage-steal = { $amount } % Tempo, mit dem Ihre Spione Technologien stehlen
classic.pedia.describe.espionage-catch = { $amount } % Chance, feindliche Spione zu fangen
classic.pedia.describe.espionage-rig-election = { $amount } % Stimmen beim Manipulieren von Stadtstaat-Wahlen
classic.pedia.describe.espionage-extra-spies = { $amount } Spione zusammen mit dem ersten Spion
classic.pedia.describe.espionage-starting-rank = neue Spione beginnen { $amount } Ränge höher
classic.pedia.describe.espionage-spy-pressure = Spione üben in ihrer Stadt { $amount }-fachen religiösen Druck aus
classic.pedia.describe.league-delegates = { $amount } Delegierte im Weltkongress
classic.pedia.describe.league-delegates-per-diplomat = { $amount } Delegierte pro Diplomat in einer fremden Hauptstadt
classic.pedia.describe.stat-resource-copies = liefert { $amount } { $resource }
classic.pedia.describe.stat-resource-quantity = { $amount } % { $resource } auf Ihren Feldern
classic.pedia.describe.stat-strategic-quantity = { $amount } % strategische Ressourcen auf Ihren Feldern
classic.pedia.describe.stat-extra-luxury-copies = { $amount } Exemplar jedes Luxusguts rund um die Stadt
classic.pedia.describe.stat-great-people-rate = { $amount } % Entstehungsrate Großer Persönlichkeiten
classic.pedia.describe.stat-great-person-rate = { $amount } % Punkte ({ $kind })
classic.pedia.describe.stat-food-kept = { $amount } % Nahrung bleibt beim Wachstum erhalten
classic.pedia.describe.stat-specialist-yield = { $amount } { $yield } pro Spezialist
classic.pedia.describe.stat-happiness-per-city = { $amount } Zufriedenheit pro Stadt
classic.pedia.describe.stat-happiness-per-luxury = { $amount } Zufriedenheit pro Luxusgut
classic.pedia.describe.stat-natural-wonder-happiness = { $amount } % Zufriedenheit durch Naturwunder
classic.pedia.describe.stat-city-count-unhappiness = { $amount } % Unzufriedenheit durch die Anzahl der Städte
classic.pedia.describe.stat-population-unhappiness = { $amount } % Unzufriedenheit durch Bevölkerung
classic.pedia.describe.stat-golden-age-length = { $amount } % Dauer Goldener Zeitalter
classic.pedia.describe.stat-connection-gold = { $amount } % Gold durch Stadtverbindungen
classic.pedia.describe.stat-research-agreement = { $amount } % Wissenschaft durch Forschungsabkommen
classic.pedia.describe.stat-great-scientist-science = { $amount } % Wissenschaft von Großen Wissenschaftlern
classic.pedia.describe.stat-great-people-per-friend = { $amount } % Große Persönlichkeiten pro Freundschaftserklärung
classic.pedia.describe.stat-free-maintenance-units = { $amount } Einheiten ohne Gold-Unterhalt
classic.pedia.describe.stat-barbarian-conversion = { $amount } % Chance, dass eine in ihrem Lager besiegte barbarische Landeinheit zu Ihnen wechselt und 25 Gold mitbringt
classic.pedia.describe.stat-plunder-gold = { $amount } % Gold aus Barbarenlagern und eroberten Städten
classic.pedia.describe.stat-captured-gold = { $amount } % Gold für den Eroberer dieser Stadt
classic.pedia.describe.stat-worker-speed = { $amount } % Arbeitertempo
classic.pedia.describe.stat-route-maintenance = { $amount } % Unterhalt für Straßen und Eisenbahnen
classic.pedia.describe.stat-any-specialist-yield = { $amount } { $yield } in einer Stadt mit Spezialist
classic.pedia.describe.rule-garrison-free-maintenance = in Städten stationierte Einheiten verursachen keinen Unterhalt
classic.pedia.describe.rule-free-hills-route-maintenance = kein Unterhalt für Straßen und Eisenbahnen auf Hügeln
classic.pedia.describe.rule-spaceship-part-purchase = Raumschiffteile können mit Gold gekauft werden
classic.pedia.describe.rule-spaceship-part-hurry = Große Ingenieure können Raumschiffteile beschleunigen
classic.pedia.describe.rule-see-barbarian-camps = neue Barbarenlager werden auf erkundetem Gebiet angezeigt
classic.pedia.describe.society-specialist-food = { $amount } % Nahrung, die Spezialisten verbrauchen
classic.pedia.describe.society-specialist-unhappiness = { $amount } % Unzufriedenheit durch Spezialisten
classic.pedia.describe.society-no-occupied-unhappiness = keine zusätzliche Unzufriedenheit bei Besetzung
classic.pedia.describe.society-wonder-culture = { $amount } % Kultur durch Weltwunder
classic.pedia.describe.society-happiness-to-culture = überschüssige Zufriedenheit wird in Kultur umgewandelt
classic.pedia.describe.society-science-while-happy = { $amount } % Wissenschaft, solange das Reich zufrieden ist
classic.pedia.describe.society-luxury-retention = behält { $amount } % der Zufriedenheit eines Luxusguts, wenn dessen letztes Exemplar abgegeben wird
classic.pedia.describe.society-raze-speed = { $amount } % Tempo beim Niederreißen
classic.pedia.describe.society-founding-territory = neue Städte beanspruchen { $amount } zusätzliche Felder
classic.pedia.describe.society-capital-science-boost = in der Hauptstadt gebaute Wissenschaftsgebäude geben einen Forschungsschub
classic.pedia.describe.society-unique-luxury-cities = die ersten { $amount } Städte auf anderen Landmassen erhalten ein einzigartiges Luxusgut
classic.pedia.describe.society-unique-luxury-quantity = { $amount } Exemplare jedes einzigartigen Luxusguts
classic.pedia.describe.society-unique-luxury-new-landmass = einzigartige Luxusgüter nur auf Landmassen abseits der Hauptstadt
classic.pedia.describe.trade-routes = { $amount } Handelsrouten
classic.pedia.describe.trade-range-tiles = { $amount } Felder Reichweite für { $domain }-Handelsrouten
classic.pedia.describe.trade-range-percent = { $amount } % Reichweite der { $domain }-Handelsrouten
classic.pedia.describe.trade-route-gold = { $amount } Gold aus { $domain }-Handelsrouten
classic.pedia.describe.trade-target-gold = { $amount } Gold für Handelsrouten zu dieser Stadt
classic.pedia.describe.trade-recipient-gold = { $amount } Gold für diese Stadt aus ausländischen Handelsrouten
classic.pedia.describe.trade-internal-percent = { $amount } % Nahrung und Produktion interner Handelsrouten
classic.pedia.describe.trade-routes-percent = { $amount } % Handelsrouten
classic.pedia.describe.trade-resource-gold-percent = { $amount } % Gold durch Ressourcenvielfalt auf Handelsrouten
classic.pedia.describe.trade-incoming-route-gold = { $amount } Gold für alle, die Ihnen eine Handelsroute schicken
classic.pedia.describe.trade-city-state-route-gold = { $amount } Gold aus Handelsrouten zu Stadtstaaten
classic.pedia.describe.trade-shared-ideology-route-gold = { $amount } Gold aus Handelsrouten zu Nationen Ihrer Ideologie
classic.pedia.describe.trade-religious-pressure = { $amount } % religiöser Druck entlang von Handelsrouten
classic.pedia.describe.placement-land = Wird auf dem Land platziert
classic.pedia.describe.placement-water = Erhebt sich aus dem Meer
classic.pedia.describe.placement-reef = Wird im offenen Wasser nahe einer Küste platziert
classic.pedia.describe.placement-peninsula-tip = Wird an der Spitze einer Halbinsel platziert
classic.pedia.describe.placement-of-terrains = { $text }, Gelände: { $terrains }
classic.pedia.describe.placement-away-from-coast = { $text }, abseits der Küste
classic.pedia.describe.placement-off-largest-landmass = { $text }, abseits der größten Landmasse
classic.pedia.describe.placement-sentence = { $text }.
classic.pedia.describe.adjacent-hills = Hügel
classic.pedia.describe.adjacent-mountains = Berge
classic.pedia.describe.adjacent-hills-or-mountains = Hügel oder Berge
classic.pedia.describe.adjacent-land = Landfelder
classic.pedia.describe.adjacent-water = Wasserfelder
classic.pedia.describe.adjacent-at-least = Mindestens { $n } benachbarte { $what }.
classic.pedia.describe.adjacent-none = Keine benachbarten { $what }.
classic.pedia.describe.adjacent-at-most = Höchstens { $n } benachbarte { $what }.
classic.pedia.describe.shape-mountain = Das Feld wird zum Berg.
classic.pedia.describe.shape-flat = Das Feld wird zu flachem Land.
classic.pedia.describe.surrounded-by-sea = Es ist vom Meer umgeben.
classic.pedia.describe.grant-free-technologies = { $n ->
    [one] eine freie Technologie Ihrer Wahl
   *[other] { $n } freie Technologien Ihrer Wahl
}
classic.pedia.describe.grant-conquest-technology = eine freie Technologie, die der frühere Besitzer der Stadt kennt (einmal pro Stadt)
classic.pedia.describe.grant-military-unit-copies = eine Kopie jeder Ihrer Landkampfeinheiten
classic.pedia.describe.grant-population = { $n ->
    [one] { $amount } Bürger
   *[other] { $amount } Bürger
}
classic.pedia.describe.unit-stat-sight = { $amount } Felder Sicht
classic.pedia.describe.unit-stat-embarked-sight = { $amount } Felder Sicht im eingeschifften Zustand
classic.pedia.describe.unit-stat-range = { $amount } Felder Fernkampfreichweite
classic.pedia.describe.unit-stat-attacks-per-turn = { $amount } Angriffe pro Zug
classic.pedia.describe.unit-stat-cargo = { $amount } Kapazität für Flugzeuge oder Raketen
classic.pedia.describe.unit-stat-intercept-chance = { $amount } Prozentpunkte Abfangchance
classic.pedia.describe.unit-stat-interceptions-per-turn = { $amount } Abfangaktionen pro Zug
classic.pedia.describe.unit-stat-evasion = { $amount } Prozentpunkte beim Ausweichen vor Abfangversuchen
classic.pedia.describe.unit-stat-interception-damage = { $amount } % Schaden durch Abfangen
classic.pedia.describe.unit-stat-paradrop-range = { $amount } Felder Fallschirmsprung-Reichweite
classic.pedia.describe.unit-stat-naval-moves = { $amount } Bewegung auf See
classic.pedia.describe.unit-stat-experience = { $amount } % gewonnene Erfahrung
classic.pedia.describe.unit-stat-withdraw-chance = { $amount } Prozentpunkte Rückzugschance vor dem Nahkampf
classic.pedia.describe.unit-stat-city-attack-plunder = bringt beim Angriff auf Städte Gold ein, mehr bei mehr Schaden
classic.pedia.describe.unit-stat-heal-enemy-land = { $amount } TP Heilung pro Zug in Feindesgebiet
classic.pedia.describe.unit-stat-heal-neutral-land = { $amount } TP Heilung pro Zug in neutralem Gebiet
classic.pedia.describe.unit-stat-heal-friendly-land = { $amount } TP Heilung pro Zug im eigenen Gebiet
classic.pedia.describe.unit-stat-heal-adjacent = benachbarte eigene Einheiten heilen { $amount } TP pro Zug
classic.pedia.describe.unit-stat-heal-on-kill = { $amount } TP Heilung nach dem Zerstören einer feindlichen Einheit
classic.pedia.describe.unit-stat-enemy-aura = { $amount } % Kampfstärke für nahe feindliche Einheiten
classic.pedia.describe.unit-stat-aura-range = { $amount } Felder Reichweite des Effekts auf nahe Feinde
classic.pedia.describe.unit-stat-adjacent-damage = { $amount } Schaden an benachbarten feindlichen Einheiten nach dem Angriff
classic.pedia.describe.unit-stat-capital-defense-falloff = ändert, wie schnell der Verteidigungsbonus der Hauptstadt mit der Entfernung nachlässt
classic.pedia.describe.unit-stat-embarked-moves = { $amount } Bewegung im eingeschifften Zustand
classic.pedia.describe.unit-stat-wounded-damage = { $amount } Prozentpunkte beim Schadensmodifikator verwundeter Einheiten
classic.pedia.describe.ability-found-religion = Kann eine Religion gründen oder ausbauen
classic.pedia.describe.ability-spread-religion = Kann ihre Religion in einer Stadt verbreiten
classic.pedia.describe.ability-religion-spreads = Kann die Religion { $n ->
    [one] { $n } Mal
   *[other] { $n } Mal
} verbreiten
classic.pedia.describe.ability-religious-strength = Religiöse Stärke: { $n }
classic.pedia.describe.ability-remove-heresy = Kann andere Religionen aus einer Stadt vertreiben
classic.pedia.describe.ability-prohibits-spread = Die Anwesenheit dieser Einheit verhindert die Verbreitung anderer Religionen
classic.pedia.describe.ability-requires-enhanced-religion = Kann erst gekauft werden, wenn Ihre Religion ausgebaut ist
classic.pedia.describe.ability-one-shot-tourism = Ein Konzert bringt Tourismus, der mit Ihrem aktuellen Tourismus wächst
classic.pedia.describe.ability-one-shot-tourism-others = Andere Nationen erhalten { $n } % des Tourismus, den ein Konzert bringt
classic.pedia.describe.ability-culture-turns = Gewährt Kultur im Verhältnis zu Ihrer jüngsten Kulturleistung
classic.pedia.describe.ability-excavates = Kann Stätten der Antike ausgraben
classic.pedia.describe.ability-food = Während es ausgebildet wird, erhöht der Nahrungsüberschuss der Stadt die Produktion
classic.pedia.describe.ability-golden-age-turns = Kann ein Goldenes Zeitalter mit einer Grunddauer von { $n ->
    [one] { $n } Zug
   *[other] { $n } Zügen
} auslösen
classic.pedia.describe.ability-beakers-turns = Gewährt einen Schub Wissenschaft im Verhältnis zu Ihrer jüngsten Leistung
classic.pedia.describe.ability-base-hurry = Kann der laufenden Produktion einer Stadt Produktion hinzufügen
classic.pedia.describe.ability-hurry-multiplier = Eine größere Stadt erhält auf diese Weise mehr Produktion
classic.pedia.describe.ability-base-gold = Handelsmissionen bringen Gold ein
classic.pedia.describe.ability-gold-per-era = In späteren Epochen zahlen Handelsmissionen mehr
classic.pedia.describe.ability-no-maintenance = Kostet keinen Gold-Unterhalt
classic.pedia.describe.ability-trade = Kann Handelsrouten einrichten
classic.pedia.describe.ability-faith-purchase-needs-belief = Kann nur dort mit Glaube gekauft werden, wo ein Glaubenssatz ihr Zeitalter freischaltet
classic.pedia.describe.ability-mechanized = Eine Maschine, keine Menschen
classic.pedia.describe.ability-priority-low = Feinde greifen diese Einheit zuletzt an
classic.pedia.describe.ability-priority-early = Feinde greifen diese Einheit in der Frühphase zuerst an
classic.pedia.describe.ability-priority-high = Feinde greifen diese Einheit zuerst an
classic.pedia.describe.ability-conscription = Wehrpflichtwert { $n }
classic.pedia.describe.ability-great-person = Eine Große Persönlichkeit
classic.pedia.describe.ability-stealth = Tarnkappenflugzeug: kein Träger nimmt es auf
classic.pedia.describe.ability-air-slots = Belegt { $n ->
    [one] { $n } Luftplatz
   *[other] { $n } Luftplätze
} in einer Stadt
classic.pedia.describe.ability-work-rate = Arbeitstempo { $n }
classic.pedia.describe.ability-move-after-purchase = Kann sich in dem Zug bewegen, in dem sie gekauft wird
classic.pedia.describe.ability-no-bad-goodies = Antike Ruinen schaden ihr nie
classic.pedia.describe.ability-purchase-only = Kann nur gekauft, nie ausgebildet werden
classic.pedia.describe.ability-buy-city-state = Macht den Stadtstaat, in dem sie steht, zur Marionette ihres Besitzers
classic.pedia.describe.ability-found-abroad = Gründet Städte nur außerhalb der Landmasse der Hauptstadt
classic.pedia.describe.ability-exotic-goods = Verkauft { $n } Ladung exotischer Waren neben fremdem Land
classic.pedia.describe.combat-ability-move-after-attacking = kann sich nach dem Angriff bewegen
classic.pedia.describe.combat-ability-always-heal = kann nach Bewegung oder Angriff heilen
classic.pedia.describe.combat-ability-embarkation = kann sich einschiffen, um Wasser zu überqueren
classic.pedia.describe.combat-ability-embarked-all-water = kann eingeschifft tiefes Wasser befahren
classic.pedia.describe.combat-ability-move-impassable = kann normalerweise unpassierbares Gelände betreten
classic.pedia.describe.combat-ability-ranged-support-fire = führt vor dem Nahkampf einen unterstützenden Fernangriff aus
classic.pedia.describe.combat-ability-hills-double-move = zahlt auf Hügeln nur die halben Bewegungskosten
classic.pedia.describe.combat-ability-double-move = zahlt auf dem angegebenen Gelände nur die halben Bewegungskosten
classic.pedia.describe.combat-ability-impassable = kann das angegebene Gelände nicht betreten
classic.pedia.describe.combat-ability-insta-heal = heilt die Einheit sofort, statt eine dauerhafte Beförderung zu geben
classic.pedia.describe.combat-ability-amphibious = keine Angriffsstrafe bei der Landung vom Wasser oder beim Überqueren eines Flusses
classic.pedia.describe.combat-ability-river-crossing = keine Angriffsstrafe beim Überqueren eines Flusses
classic.pedia.describe.combat-ability-indirect-fire = Fernangriffe benötigen keine Sichtlinie
classic.pedia.describe.combat-ability-heal-outside-friendly = kann auf See außerhalb des eigenen Gebiets heilen
classic.pedia.describe.combat-ability-air-sweep = kann Luftsäuberungen durchführen
classic.pedia.describe.combat-ability-rival-territory = kann fremdes Gebiet ohne Offene Grenzen betreten
classic.pedia.describe.combat-ability-rough-terrain-ends-turn = das Betreten von unwegsamem Gelände beendet die Bewegung dieser Einheit
classic.pedia.describe.combat-ability-ignore-terrain-cost = ignoriert zusätzliche Bewegungskosten durch Gelände
classic.pedia.describe.combat-ability-flat-movement-cost = jedes Feld kostet gleich viel Bewegung, auch Straßen
classic.pedia.describe.combat-ability-hovering = kann sich über Land und Wasser bewegen
classic.pedia.describe.combat-ability-no-capture = kann keine Städte oder Einheiten erobern
classic.pedia.describe.combat-ability-only-defensive = kann keine Nahkampfangriffe ausführen
classic.pedia.describe.combat-ability-no-defensive-bonus = erhält keinen Geländeverteidigungs- oder Befestigungsbonus
classic.pedia.describe.combat-ability-must-set-up = muss vor einem Fernangriff in Stellung gehen
classic.pedia.describe.combat-ability-free-pillage-moves = Plündern kostet keine Bewegung
classic.pedia.describe.combat-ability-city-attack-only = kann nur Städte angreifen
classic.pedia.describe.combat-ability-capture-defeated-enemy = kann besiegte feindliche Einheiten gefangen nehmen
classic.pedia.describe.combat-ability-heavy-charge = stärkere Angriffe gegen Einheiten, die sich nicht zurückziehen können
classic.pedia.describe.combat-ability-sapper = hilft benachbarten eigenen Einheiten beim Angriff auf eine Stadt neben dieser Einheit
classic.pedia.describe.combat-ability-ignore-zone-of-control = ignoriert feindliche Kontrollzonen
classic.pedia.describe.combat-ability-embarked-to-land-flat-cost = die Landung vom Wasser kostet einen Bewegungspunkt
classic.pedia.describe.combat-ability-hills-ignore-terrain-cost = ignoriert Geländekosten beim Betreten von Hügeln
classic.pedia.describe.combat-ability-cross-mountains = kann Berge überqueren
classic.pedia.describe.combat-ability-road-in-friendly-land = bewegt sich im eigenen Gebiet auf dem angegebenen Gelände wie auf einer Straße
classic.pedia.describe.combat-ability-invisible = nur neben feindlichen Einheiten und Städten sichtbar oder für Einheiten, die U-Boote sehen
classic.pedia.describe.combat-ability-see-invisible = sieht U-Boote in Sichtweite
classic.pedia.describe.inert-amount = { $subject }: { $amount }
classic.pedia.describe.inert-refs = { $text } ({ $names })
classic.pedia.describe.inert-trigger = { $trigger }: { $text }
classic.pedia.describe.building-ideology-choice = Mit { $n } dieser Gebäude können Sie eine Ideologie wählen
classic.pedia.describe.building-free-great-work = Enthält ein kostenloses Großes Kunstwerk
classic.pedia.describe.building-nullify-influence = Schützt Ihre Nation vor dem Tourismusbonus, den andere Nationen durch das Internet erhalten
classic.pedia.describe.building-missionary-extra-spreads = Hier gekaufte Missionare können die Religion { $n ->
    [one] { $n } Mal zusätzlich
   *[other] { $n } Mal zusätzlich
} verbreiten
classic.pedia.describe.building-religious-pressure = +{ $n } % religiöser Druck von dieser Stadt
classic.pedia.describe.building-food-trade-routes = Diese Stadt kann Nahrungs-Handelsrouten zu Ihren anderen Städten senden
classic.pedia.describe.building-production-trade-routes = Diese Stadt kann Produktions-Handelsrouten zu Ihren anderen Städten senden
classic.pedia.describe.building-counter-espionage = Bietet Spionageabwehr in dieser Stadt
classic.pedia.describe.building-affect-spies-now = Spione, die hier bereits Informationen sammeln, spüren seine Spionagewirkung sofort
classic.pedia.describe.building-border-obstacle = Feindliche Landeinheiten zahlen in Ihrem Gebiet 1 zusätzlichen Bewegungspunkt pro Feld
classic.pedia.describe.building-airlift = Landeinheiten können von oder zu dieser Stadt eingeflogen werden
classic.pedia.describe.building-allows-range-strike = Erlaubt dieser Stadt Fernangriffe
classic.pedia.describe.unknown = Unbekannt
classic.pedia.describe-classic.trigger-on-adopt = bei Übernahme
classic.pedia.describe-classic.trigger-wonder-found-first = beim ersten Entdecken eines Naturwunders
classic.pedia.describe-classic.trigger-wonder-found = beim Entdecken eines Naturwunders, das eine andere Nation zuvor gefunden hat
classic.pedia.describe-classic.trigger-long-count = am Ende jedes Zyklus der Langen Zählung
classic.pedia.describe-classic.trigger-great-person-expended = wenn eine Große Persönlichkeit eingesetzt wird
classic.pedia.describe-classic.trigger-city-converted = wenn eine Stadt erstmals zur Religion übertritt
classic.pedia.describe-classic.trigger-religion-spread = wenn ein Missionar oder Prophet die Religion verbreitet
classic.pedia.describe-classic.trigger-great-person-born = wenn eine Große Persönlichkeit geboren wird
classic.pedia.describe-classic.selector-following-cities = in Städten, die der Religion folgen
classic.pedia.describe-classic.selector-following-city-tiles = auf Feldern von Städten, die der Religion folgen
classic.pedia.describe-classic.selector-holy-city = in der Heiligen Stadt
classic.pedia.describe-classic.selector-founder = für den Gründer
classic.pedia.describe-classic.selector-own-religion = für die Religion
classic.pedia.describe-classic.condition-barbarian = gegen Barbaren
classic.pedia.describe-classic.condition-city-state = gegen Stadtstaaten
classic.pedia.describe-classic.condition-more-cities = gegen Nationen mit mehr Städten
classic.pedia.describe-classic.condition-min-followers = ab { $n ->
    [one] { $n } Anhänger
   *[other] { $n } Anhängern
}
classic.pedia.describe-classic.condition-near-own-following-city = in der Nähe eigener Städte, die der Religion folgen
classic.pedia.describe-classic.condition-near-enemy-following-city = in der Nähe feindlicher Städte, die der Religion folgen
classic.pedia.describe-classic.condition-golden-age = während eines Goldenen Zeitalters
classic.pedia.describe-classic.condition-great-general-earned = sobald ein Großer General erworben wurde
classic.pedia.describe-classic.condition-great-person-is = wenn es ein { $kind } ist
classic.pedia.describe-classic.modifier-happiness = { $amount } Zufriedenheit
classic.pedia.describe-classic.modifier-great-person-points = { $amount } Punkte ({ $kind })
classic.pedia.describe-classic.modifier-free-policies = { $count ->
    [one] { $count } kostenlose Sozialpolitik
   *[other] { $count } kostenlose Sozialpolitiken
}
classic.pedia.describe-classic.modifier-yield-per-following-city = { $foreign ->
    [true] { $amount } { $yield } pro fremder Stadt, die der Religion folgt
   *[false] { $amount } { $yield } pro Stadt, die der Religion folgt
}
classic.pedia.describe-classic.modifier-yield-per-followers = { $foreign ->
    [true] +1 { $yield } pro { $per } ausländische Anhänger
   *[false] +1 { $yield } pro { $per } Anhänger
}
classic.pedia.describe-classic.modifier-happiness-per-following-city = { $amount } Zufriedenheit pro Stadt, die der Religion folgt
classic.pedia.describe-classic.modifier-religion = { $percent } % { $stat }
classic.pedia.describe-classic.modifier-faith-building = { $name } kann mit Glaube gekauft werden
classic.pedia.describe-classic.modifier-faith-units = Landeinheiten (Zeitalter: { $era }) können mit Glaube gekauft werden
classic.pedia.describe-classic.modifier-reformation-belief = ein Reformations-Glaubenssatz kann hinzugefügt werden
classic.pedia.describe-classic.modifier-tourism-toward = { $percent } % Tourismus gegenüber Nationen { $when }
classic.pedia.describe-classic.modifier-happiness-per-count = { $amount } Zufriedenheit { $per }
classic.pedia.describe-classic.modifier-grant-spies = { $count ->
    [one] ein kostenloser Spion
   *[other] { $count } kostenlose Spione
}
classic.pedia.describe-classic.modifier-promote-spies = { $ranks ->
    [one] jeder Spion steigt einen Rang auf
   *[other] jeder Spion steigt um { $ranks } Ränge auf
}
classic.pedia.describe-classic.modifier-extra-founding-belief = ein weiterer Glaubenssatz bei der Gründung einer Religion
classic.pedia.describe-classic.modifier-second-religion-pantheon = Städte erhalten auch den Pantheon-Glaubenssatz ihrer zweiten Religion
classic.pedia.describe-classic.modifier-instant-tourism = +{ $amount } Tourismus bei allen bekannten Nationen
classic.pedia.describe-classic.modifier-experience-per-great-work = hier ausgebildete Einheiten starten mit { $amount } Erfahrung pro Großem Werk in der Stadt
classic.pedia.describe-classic.grant-free-great-people = { $n ->
    [one] eine kostenlose Große Persönlichkeit Ihrer Wahl
   *[other] { $n } kostenlose Große Persönlichkeiten Ihrer Wahl
}
classic.pedia.describe-classic.grant-long-count-great-person = eine kostenlose Große Persönlichkeit einer zuvor nicht gewählten Art
classic.pedia.describe-classic.grant-golden-age = ein Goldenes Zeitalter von { $turns ->
    [one] { $turns } Zug
   *[other] { $turns } Zügen
}
classic.pedia.describe-classic.unit-stat-general-points = { $amount } % Punkte für Große Generäle aus Kämpfen
classic.pedia.describe-classic.unit-stat-stacked-general = { $amount } % Kampfstärke für eine Einheit, die das Feld mit diesem Großen General teilt
classic.pedia.describe-classic.unit-stat-maintenance = { $amount } % Gold-Unterhalt
classic.pedia.describe-classic.unit-stat-experience-needed = { $amount } % Erfahrung für eine Beförderung
classic.pedia.describe-classic.unit-stat-general-aura = { $amount } % Kampfbonus von Großen Generälen und Admirälen
classic.pedia.describe-classic.unit-stat-mission-gold = { $amount } % Gold aus einer Handelsmission
classic.pedia.describe-classic.unit-stat-mission-influence = { $amount } % Einfluss aus einer Handelsmission
classic.pedia.describe-classic.unit-stat-adjacent-damage-chance = { $amount } % Chance, benachbarte Feinde nach dem Angriff zu verletzen
classic.pedia.describe-classic.unit-stat-rival-land-loss = verliert für jeden Zug, den es in fremdem Gebiet beendet, religiöse Stärke
classic.pedia.describe-classic.unit-stat-golden-age-on-kill = Siege bringen Punkte für ein Goldenes Zeitalter, mehr bei stärkeren Gegnern
classic.pedia.describe-classic.ability-great-general = gibt nahen eigenen Landeinheiten den Kampfbonus eines Großen Generals
classic.pedia.describe-classic.ability-post-combat-promotions = wird nach dem ersten Kampf durch eine zufällige Beförderung ersetzt
classic.pedia.describe-classic.ability-lends-movement = ein Großer General, der zu Beginn des Zuges mit dieser Einheit auf einem Feld steht, bewegt sich mit ihrer Bewegungsrate
classic.pedia.describe-classic.ability-ruins-reward = nimmt beim Betreten Antiker Ruinen die beste Belohnung
classic.pedia.describe-classic.ability-exotic-cargo = transportiert exotische Waren, die einmal neben fremdem Land verkauft werden
classic.pedia.describe-classic.ability-great-admiral = gibt nahen eigenen Marineeinheiten den Kampfbonus eines Großen Admirals
classic.pedia.describe-classic.ability-heal-not-barbarians = die Heilung nach einem Sieg gilt nicht für Barbaren
classic.pedia.describe-classic.cost-policies = Kosten der Sozialpolitiken
classic.pedia.describe-classic.cost-policy-city-penalty = Sozialpolitikkosten pro Stadt
classic.pedia.describe-classic.cost-faith-purchase = Kaufkosten in Glaube
classic.pedia.describe-classic.count-policies = { $n ->
    [one] Sozialpolitik
   *[other] Sozialpolitiken
}
classic.pedia.describe-classic.count-majority-followers = { $n ->
    [one] Anhänger
   *[other] Anhänger
}
classic.pedia.describe-classic.count-religions-present = { $n ->
    [one] vorhandene Religion
   *[other] vorhandene Religionen
}
classic.pedia.describe-classic.count-peaceful-foreign-followers = { $n ->
    [one] Anhänger in einer fremden Stadt, mit der kein Krieg herrscht
   *[other] Anhänger in fremden Städten, mit denen kein Krieg herrscht
}
classic.pedia.describe-classic.count-other-religion-followers = { $n ->
    [one] Anhänger anderer Religionen
   *[other] Anhänger anderer Religionen
}
classic.pedia.describe-classic.count-trade-partners = { $n ->
    [one] Handelspartner
   *[other] Handelspartner
}
classic.pedia.describe-classic.count-city-state-routes = { $n ->
    [one] Handelsroute zu einem Stadtstaat
   *[other] Handelsrouten zu Stadtstaaten
}
classic.pedia.describe-classic.league-happiness = +{ $n } Zufriedenheit
classic.pedia.describe-classic.league-free-policies = { $n ->
    [one] { $n } kostenlose Sozialpolitik
   *[other] { $n } kostenlose Sozialpolitiken
}
classic.pedia.describe-classic.league-golden-age-points = +{ $n } Punkte für ein Goldenes Zeitalter
classic.pedia.describe-classic.league-culture-boost = +100 % Kultur für { $n ->
    [one] { $n } Zug
   *[other] { $n } Züge
}
classic.pedia.describe-classic.league-tourism-boost = +100 % Tourismus für { $n ->
    [one] { $n } Zug
   *[other] { $n } Züge
}
classic.pedia.describe-classic.league-city-state-influence = +{ $n } Einfluss bei jedem bekannten Stadtstaat
classic.pedia.describe-classic.league-research-turns = Wissenschaft { $n ->
    [one] des letzten Zuges
   *[other] der letzten { $n } Züge
}
classic.pedia.describe-classic.league-free-unit = { $name } gratis in der Hauptstadt
classic.pedia.describe-classic.league-building = { $name } in der Hauptstadt
classic.pedia.describe-classic.city-state-resting-point = { $amount } Ruhewert des Einflusses bei Stadtstaaten
classic.pedia.describe-classic.city-state-decay = { $amount } % Einflussverlust bei Stadtstaaten
classic.pedia.describe-classic.city-state-gold-gift = { $amount } % Einfluss durch Goldgeschenke an Stadtstaaten
classic.pedia.describe-classic.city-state-bully-score = { $amount } % Militärmacht beim Fordern von Tribut
classic.pedia.describe-classic.city-state-unit-gift-influence = { $amount } Einfluss pro an einen Stadtstaat geschenkter Einheit
classic.pedia.describe-classic.city-state-friend-bonus = { $amount } % Nahrung, Kultur und Glaube von befreundeten Stadtstaaten
classic.pedia.describe-classic.city-state-friendship-rate = { $amount } % Einflusserholung bei Stadtstaaten; { $decay } % Einflussverlust
classic.pedia.describe-classic.city-state-annex = kann Stadtstaaten aufkaufen, die fünf Züge lang verbündet sind
classic.pedia.describe-classic.city-state-resting-point-religion = { $amount } Ruhewert des Einflusses bei Stadtstaaten, die der Religion folgen
classic.pedia.describe-classic.city-state-afraid-influence = { $amount } Einfluss pro Zug bei Stadtstaaten, die Sie fürchten
classic.pedia.describe-classic.city-state-trade-route-influence = { $amount } Einfluss pro Zug bei Stadtstaaten, mit denen Sie eine Handelsroute haben
classic.pedia.describe-classic.city-state-unit-gift-rate = militaristische Stadtstaaten schenken Einheiten { $amount } % schneller, solange Sie einen gemeinsamen Feind haben
classic.pedia.describe-classic.city-state-ally-resources = verbündete Stadtstaaten geben doppelt so viele Ressourcen, ihre Luxusgüter 50 % mehr Zufriedenheit
classic.pedia.describe-classic.city-state-ally-great-people = verbündete Stadtstaaten schenken gelegentlich eine Große Persönlichkeit
classic.pedia.describe-classic.city-state-ally-science = ein Viertel der Wissenschaft verbündeter Stadtstaaten
classic.pedia.describe-classic.city-state-great-person-gift = { $amount } Einfluss pro an einen Stadtstaat geschenkter Großer Persönlichkeit
classic.pedia.describe-classic.city-state-mission-gold = { $amount } % Gold aus Handelsmissionen
classic.pedia.describe-classic.great-artist = Großer Künstler
classic.pedia.describe-classic.great-engineer = Großer Ingenieur
classic.pedia.describe-classic.great-merchant = Großer Händler
classic.pedia.describe-classic.great-scientist = Großer Wissenschaftler
classic.pedia.describe-classic.great-writer = Großer Schriftsteller
classic.pedia.describe-classic.great-musician = Großer Musiker
classic.pedia.describe-classic.great-general = Großer General
classic.pedia.describe-classic.great-admiral = Großer Admiral
classic.pedia.describe-classic.great-prophet = Großer Prophet
classic.pedia.describe-classic.religion-pressure-out = religiöser Druck
classic.pedia.describe-classic.religion-spread-distance = religiöse Verbreitungsreichweite
classic.pedia.describe-classic.religion-missionary-strength = Missionarsstärke
classic.pedia.describe-classic.religion-prophet-strength = Stärke Großer Propheten
classic.pedia.describe-classic.religion-prophet-cost = Glaubenskosten Großer Propheten
classic.pedia.describe-classic.religion-missionary-cost = Glaubenskosten von Missionaren
classic.pedia.describe-classic.religion-friendly-city-state-pressure = religiöser Druck auf befreundete Stadtstaaten
classic.pedia.describe-classic.religion-other-erosion = des Drucks eines Missionars werden anderen Religionen entzogen
classic.pedia.describe-classic.religion-pressure-retention = ihres Drucks bleiben erhalten, wenn Inquisitoren oder Propheten sie entfernen
classic.pedia.describe-classic.religion-converts-barbarians = Missionare bekehren benachbarte barbarische Einheiten
classic.pedia.describe-classic.religion-faith-great-people = jede Große Persönlichkeit kann ab dem Industriezeitalter mit Glaube gekauft werden
classic.pedia.describe-classic.tourism-flat = { $amount } Tourismus
classic.pedia.describe-classic.tourism-percent = { $amount } % Tourismus
classic.pedia.describe-classic.tourism-great-works = { $amount } % Tourismus aus Großen Werken
classic.pedia.describe-classic.tourism-landmarks = { $amount } % Tourismus aus Wahrzeichen und Wundern
classic.pedia.describe-classic.tourism-per-faith-building = { $amount } Tourismus pro Glaubensgebäude
classic.pedia.describe-classic.tourism-spread = { $amount } % Tourismusverbreitung
classic.pedia.describe-classic.tourism-theming = { $amount } % Themenboni
classic.pedia.describe-classic.tourism-great-work-culture = { $amount } Kultur pro Großem Werk
classic.pedia.describe-classic.tourism-shared-religion = mit gleicher Religion
classic.pedia.describe-classic.tourism-trade-route = mit Handelsroute zu Ihnen
classic.pedia.describe-classic.tourism-open-borders = mit Offenen Grenzen
classic.pedia.describe-classic.tourism-same-ideology = Ihrer Ideologie
classic.pedia.describe-classic.tourism-less-happy = mit weniger Zufriedenheit als Sie
classic.pedia.describe-classic.tourism-common-enemy = im Krieg mit Ihren Feinden
