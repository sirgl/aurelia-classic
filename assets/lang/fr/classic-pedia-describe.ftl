# Translation of classic-pedia-describe.ftl into fr. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.pedia.describe.join = { $a }, { $b }
classic.pedia.describe.join-or = { $a } ou { $b }
classic.pedia.describe.yield-amount = { $amount } { $yield }
classic.pedia.describe.yield-output = production
classic.pedia.describe.yield-food = Nourriture
classic.pedia.describe.yield-production = Production
classic.pedia.describe.yield-gold = Or
classic.pedia.describe.yield-science = Science
classic.pedia.describe.yield-culture = Culture
classic.pedia.describe.yield-faith = Foi
classic.pedia.describe.with-trigger = { $trigger } : { $text }
classic.pedia.describe.with-selector = { $text } { $selector }
classic.pedia.describe.with-condition = { $text } { $condition }
classic.pedia.describe.with-second-condition = { $text } et { $condition }
classic.pedia.describe.with-duration = { $text } pendant { $turns ->
    [one] { $turns } tour
   *[other] { $turns } tours
}
classic.pedia.describe.trigger-on-built = à la construction
classic.pedia.describe.trigger-on-turn-start = à chaque tour
classic.pedia.describe.trigger-on-technology-unlocked = à son acquisition
classic.pedia.describe.trigger-on-city-founded = à la fondation d'une ville
classic.pedia.describe.trigger-on-unit-killed = à chaque unité tuée
classic.pedia.describe.trigger-on-unit-trained = unités formées ici
classic.pedia.describe.trigger-on-city-conquered = à la conquête d'une ville
classic.pedia.describe.trigger-on-era-entered = en entrant dans une nouvelle ère
classic.pedia.describe.selector-source-city = dans cette ville
classic.pedia.describe.selector-owner-cities = dans chaque ville
classic.pedia.describe.selector-event-city = dans la ville concernée
classic.pedia.describe.selector-owner-units = pour chaque unité
classic.pedia.describe.selector-event-unit = pour l'unité concernée
classic.pedia.describe.selector-source-city-tiles = sur les cases exploitées par cette ville
classic.pedia.describe.selector-owner-city-tiles = sur les cases exploitées
classic.pedia.describe.selector-capital = dans la capitale
classic.pedia.describe.condition-has-technology = avec { $name }
classic.pedia.describe.condition-lacks-technology = jusqu'à { $name }
classic.pedia.describe.condition-min-population = à partir de { $n ->
    [one] { $n } citoyen
   *[other] { $n } citoyens
}
classic.pedia.describe.condition-has-building = avec le bâtiment { $name }
classic.pedia.describe.condition-on-tile = sur { $name }
classic.pedia.describe.condition-terrain-until = sur { $terrain } jusqu'à { $technology }
classic.pedia.describe.condition-against-role = contre les unités { $role }
classic.pedia.describe.condition-against-class = contre les unités de type { $name }
classic.pedia.describe.condition-for-units = pour les unités { $kind }
classic.pedia.describe.condition-near-city = dans un rayon de { $n ->
    [one] { $n } case
   *[other] { $n } cases
} autour de la ville
classic.pedia.describe.condition-resource-class = sur les ressources { $class }
classic.pedia.describe.condition-unimproved-feature = sur { $name } sans aménagement
classic.pedia.describe.condition-adjacent-unimproved-feature = avec au moins { $n } { $name } sans aménagement à côté de la ville
classic.pedia.describe.condition-producing = en produisant { $what }
classic.pedia.describe.condition-garrisoned = avec une garnison
classic.pedia.describe.condition-coastal = si la ville est côtière
classic.pedia.describe.condition-learned-technology = à l'acquisition de { $name }
classic.pedia.describe.condition-on-river = si la ville est sur une rivière
classic.pedia.describe.condition-at-peace = tant que vous êtes en paix avec toutes les nations
classic.pedia.describe.condition-connected = si la ville est reliée à la capitale
classic.pedia.describe.condition-near-improvement = dans un rayon de { $n ->
    [one] { $n } case
   *[other] { $n } cases
} autour de l'aménagement { $name }
classic.pedia.describe.plot-river = sur les cases de rivière
classic.pedia.describe.plot-sea = sur les cases de mer
classic.pedia.describe.plot-lake = sur les cases de lac
classic.pedia.describe.plot-sea-resource = sur les ressources marines
classic.pedia.describe.plot-natural-wonder = sur les merveilles naturelles
classic.pedia.describe.production-units = des unités
classic.pedia.describe.production-kind-units = des unités { $kind }
classic.pedia.describe.production-a = { $name }
classic.pedia.describe.production-military-units = des unités militaires
classic.pedia.describe.production-spaceship-parts = des pièces de vaisseau spatial
classic.pedia.describe.production-buildings = des bâtiments
classic.pedia.describe.production-world-wonders = des merveilles du monde
classic.pedia.describe.production-world-wonders-before = des merveilles du monde antérieures à « { $era } »
classic.pedia.describe.production-built-in-capital = des bâtiments que la capitale possède déjà
classic.pedia.describe.role-great-person = de grand personnage
classic.pedia.describe.role-religious = religieuses
classic.pedia.describe.role-civilian = civiles
classic.pedia.describe.role-spaceship-part = de pièce de vaisseau spatial
classic.pedia.describe.role-recon = de reconnaissance
classic.pedia.describe.role-melee = de mêlée
classic.pedia.describe.role-archer = d'archers
classic.pedia.describe.role-mounted = montées
classic.pedia.describe.role-siege = de siège
classic.pedia.describe.role-gun = d'armes à feu
classic.pedia.describe.role-armor = blindées
classic.pedia.describe.role-naval-melee = navales de mêlée
classic.pedia.describe.role-naval-ranged = navales à distance
classic.pedia.describe.role-submarine = sous-marines
classic.pedia.describe.role-carrier = porte-avions
classic.pedia.describe.role-fighter = de chasse
classic.pedia.describe.role-bomber = de bombardement
classic.pedia.describe.role-helicopter = d'hélicoptères
classic.pedia.describe.role-missile = de missiles
classic.pedia.describe.domain-land = terrestres
classic.pedia.describe.domain-sea = navales
classic.pedia.describe.domain-air = aériennes
classic.pedia.describe.resource-class-bonus = bonus
classic.pedia.describe.resource-class-strategic = stratégiques
classic.pedia.describe.resource-class-luxury = de luxe
classic.pedia.describe.resource-class-artifact = d'artefact
classic.pedia.describe.modifier-add-yield = { $amount } { $yield }
classic.pedia.describe.modifier-add-yield-percent = { $percent } % { $yield }
classic.pedia.describe.modifier-multiply-yield = ×{ $factor } { $yield }
classic.pedia.describe.modifier-growth = { $percent } % de croissance
classic.pedia.describe.modifier-cost = { $percent } % de { $target }
classic.pedia.describe.modifier-combat = { $percent } % de force de combat
classic.pedia.describe.modifier-combat-in = { $text } { $context }
classic.pedia.describe.modifier-movement = { $amount } mouvement
classic.pedia.describe.modifier-unlocks = débloque { $name }
classic.pedia.describe.modifier-free-unit = { $count } × { $name } (gratuit)
classic.pedia.describe.modifier-free-building = { $name } (gratuit)
classic.pedia.describe.modifier-promotion = la promotion { $name }
classic.pedia.describe.modifier-free-building-in-cities = { $count ->
    [one] { $name } (gratuit) dans la première ville
   *[other] { $name } (gratuit) dans les { $count } premières villes
}
classic.pedia.describe.modifier-yield-per-count = { $amount } { $yield } { $per }
classic.pedia.describe.modifier-yield-percent-per-count = { $percent } % { $yield } { $per }
classic.pedia.describe.modifier-yield-from-kill = { $yield } pour chaque unité ennemie détruite, davantage pour les plus fortes
classic.pedia.describe.modifier-forbid-unit = ne peut ni former ni acheter d'unités { $name }
classic.pedia.describe.modifier-forbid-annexing = ne peut pas annexer de villes
classic.pedia.describe.modifier-post-combat-promotion = peut obtenir la promotion { $name } après son premier combat
classic.pedia.describe.modifier-passive-promotion = les unités éligibles reçoivent les effets de la promotion { $name }
classic.pedia.describe.modifier-experience = +{ $amount } points d'expérience
classic.pedia.describe.per-count-one = par { $unit }
classic.pedia.describe.per-count-many = par { $every } { $unit }
classic.pedia.describe.per-count-capped = { $per } (au maximum { $max })
classic.pedia.describe.count-population = { $n ->
    [one] citoyen
   *[other] citoyens
}
classic.pedia.describe.count-world-wonders = { $n ->
    [one] merveille du monde
   *[other] merveilles du monde
}
classic.pedia.describe.count-connected-cities = { $n ->
    [one] ville reliée à la capitale
   *[other] villes reliées à la capitale
}
classic.pedia.describe.per-population-step = { $n ->
    [one] +1 { $yield } par citoyen
   *[other] +1 { $yield } par { $n } citoyens
}
classic.pedia.describe.per-population = { $amount } { $yield } par citoyen
classic.pedia.describe.cost-units = coût de production des unités
classic.pedia.describe.cost-buildings = coût de production des bâtiments
classic.pedia.describe.cost-technologies = coût des technologies
classic.pedia.describe.cost-tile-purchase = coût d'achat des cases
classic.pedia.describe.cost-hurry = coût d'achat
classic.pedia.describe.cost-border-growth = culture requise pour l'expansion des frontières
classic.pedia.describe.cost-border-growth-exponent = hausse du coût d'expansion des frontières
classic.pedia.describe.cost-unit-upgrade = coût d'amélioration des unités
classic.pedia.describe.cost-unit-purchase = coût en or de l'achat d'unités
classic.pedia.describe.cost-building-purchase = coût en or de l'achat de bâtiments
classic.pedia.describe.context-attack = à l'attaque
classic.pedia.describe.context-defense = en défense
classic.pedia.describe.context-vs-city = contre les villes
classic.pedia.describe.context-attack-open = à l'attaque en terrain dégagé
classic.pedia.describe.context-attack-rough = à l'attaque en terrain accidenté
classic.pedia.describe.context-defense-open = en défense en terrain dégagé
classic.pedia.describe.context-defense-rough = en défense en terrain accidenté
classic.pedia.describe.context-ranged-attack-open = en attaque à distance en terrain dégagé
classic.pedia.describe.context-ranged-attack-rough = en attaque à distance en terrain accidenté
classic.pedia.describe.context-defense-vs-ranged = en défense contre les attaques à distance
classic.pedia.describe.context-ranged-attack = en attaque à distance
classic.pedia.describe.context-vs-wounded = contre les unités blessées
classic.pedia.describe.context-vs-fortified = contre les unités fortifiées
classic.pedia.describe.context-friendly-land = en territoire ami
classic.pedia.describe.context-outside-friendly-land = hors du territoire ami
classic.pedia.describe.context-attack-friendly-land = à l'attaque en territoire ami
classic.pedia.describe.context-flanking = par unité en position de flanc
classic.pedia.describe.context-adjacent-friendly = à côté d'une unité amie
classic.pedia.describe.context-near-capital = près de la capitale
classic.pedia.describe.context-embarked-defense = en défense lorsque l'unité est embarquée
classic.pedia.describe.context-interception = lors d'une interception
classic.pedia.describe.context-air-sweep = lors des balayages aériens
classic.pedia.describe.city-combat-defense = { $amount } de force de combat de la ville grâce aux défenses
classic.pedia.describe.city-combat-hit-points = { $amount } points de vie de la ville
classic.pedia.describe.city-combat-defense-percent = { $amount } % de force défensive grâce aux bâtiments
classic.pedia.describe.city-combat-garrison-ranged = { $amount } % de force d'attaque à distance de la ville avec une garnison
classic.pedia.describe.city-combat-nuke-damage = { $amount } % de dégâts reçus des attaques nucléaires
classic.pedia.describe.city-combat-air-capacity = { $amount } de capacité d'accueil d'avions
classic.pedia.describe.city-combat-ranged = { $amount } % de force d'attaque à distance de la ville
classic.pedia.describe.espionage-enemy-steal = { $amount } % de vitesse de vol de technologies des espions ennemis
classic.pedia.describe.espionage-steal = { $amount } % de vitesse de vol de technologies de vos espions
classic.pedia.describe.espionage-catch = { $amount } % de chances de capturer les espions ennemis
classic.pedia.describe.espionage-rig-election = { $amount } % de voix lors du truquage des élections des cités-États
classic.pedia.describe.espionage-extra-spies = { $amount } espions avec le premier espion
classic.pedia.describe.espionage-starting-rank = les nouveaux espions débutent avec { $amount } rangs de plus
classic.pedia.describe.espionage-spy-pressure = les espions exercent une pression religieuse ×{ $amount } dans la ville où ils se trouvent
classic.pedia.describe.league-delegates = { $amount } délégués au Congrès mondial
classic.pedia.describe.league-delegates-per-diplomat = { $amount } délégués par diplomate dans une capitale étrangère
classic.pedia.describe.stat-resource-copies = fournit { $amount } { $resource }
classic.pedia.describe.stat-resource-quantity = { $amount } % de { $resource } issu de vos cases
classic.pedia.describe.stat-strategic-quantity = { $amount } % de ressources stratégiques issues de vos cases
classic.pedia.describe.stat-extra-luxury-copies = { $amount } exemplaire(s) de chaque ressource de luxe autour de la ville
classic.pedia.describe.stat-great-people-rate = { $amount } % de grands personnages
classic.pedia.describe.stat-great-person-rate = { $amount } % de points de { $kind }
classic.pedia.describe.stat-food-kept = { $amount } % de nourriture conservée à la croissance
classic.pedia.describe.stat-specialist-yield = { $amount } { $yield } par spécialiste
classic.pedia.describe.stat-happiness-per-city = { $amount } bonheur par ville
classic.pedia.describe.stat-happiness-per-luxury = { $amount } bonheur par ressource de luxe
classic.pedia.describe.stat-natural-wonder-happiness = { $amount } % de bonheur des merveilles naturelles
classic.pedia.describe.stat-city-count-unhappiness = { $amount } % de mécontentement lié au nombre de villes
classic.pedia.describe.stat-population-unhappiness = { $amount } % de mécontentement lié à la population
classic.pedia.describe.stat-golden-age-length = { $amount } % de durée des âges d'or
classic.pedia.describe.stat-connection-gold = { $amount } % d'or des connexions entre villes
classic.pedia.describe.stat-research-agreement = { $amount } % de science des accords de recherche
classic.pedia.describe.stat-great-scientist-science = { $amount } % de science des grands scientifiques
classic.pedia.describe.stat-great-people-per-friend = { $amount } % de grands personnages par déclaration d'amitié
classic.pedia.describe.stat-free-maintenance-units = { $amount } unités sans entretien en or
classic.pedia.describe.stat-barbarian-conversion = { $amount } % de chances qu'une unité terrestre barbare vaincue dans son camp vous rejoigne et vous apporte 25 or
classic.pedia.describe.stat-plunder-gold = { $amount } % d'or des camps barbares et des villes capturées
classic.pedia.describe.stat-captured-gold = { $amount } % d'or pour qui capture cette ville
classic.pedia.describe.stat-worker-speed = { $amount } % de vitesse des ouvriers
classic.pedia.describe.stat-route-maintenance = { $amount } % d'entretien des routes et des chemins de fer
classic.pedia.describe.stat-any-specialist-yield = { $amount } { $yield } dans une ville avec un spécialiste
classic.pedia.describe.rule-garrison-free-maintenance = les unités en garnison dans les villes n'ont aucun entretien
classic.pedia.describe.rule-free-hills-route-maintenance = aucun entretien pour les routes et les chemins de fer sur les collines
classic.pedia.describe.rule-spaceship-part-purchase = les pièces de vaisseau spatial peuvent être achetées avec de l'or
classic.pedia.describe.rule-spaceship-part-hurry = les grands ingénieurs peuvent accélérer les pièces de vaisseau spatial
classic.pedia.describe.rule-see-barbarian-camps = les nouveaux camps barbares apparaissent sur les terres explorées
classic.pedia.describe.society-specialist-food = { $amount } % de nourriture consommée par les spécialistes
classic.pedia.describe.society-specialist-unhappiness = { $amount } % de mécontentement lié aux spécialistes
classic.pedia.describe.society-no-occupied-unhappiness = aucun mécontentement supplémentaire pendant l'occupation
classic.pedia.describe.society-wonder-culture = { $amount } % de culture des merveilles du monde
classic.pedia.describe.society-happiness-to-culture = le bonheur excédentaire est converti en culture
classic.pedia.describe.society-science-while-happy = { $amount } % de science tant que l'empire est heureux
classic.pedia.describe.society-luxury-retention = conserve { $amount } % du bonheur d'une ressource de luxe quand son dernier exemplaire est échangé
classic.pedia.describe.society-raze-speed = { $amount } % de vitesse de rasage
classic.pedia.describe.society-founding-territory = les nouvelles villes revendiquent { $amount } cases supplémentaires
classic.pedia.describe.society-capital-science-boost = les bâtiments scientifiques construits dans la capitale donnent un bonus de recherche
classic.pedia.describe.society-unique-luxury-cities = les { $amount } premières villes sur d'autres masses terrestres obtiennent une ressource de luxe unique
classic.pedia.describe.society-unique-luxury-quantity = { $amount } exemplaires de chaque ressource de luxe unique
classic.pedia.describe.society-unique-luxury-new-landmass = ressources de luxe uniques seulement sur les masses terrestres éloignées de la capitale
classic.pedia.describe.trade-routes = { $amount } routes commerciales
classic.pedia.describe.trade-range-tiles = { $amount } cases de portée des routes commerciales { $domain }
classic.pedia.describe.trade-range-percent = { $amount } % de portée des routes commerciales { $domain }
classic.pedia.describe.trade-route-gold = { $amount } or des routes commerciales { $domain }
classic.pedia.describe.trade-target-gold = { $amount } or pour les routes commerciales vers cette ville
classic.pedia.describe.trade-recipient-gold = { $amount } or pour cette ville issu des routes commerciales étrangères
classic.pedia.describe.trade-internal-percent = { $amount } % de nourriture et de production des routes commerciales internes
classic.pedia.describe.trade-routes-percent = { $amount } % de routes commerciales
classic.pedia.describe.trade-resource-gold-percent = { $amount } % d'or issu de la diversité des ressources sur les routes commerciales
classic.pedia.describe.trade-incoming-route-gold = { $amount } or pour qui vous envoie une route commerciale
classic.pedia.describe.trade-city-state-route-gold = { $amount } or des routes commerciales vers les cités-États
classic.pedia.describe.trade-shared-ideology-route-gold = { $amount } or des routes commerciales vers les nations de votre idéologie
classic.pedia.describe.trade-religious-pressure = { $amount } % de pression religieuse le long des routes commerciales
classic.pedia.describe.placement-land = Placé sur terre
classic.pedia.describe.placement-water = Émerge de la mer
classic.pedia.describe.placement-reef = Placé en eau libre près d'une côte
classic.pedia.describe.placement-peninsula-tip = Placé à la pointe d'une péninsule
classic.pedia.describe.placement-of-terrains = { $text } : { $terrains }
classic.pedia.describe.placement-away-from-coast = { $text }, loin de la côte
classic.pedia.describe.placement-off-largest-landmass = { $text }, hors de la plus grande masse terrestre
classic.pedia.describe.placement-sentence = { $text }.
classic.pedia.describe.adjacent-hills = collines
classic.pedia.describe.adjacent-mountains = montagnes
classic.pedia.describe.adjacent-hills-or-mountains = collines ou montagnes
classic.pedia.describe.adjacent-land = cases de terre
classic.pedia.describe.adjacent-water = cases d'eau
classic.pedia.describe.adjacent-at-least = Au moins { $n } { $what } voisines.
classic.pedia.describe.adjacent-none = Pas de { $what } voisines.
classic.pedia.describe.adjacent-at-most = Au plus { $n } { $what } voisines.
classic.pedia.describe.shape-mountain = La case devient une montagne.
classic.pedia.describe.shape-flat = La case devient une terre plate.
classic.pedia.describe.surrounded-by-sea = La mer l'entoure.
classic.pedia.describe.grant-free-technologies = { $n ->
    [one] une technologie gratuite de votre choix
   *[other] { $n } technologies gratuites de votre choix
}
classic.pedia.describe.grant-conquest-technology = une technologie gratuite connue de l'ancien propriétaire de la ville (une fois par ville)
classic.pedia.describe.grant-military-unit-copies = une copie de chacune de vos unités de combat terrestres
classic.pedia.describe.grant-population = { $n ->
    [one] { $amount } citoyen
   *[other] { $amount } citoyens
}
classic.pedia.describe.unit-stat-sight = { $amount } cases de vision
classic.pedia.describe.unit-stat-embarked-sight = { $amount } cases de vision lorsque l'unité est embarquée
classic.pedia.describe.unit-stat-range = { $amount } cases de portée d'attaque à distance
classic.pedia.describe.unit-stat-attacks-per-turn = { $amount } attaques par tour
classic.pedia.describe.unit-stat-cargo = { $amount } de capacité de transport d'avions ou de missiles
classic.pedia.describe.unit-stat-intercept-chance = { $amount } points de pourcentage de chances d'interception
classic.pedia.describe.unit-stat-interceptions-per-turn = { $amount } interceptions par tour
classic.pedia.describe.unit-stat-evasion = { $amount } points de pourcentage d'esquive des interceptions
classic.pedia.describe.unit-stat-interception-damage = { $amount } % de dégâts reçus des interceptions
classic.pedia.describe.unit-stat-paradrop-range = { $amount } cases de portée de parachutage
classic.pedia.describe.unit-stat-naval-moves = { $amount } mouvement en mer
classic.pedia.describe.unit-stat-experience = { $amount } % d'expérience gagnée
classic.pedia.describe.unit-stat-withdraw-chance = { $amount } points de pourcentage de chances de retraite avant un combat de mêlée
classic.pedia.describe.unit-stat-city-attack-plunder = rapporte de l'or en attaquant des villes, davantage selon les dégâts
classic.pedia.describe.unit-stat-heal-enemy-land = { $amount } PV soignés par tour en territoire ennemi
classic.pedia.describe.unit-stat-heal-neutral-land = { $amount } PV soignés par tour en territoire neutre
classic.pedia.describe.unit-stat-heal-friendly-land = { $amount } PV soignés par tour en territoire ami
classic.pedia.describe.unit-stat-heal-adjacent = les unités amies adjacentes récupèrent { $amount } PV par tour
classic.pedia.describe.unit-stat-heal-on-kill = { $amount } PV soignés après la destruction d'une unité ennemie
classic.pedia.describe.unit-stat-enemy-aura = { $amount } % de force de combat pour les unités ennemies proches
classic.pedia.describe.unit-stat-aura-range = { $amount } cases de portée de l'effet des ennemis proches
classic.pedia.describe.unit-stat-adjacent-damage = { $amount } de dégâts aux unités ennemies adjacentes après une attaque
classic.pedia.describe.unit-stat-capital-defense-falloff = modifie la vitesse à laquelle le bonus de défense de la capitale s'estompe avec la distance
classic.pedia.describe.unit-stat-embarked-moves = { $amount } mouvement lorsque l'unité est embarquée
classic.pedia.describe.unit-stat-wounded-damage = { $amount } points de pourcentage au modificateur de dégâts des unités blessées
classic.pedia.describe.ability-found-religion = Peut fonder ou améliorer une religion
classic.pedia.describe.ability-spread-religion = Peut propager sa religion dans une ville
classic.pedia.describe.ability-religion-spreads = Peut propager la religion { $n ->
    [one] { $n } fois
   *[other] { $n } fois
}
classic.pedia.describe.ability-religious-strength = Force religieuse : { $n }
classic.pedia.describe.ability-remove-heresy = Peut chasser les autres religions d'une ville
classic.pedia.describe.ability-prohibits-spread = Sa présence empêche la propagation des autres religions
classic.pedia.describe.ability-requires-enhanced-religion = Ne peut être acheté qu'une fois votre religion améliorée
classic.pedia.describe.ability-one-shot-tourism = Un concert rapporte un tourisme qui augmente avec votre tourisme actuel
classic.pedia.describe.ability-one-shot-tourism-others = Les autres nations reçoivent { $n } % du tourisme qu'apporte un concert
classic.pedia.describe.ability-culture-turns = Accorde de la culture proportionnelle à votre production de culture récente
classic.pedia.describe.ability-excavates = Peut fouiller les sites antiques
classic.pedia.describe.ability-food = Pendant sa formation, le surplus de nourriture de la ville s'ajoute à la production
classic.pedia.describe.ability-golden-age-turns = Peut déclencher un âge d'or d'une durée de base de { $n ->
    [one] { $n } tour
   *[other] { $n } tours
}
classic.pedia.describe.ability-beakers-turns = Accorde une poussée de science proportionnelle à votre production récente
classic.pedia.describe.ability-base-hurry = Peut ajouter de la production à ce qu'une ville construit
classic.pedia.describe.ability-hurry-multiplier = Une ville plus grande reçoit ainsi plus de production
classic.pedia.describe.ability-base-gold = Les missions commerciales rapportent de l'or
classic.pedia.describe.ability-gold-per-era = Aux ères tardives, les missions commerciales rapportent davantage
classic.pedia.describe.ability-no-maintenance = N'a aucun entretien en or
classic.pedia.describe.ability-trade = Peut établir des routes commerciales
classic.pedia.describe.ability-faith-purchase-needs-belief = Achetable avec de la foi uniquement si une croyance débloque son ère
classic.pedia.describe.ability-mechanized = Une machine, pas des humains
classic.pedia.describe.ability-priority-low = Visée en dernier par les ennemis
classic.pedia.describe.ability-priority-early = Visée en premier par les ennemis en début de partie
classic.pedia.describe.ability-priority-high = Visée en premier par les ennemis
classic.pedia.describe.ability-conscription = Valeur de conscription : { $n }
classic.pedia.describe.ability-great-person = Un grand personnage
classic.pedia.describe.ability-stealth = Avion furtif : aucun porte-avions ne peut l'accueillir
classic.pedia.describe.ability-air-slots = Occupe { $n ->
    [one] { $n } emplacement aérien
   *[other] { $n } emplacements aériens
} dans une ville
classic.pedia.describe.ability-work-rate = Vitesse de travail : { $n }
classic.pedia.describe.ability-move-after-purchase = Peut se déplacer le tour de son achat
classic.pedia.describe.ability-no-bad-goodies = Les ruines antiques ne lui nuisent jamais
classic.pedia.describe.ability-purchase-only = Ne peut qu'être achetée, jamais formée
classic.pedia.describe.ability-buy-city-state = Transforme la cité-État où elle se trouve en marionnette de son propriétaire
classic.pedia.describe.ability-found-abroad = Ne fonde de villes que hors de la masse terrestre de la capitale
classic.pedia.describe.ability-exotic-goods = Vend { $n } cargaison(s) de marchandises exotiques près d'une terre étrangère
classic.pedia.describe.combat-ability-move-after-attacking = peut se déplacer après une attaque
classic.pedia.describe.combat-ability-always-heal = peut se soigner après un déplacement ou une attaque
classic.pedia.describe.combat-ability-embarkation = peut embarquer pour traverser l'eau
classic.pedia.describe.combat-ability-embarked-all-water = peut entrer en eau profonde lorsqu'elle est embarquée
classic.pedia.describe.combat-ability-move-impassable = peut entrer sur des terrains normalement infranchissables
classic.pedia.describe.combat-ability-ranged-support-fire = effectue un tir de soutien à distance avant le combat de mêlée
classic.pedia.describe.combat-ability-hills-double-move = paie la moitié du coût de mouvement habituel sur les collines
classic.pedia.describe.combat-ability-double-move = paie la moitié du coût de mouvement habituel sur le terrain indiqué
classic.pedia.describe.combat-ability-impassable = ne peut pas entrer sur le terrain indiqué
classic.pedia.describe.combat-ability-insta-heal = soigne instantanément l'unité au lieu d'ajouter une promotion durable
classic.pedia.describe.combat-ability-amphibious = aucune pénalité d'attaque lors d'un débarquement depuis l'eau ou d'une traversée de rivière
classic.pedia.describe.combat-ability-river-crossing = aucune pénalité d'attaque lors d'une traversée de rivière
classic.pedia.describe.combat-ability-indirect-fire = les attaques à distance ne nécessitent pas de ligne de vue
classic.pedia.describe.combat-ability-heal-outside-friendly = peut se soigner en mer hors du territoire ami
classic.pedia.describe.combat-ability-air-sweep = peut effectuer des balayages aériens
classic.pedia.describe.combat-ability-rival-territory = peut entrer en territoire rival sans frontières ouvertes
classic.pedia.describe.combat-ability-rough-terrain-ends-turn = entrer en terrain accidenté met fin au mouvement de cette unité
classic.pedia.describe.combat-ability-ignore-terrain-cost = ignore les coûts de mouvement supplémentaires du terrain
classic.pedia.describe.combat-ability-flat-movement-cost = chaque case a le même coût de mouvement, routes comprises
classic.pedia.describe.combat-ability-hovering = peut se déplacer sur terre et sur l'eau
classic.pedia.describe.combat-ability-no-capture = ne peut capturer ni villes ni unités
classic.pedia.describe.combat-ability-only-defensive = ne peut pas attaquer en mêlée
classic.pedia.describe.combat-ability-no-defensive-bonus = ne reçoit aucun bonus de défense de terrain ni de fortification
classic.pedia.describe.combat-ability-must-set-up = doit se mettre en position avant une attaque à distance
classic.pedia.describe.combat-ability-free-pillage-moves = piller ne coûte aucun mouvement
classic.pedia.describe.combat-ability-city-attack-only = ne peut attaquer que des villes
classic.pedia.describe.combat-ability-capture-defeated-enemy = peut capturer les unités ennemies vaincues
classic.pedia.describe.combat-ability-heavy-charge = attaques renforcées contre les unités qui ne peuvent pas battre en retraite
classic.pedia.describe.combat-ability-sapper = aide les unités amies adjacentes à attaquer une ville voisine de cette unité
classic.pedia.describe.combat-ability-ignore-zone-of-control = ignore les zones de contrôle ennemies
classic.pedia.describe.combat-ability-embarked-to-land-flat-cost = débarquer coûte un point de mouvement
classic.pedia.describe.combat-ability-hills-ignore-terrain-cost = ignore les coûts de terrain en montant sur les collines
classic.pedia.describe.combat-ability-cross-mountains = peut franchir les montagnes
classic.pedia.describe.combat-ability-road-in-friendly-land = se déplace sur le terrain indiqué, sur son propre territoire, comme sur une route
classic.pedia.describe.combat-ability-invisible = visible seulement à côté d'unités et de villes ennemies, ou par les unités qui détectent les sous-marins
classic.pedia.describe.combat-ability-see-invisible = voit les sous-marins dans son champ de vision
classic.pedia.describe.inert-amount = { $subject } : { $amount }
classic.pedia.describe.inert-refs = { $text } ({ $names })
classic.pedia.describe.inert-trigger = { $trigger } : { $text }
classic.pedia.describe.building-ideology-choice = Avec { $n } de ces bâtiments, vous pouvez choisir une idéologie
classic.pedia.describe.building-free-great-work = Comprend une grande œuvre d'art gratuite
classic.pedia.describe.building-nullify-influence = Protège votre nation du bonus de tourisme que les autres nations tirent d'Internet
classic.pedia.describe.building-missionary-extra-spreads = Les missionnaires achetés ici peuvent propager la religion { $n ->
    [one] { $n } fois de plus
   *[other] { $n } fois de plus
}
classic.pedia.describe.building-religious-pressure = +{ $n } % de pression religieuse depuis cette ville
classic.pedia.describe.building-food-trade-routes = Cette ville peut envoyer des routes commerciales de nourriture vers vos autres villes
classic.pedia.describe.building-production-trade-routes = Cette ville peut envoyer des routes commerciales de production vers vos autres villes
classic.pedia.describe.building-counter-espionage = Assure le contre-espionnage dans cette ville
classic.pedia.describe.building-affect-spies-now = Les espions qui collectent déjà des renseignements ici en ressentent aussitôt les effets d'espionnage
classic.pedia.describe.building-border-obstacle = Les unités terrestres ennemies dépensent 1 point de mouvement supplémentaire par case sur votre territoire
classic.pedia.describe.building-airlift = Des unités terrestres peuvent être transportées par avion depuis ou vers cette ville
classic.pedia.describe.building-allows-range-strike = Permet à cette ville d'effectuer des tirs à distance
classic.pedia.describe.unknown = Inconnu
classic.pedia.describe-classic.trigger-on-adopt = à l'adoption
classic.pedia.describe-classic.trigger-wonder-found-first = en découvrant une merveille naturelle en premier
classic.pedia.describe-classic.trigger-wonder-found = en découvrant une merveille naturelle déjà trouvée par une autre nation
classic.pedia.describe-classic.trigger-long-count = à la fin de chaque cycle du Compte long
classic.pedia.describe-classic.trigger-great-person-expended = quand un grand personnage est utilisé
classic.pedia.describe-classic.trigger-city-converted = quand une ville se convertit pour la première fois à la religion
classic.pedia.describe-classic.trigger-religion-spread = quand un missionnaire ou un prophète propage la religion
classic.pedia.describe-classic.trigger-great-person-born = à la naissance d'un grand personnage
classic.pedia.describe-classic.selector-following-cities = dans les villes qui suivent la religion
classic.pedia.describe-classic.selector-following-city-tiles = sur les cases des villes qui suivent la religion
classic.pedia.describe-classic.selector-holy-city = dans la ville sainte
classic.pedia.describe-classic.selector-founder = pour le fondateur
classic.pedia.describe-classic.selector-own-religion = pour la religion
classic.pedia.describe-classic.condition-barbarian = contre les barbares
classic.pedia.describe-classic.condition-city-state = contre les cités-États
classic.pedia.describe-classic.condition-more-cities = contre les nations ayant plus de villes
classic.pedia.describe-classic.condition-min-followers = à partir de { $n ->
    [one] { $n } fidèle
   *[other] { $n } fidèles
}
classic.pedia.describe-classic.condition-near-own-following-city = près de vos villes qui suivent la religion
classic.pedia.describe-classic.condition-near-enemy-following-city = près des villes ennemies qui suivent la religion
classic.pedia.describe-classic.condition-golden-age = pendant un âge d'or
classic.pedia.describe-classic.condition-great-general-earned = une fois qu'un grand général a été obtenu
classic.pedia.describe-classic.condition-great-person-is = si c'est un { $kind }
classic.pedia.describe-classic.modifier-happiness = { $amount } bonheur
classic.pedia.describe-classic.modifier-great-person-points = { $amount } points de { $kind }
classic.pedia.describe-classic.modifier-free-policies = { $count ->
    [one] { $count } politique gratuite
   *[other] { $count } politiques gratuites
}
classic.pedia.describe-classic.modifier-yield-per-following-city = { $foreign ->
    [true] { $amount } { $yield } par ville étrangère qui suit la religion
   *[false] { $amount } { $yield } par ville qui suit la religion
}
classic.pedia.describe-classic.modifier-yield-per-followers = { $foreign ->
    [true] +1 { $yield } par { $per } fidèles étrangers
   *[false] +1 { $yield } par { $per } fidèles
}
classic.pedia.describe-classic.modifier-happiness-per-following-city = { $amount } bonheur par ville qui suit la religion
classic.pedia.describe-classic.modifier-religion = { $percent } % de { $stat }
classic.pedia.describe-classic.modifier-faith-building = { $name } achetable avec de la foi
classic.pedia.describe-classic.modifier-faith-units = les unités terrestres de la période « { $era } » peuvent être achetées avec de la foi
classic.pedia.describe-classic.modifier-reformation-belief = une croyance de Réforme peut être ajoutée
classic.pedia.describe-classic.modifier-tourism-toward = { $percent } % de tourisme envers les nations { $when }
classic.pedia.describe-classic.modifier-happiness-per-count = { $amount } bonheur { $per }
classic.pedia.describe-classic.modifier-grant-spies = { $count ->
    [one] un espion gratuit
   *[other] { $count } espions gratuits
}
classic.pedia.describe-classic.modifier-promote-spies = { $ranks ->
    [one] chaque espion gagne un rang
   *[other] chaque espion gagne { $ranks } rangs
}
classic.pedia.describe-classic.modifier-extra-founding-belief = une croyance de plus à la fondation d'une religion
classic.pedia.describe-classic.modifier-second-religion-pantheon = les villes reçoivent aussi la croyance de panthéon de leur seconde religion
classic.pedia.describe-classic.modifier-instant-tourism = +{ $amount } tourisme avec chaque nation connue
classic.pedia.describe-classic.modifier-experience-per-great-work = les unités formées ici débutent avec { $amount } d'expérience par grande œuvre de la ville
classic.pedia.describe-classic.grant-free-great-people = { $n ->
    [one] un grand personnage gratuit de votre choix
   *[other] { $n } grands personnages gratuits de votre choix
}
classic.pedia.describe-classic.grant-long-count-great-person = un grand personnage gratuit d'un type jamais choisi
classic.pedia.describe-classic.grant-golden-age = un âge d'or de { $turns ->
    [one] { $turns } tour
   *[other] { $turns } tours
}
classic.pedia.describe-classic.unit-stat-general-points = { $amount } % de points de grand général au combat
classic.pedia.describe-classic.unit-stat-stacked-general = { $amount } % de force de combat pour une unité sur la même case que ce grand général
classic.pedia.describe-classic.unit-stat-maintenance = { $amount } % d'entretien en or
classic.pedia.describe-classic.unit-stat-experience-needed = { $amount } % d'expérience nécessaire pour une promotion
classic.pedia.describe-classic.unit-stat-general-aura = { $amount } % du bonus de combat des grands généraux et amiraux
classic.pedia.describe-classic.unit-stat-mission-gold = { $amount } % d'or d'une mission commerciale
classic.pedia.describe-classic.unit-stat-mission-influence = { $amount } % d'influence d'une mission commerciale
classic.pedia.describe-classic.unit-stat-adjacent-damage-chance = { $amount } % de chances d'endommager les ennemis adjacents après une attaque
classic.pedia.describe-classic.unit-stat-rival-land-loss = perd de la force religieuse à chaque tour terminé en territoire rival
classic.pedia.describe-classic.unit-stat-golden-age-on-kill = chaque victoire ajoute des points d'âge d'or, davantage contre les plus forts
classic.pedia.describe-classic.ability-great-general = procure le bonus de combat d'un grand général aux unités terrestres amies proches
classic.pedia.describe-classic.ability-post-combat-promotions = est remplacée par une promotion aléatoire après son premier combat
classic.pedia.describe-classic.ability-lends-movement = un grand général sur la même case au début du tour se déplace à sa vitesse
classic.pedia.describe-classic.ability-ruins-reward = choisit la meilleure des récompenses en entrant dans des ruines antiques
classic.pedia.describe-classic.ability-exotic-cargo = transporte des marchandises exotiques à vendre une fois près d'une terre étrangère
classic.pedia.describe-classic.ability-great-admiral = procure le bonus de combat d'un grand amiral aux unités navales amies proches
classic.pedia.describe-classic.ability-heal-not-barbarians = le soin après une victoire ne s'applique pas aux barbares
classic.pedia.describe-classic.cost-policies = coût des politiques
classic.pedia.describe-classic.cost-policy-city-penalty = coût des politiques par ville
classic.pedia.describe-classic.cost-faith-purchase = coût d'achat avec de la foi
classic.pedia.describe-classic.count-policies = { $n ->
    [one] politique
   *[other] politiques
}
classic.pedia.describe-classic.count-majority-followers = { $n ->
    [one] fidèle
   *[other] fidèles
}
classic.pedia.describe-classic.count-religions-present = { $n ->
    [one] religion présente
   *[other] religions présentes
}
classic.pedia.describe-classic.count-peaceful-foreign-followers = { $n ->
    [one] fidèle dans une ville étrangère qui n'est pas en guerre
   *[other] fidèles dans des villes étrangères qui ne sont pas en guerre
}
classic.pedia.describe-classic.count-other-religion-followers = { $n ->
    [one] fidèle d'une autre religion
   *[other] fidèles d'autres religions
}
classic.pedia.describe-classic.count-trade-partners = { $n ->
    [one] partenaire commercial
   *[other] partenaires commerciaux
}
classic.pedia.describe-classic.count-city-state-routes = { $n ->
    [one] route commerciale vers une cité-État
   *[other] routes commerciales vers des cités-États
}
classic.pedia.describe-classic.league-happiness = +{ $n } bonheur
classic.pedia.describe-classic.league-free-policies = { $n ->
    [one] { $n } politique gratuite
   *[other] { $n } politiques gratuites
}
classic.pedia.describe-classic.league-golden-age-points = +{ $n } points vers un âge d'or
classic.pedia.describe-classic.league-culture-boost = +100 % de culture pendant { $n ->
    [one] { $n } tour
   *[other] { $n } tours
}
classic.pedia.describe-classic.league-tourism-boost = +100 % de tourisme pendant { $n ->
    [one] { $n } tour
   *[other] { $n } tours
}
classic.pedia.describe-classic.league-city-state-influence = +{ $n } d'influence auprès de chaque cité-État rencontrée
classic.pedia.describe-classic.league-research-turns = science { $n ->
    [one] du dernier tour
   *[other] des { $n } derniers tours
}
classic.pedia.describe-classic.league-free-unit = { $name } (gratuit) dans la capitale
classic.pedia.describe-classic.league-building = { $name } dans la capitale
classic.pedia.describe-classic.city-state-resting-point = { $amount } d'influence au repos auprès des cités-États
classic.pedia.describe-classic.city-state-decay = { $amount } % de déclin d'influence auprès des cités-États
classic.pedia.describe-classic.city-state-gold-gift = { $amount } % d'influence grâce aux dons d'or aux cités-États
classic.pedia.describe-classic.city-state-bully-score = { $amount } % de puissance militaire pour exiger un tribut
classic.pedia.describe-classic.city-state-unit-gift-influence = { $amount } d'influence par unité offerte à une cité-État
classic.pedia.describe-classic.city-state-friend-bonus = { $amount } % de nourriture, de culture et de foi des cités-États amies
classic.pedia.describe-classic.city-state-friendship-rate = { $amount } % de regain d'influence auprès des cités-États ; { $decay } % de déclin d'influence
classic.pedia.describe-classic.city-state-annex = peut racheter les cités-États alliées depuis cinq tours
classic.pedia.describe-classic.city-state-resting-point-religion = { $amount } de point de repos d'influence auprès des cités-États qui suivent la religion
classic.pedia.describe-classic.city-state-afraid-influence = { $amount } d'influence par tour auprès des cités-États qui vous craignent
classic.pedia.describe-classic.city-state-trade-route-influence = { $amount } d'influence par tour auprès des cités-États avec lesquelles vous avez une route commerciale
classic.pedia.describe-classic.city-state-unit-gift-rate = les cités-États militaristes offrent des unités { $amount } % plus vite tant que vous avez un ennemi commun
classic.pedia.describe-classic.city-state-ally-resources = les cités-États alliées donnent le double de ressources, et leurs ressources de luxe procurent 50 % de bonheur en plus
classic.pedia.describe-classic.city-state-ally-great-people = les cités-États alliées offrent de temps à autre un grand personnage
classic.pedia.describe-classic.city-state-ally-science = un quart de la science des cités-États alliées
classic.pedia.describe-classic.city-state-great-person-gift = { $amount } d'influence par grand personnage offert à une cité-État
classic.pedia.describe-classic.city-state-mission-gold = { $amount } % d'or des missions commerciales
classic.pedia.describe-classic.great-artist = Grand artiste
classic.pedia.describe-classic.great-engineer = Grand ingénieur
classic.pedia.describe-classic.great-merchant = Grand marchand
classic.pedia.describe-classic.great-scientist = Grand scientifique
classic.pedia.describe-classic.great-writer = Grand écrivain
classic.pedia.describe-classic.great-musician = Grand musicien
classic.pedia.describe-classic.great-general = Grand général
classic.pedia.describe-classic.great-admiral = Grand amiral
classic.pedia.describe-classic.great-prophet = Grand prophète
classic.pedia.describe-classic.religion-pressure-out = pression religieuse
classic.pedia.describe-classic.religion-spread-distance = distance de propagation religieuse
classic.pedia.describe-classic.religion-missionary-strength = force des missionnaires
classic.pedia.describe-classic.religion-prophet-strength = force des grands prophètes
classic.pedia.describe-classic.religion-prophet-cost = coût en foi des grands prophètes
classic.pedia.describe-classic.religion-missionary-cost = coût en foi des missionnaires
classic.pedia.describe-classic.religion-friendly-city-state-pressure = pression religieuse sur les cités-États amies
classic.pedia.describe-classic.religion-other-erosion = la pression d'un missionnaire prélevée sur les autres religions
classic.pedia.describe-classic.religion-pressure-retention = la pression conservée quand des inquisiteurs ou des prophètes la retirent
classic.pedia.describe-classic.religion-converts-barbarians = les missionnaires convertissent les unités barbares adjacentes
classic.pedia.describe-classic.religion-faith-great-people = tout grand personnage peut être acheté avec de la foi à partir de l'ère industrielle
classic.pedia.describe-classic.tourism-flat = { $amount } tourisme
classic.pedia.describe-classic.tourism-percent = { $amount } % de tourisme
classic.pedia.describe-classic.tourism-great-works = { $amount } % de tourisme des grandes œuvres
classic.pedia.describe-classic.tourism-landmarks = { $amount } % de tourisme des sites et des merveilles
classic.pedia.describe-classic.tourism-per-faith-building = { $amount } tourisme par bâtiment de foi
classic.pedia.describe-classic.tourism-spread = { $amount } % de diffusion du tourisme
classic.pedia.describe-classic.tourism-theming = { $amount } % de bonus de thématisation
classic.pedia.describe-classic.tourism-great-work-culture = { $amount } culture par grande œuvre
classic.pedia.describe-classic.tourism-shared-religion = qui partagent votre religion
classic.pedia.describe-classic.tourism-trade-route = avec lesquelles vous avez une route commerciale
classic.pedia.describe-classic.tourism-open-borders = avec lesquelles vous avez des frontières ouvertes
classic.pedia.describe-classic.tourism-same-ideology = de votre idéologie
classic.pedia.describe-classic.tourism-less-happy = moins heureuses que vous
classic.pedia.describe-classic.tourism-common-enemy = en guerre contre vos ennemis
