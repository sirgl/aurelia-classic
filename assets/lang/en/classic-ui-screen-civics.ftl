# The Social Policies screen of a running game: branches, ideologies, tenets, adoption.

classic.ui.screen-civics.title = Social policies
classic.ui.screen-civics.status-ready = { $culture } culture  ·  +{ $per_turn } per turn  ·  next policy { $cost } culture, ready now
classic.ui.screen-civics.status-turns = { $culture } culture  ·  +{ $per_turn } per turn  ·  next policy { $cost } culture, { $n ->
    [one] in { $n } turn
   *[other] in { $n } turns
}
classic.ui.screen-civics.status-stalled = { $culture } culture  ·  +{ $per_turn } per turn  ·  next policy { $cost } culture, no culture income
classic.ui.screen-civics.free-policies = { $n ->
    [one] { $n } free policy
   *[other] { $n } free policies
}
classic.ui.screen-civics.free-tenets = { $n ->
    [one] { $n } free tenet
   *[other] { $n } free tenets
}
classic.ui.screen-civics.branches = BRANCHES
classic.ui.screen-civics.ideologies = IDEOLOGIES
classic.ui.screen-civics.status-adopted-tenets = { $n ->
    [one] ADOPTED · { $n } tenet
   *[other] ADOPTED · { $n } tenets
}
classic.ui.screen-civics.status-policies = { $adopted } / { $total } POLICIES
classic.ui.screen-civics.can-be-chosen = CAN BE CHOSEN
classic.ui.screen-civics.can-be-unlocked = CAN BE UNLOCKED
classic.ui.screen-civics.from-era = FROM THE { $era }
classic.ui.screen-civics.choose-ideology = Choose ideology
classic.ui.screen-civics.choose-ideology-free = Choose ideology (+{ $n } tenets)
classic.ui.screen-civics.level = LEVEL { $tier }
classic.ui.screen-civics.adopted = ADOPTED
classic.ui.screen-civics.available = AVAILABLE
classic.ui.screen-civics.locked-caps = LOCKED
classic.ui.screen-civics.branch-completed = BRANCH COMPLETED
classic.ui.screen-civics.complete-branch = COMPLETE THE BRANCH
classic.ui.screen-civics.role-opener = { $branch } · OPENER
classic.ui.screen-civics.role-policy = { $branch } · POLICY
classic.ui.screen-civics.tab-available = Available now · { $n }
classic.ui.screen-civics.branches-tab = Branches
classic.ui.screen-civics.opener = OPENER
classic.ui.screen-civics.finisher = FINISHER
classic.ui.screen-civics.needs-culture = Needs { $cost } culture ({ $have } now)
classic.ui.screen-civics.needs-policy = Needs { $name } first
classic.ui.screen-civics.adopt-culture = Adopt · { $n }
classic.ui.screen-civics.adopt-free = Adopt · free
classic.ui.screen-civics.unlock-culture = Unlock · { $n }
classic.ui.screen-civics.unlock-free = Unlock · free
classic.ui.screen-civics.choose = Choose
classic.ui.screen-civics.tag-tenets = Tenets: { $n }
classic.ui.screen-civics.tag-price = { $n } culture
classic.ui.screen-civics.tag-free = free
classic.ui.screen-civics.included = COMES WITH THE BRANCH
classic.ui.screen-civics.available-title = AVAILABLE NOW
classic.ui.screen-civics.nothing-now = Nothing to adopt right now
classic.ui.screen-civics.culture-bar = { $culture } of { $cost } culture for the next policy
classic.ui.screen-civics.keys-hint = Arrows: branch and policy · Enter: adopt · Space: switch list
classic.ui.screen-civics.more-offers = + { $n } more: pick them in the branches
classic.ui.screen-civics.role-ideology = { $branch } · IDEOLOGY
classic.ui.screen-civics.tag-progress = { $adopted }/{ $total }
