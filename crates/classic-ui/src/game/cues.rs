//! Command journal -> cues, as the observer may see them.
//!
//! A batch of applied commands is read after it ran, against the world after the batch:
//! units the batch destroyed are named by the `Destroyed` entries (persistent id, kind,
//! owner, tile), the others by the world. Nothing hidden from the observer becomes a
//! cue: other players' moves show only their visible part, actions in the fog none.
use std::collections::HashMap;

use classic_rules::classic::ClassicChange;
use classic_rules::command::Command;
use classic_rules::ids::{CityHandle, PlayerId, TileId, UnitHandle};
use classic_rules::knowledge::PlayerView;
use classic_rules::rules::content::{Domain, UnitRole};
use classic_rules::transaction::journal::{AppliedCommand, CityChange, PlayerChange, UnitChange};
use classic_rules::world::state::GameView;
use classic_rules::CompiledRuleset;

use client::shell::presentation::cue::{Attack, AttackStyle, CityStrike, Cue, Derived, Figure, Origin};
use render::gpu::flight::FlightClass;
use render::unit_models::Weapon;
use client::shell::presentation::motion::Sound;

/// Who looks and through which eyes.
pub struct Observer<'a> {
    pub player: PlayerId,
    view: PlayerView<'a>,
    game: GameView<'a>,
    rules: &'a CompiledRuleset,
}

impl<'a> Observer<'a> {
    pub fn new(game: GameView<'a>, rules: &'a CompiledRuleset, player: PlayerId) -> Self {
        let view = PlayerView::new(game.auth, game.derived, rules, player);
        Self { player, view, game, rules }
    }

    fn sees(&self, tile: TileId) -> bool {
        self.view.is_visible(tile)
    }
}

/// Units the batch names: alive ones from the world, destroyed ones from the journal.
struct Roster {
    gone: HashMap<UnitHandle, (Figure, TileId)>,
}

impl Roster {
    fn of(records: &[AppliedCommand]) -> Self {
        let mut gone = HashMap::new();
        for record in records {
            for change in &record.changes.unit_changes {
                if let UnitChange::Destroyed { unit, id, owner, tile, kind, .. } = *change {
                    let figure = Figure { id, kind: kind.0, owner: owner.0 };
                    gone.insert(unit, (figure, tile));
                }
            }
        }
        Self { gone }
    }

    fn figure(&self, game: GameView<'_>, unit: UnitHandle) -> Option<Figure> {
        if let Some(view) = game.auth.units().get(unit) {
            let core = view.core;
            return Some(Figure { id: core.id, kind: core.kind.0, owner: core.owner.0 });
        }
        self.gone.get(&unit).map(|(figure, _)| *figure)
    }

    /// Health in percent (before, after) of a unit that took `damage` hit points in the
    /// batch and still lives (the world shows it after the batch).
    fn health_change(&self, game: GameView<'_>, unit: UnitHandle, damage: Option<u16>) -> Option<[u8; 2]> {
        let after = game.auth.units().get(unit)?.core.hp;
        let before = after.saturating_add(damage?);
        let pct = |hp: u16| hp.clamp(1, 100) as u8;
        Some([pct(before), pct(after)])
    }

    /// Where the unit stands after the batch (or where it fell).
    fn tile(&self, game: GameView<'_>, unit: UnitHandle) -> Option<TileId> {
        if let Some(view) = game.auth.units().get(unit) {
            return Some(view.core.tile);
        }
        self.gone.get(&unit).map(|(_, tile)| *tile)
    }
}

/// Every cue of a batch of applied commands, in order.
pub fn derive(observer: &Observer<'_>, records: &[AppliedCommand]) -> Vec<Derived> {
    let roster = Roster::of(records);
    let mut out = Vec::new();
    for record in records {
        derive_record(observer, &roster, record, &mut out);
    }
    out
}

fn derive_record(observer: &Observer<'_>, roster: &Roster, record: &AppliedCommand, out: &mut Vec<Derived>) {
    let game = observer.game;
    let actor = acting_player(game, roster, &record.command);
    let origin = match actor {
        Some(player) if player == observer.player => Origin::Own,
        _ => Origin::Other,
    };
    let mut push = |cue: Cue, concerns: bool| {
        out.push(Derived { cue, origin, concerns_observer: concerns || origin == Origin::Own });
    };
    for (unit, path) in moves(record) {
        if let Some((cue, concerns)) = move_cue(observer, roster, &record.command, unit, path) {
            push(cue, concerns);
        }
    }
    if let Some((cue, concerns)) = nuke_cue(observer, roster, record) {
        push(cue, concerns);
    }
    if let Some((cue, concerns)) = city_strike_cue(observer, roster, record) {
        push(cue, concerns);
    }
    if let Some((cue, concerns)) = attack_cue(observer, roster, record) {
        push(cue, concerns);
    }
    for (cue, concerns) in unit_cues(observer, roster, record) {
        push(cue, concerns);
    }
    for (cue, concerns) in city_cues(observer, record) {
        push(cue, concerns);
    }
    for cue in announcements(observer, record, origin) {
        push(cue, true);
    }
}

/// The player whose command this is.
fn acting_player(game: GameView<'_>, roster: &Roster, command: &Command) -> Option<PlayerId> {
    let unit = match command {
        Command::MoveUnit { unit, .. }
        | Command::MarchTo { unit, .. }
        | Command::Fortify { unit }
        | Command::RangedAttack { unit, .. }
        | Command::SetUpRangedAttack { unit }
        | Command::Rebase { unit, .. }
        | Command::RebaseToCarrier { unit, .. }
        | Command::AirSweep { unit, .. }
        | Command::Nuke { unit, .. }
        | Command::Airlift { unit, .. }
        | Command::Paradrop { unit, .. }
        | Command::FoundReligion { unit, .. } => *unit,
        Command::AttackUnit { attacker, .. } | Command::AttackCity { attacker, .. } => *attacker,
        Command::FoundCity { settler } => *settler,
        Command::EndTurn { player }
        | Command::ChooseTechnology { player, .. }
        | Command::FoundPantheon { player, .. }
        | Command::AdoptPolicy { player, .. }
        | Command::UnlockPolicyBranch { player, .. } => return Some(*player),
        Command::CityStrike { city, .. } => return game.auth.cities().get(*city).map(|c| c.owner),
        _ => return None,
    };
    roster.figure(game, unit).map(|f| PlayerId(f.owner))
}

/// The steps of every unit that moved, in order: (unit, [from, to, to, ...]).
fn moves(record: &AppliedCommand) -> Vec<(UnitHandle, Vec<TileId>)> {
    let mut paths: Vec<(UnitHandle, Vec<TileId>)> = Vec::new();
    for change in &record.changes.unit_changes {
        let UnitChange::Moved { unit, from, to } = *change else {
            continue;
        };
        match paths.iter_mut().find(|(u, _)| *u == unit) {
            Some((_, path)) => path.push(to),
            None => paths.push((unit, vec![from, to])),
        }
    }
    paths
}

fn move_cue(observer: &Observer<'_>, roster: &Roster, command: &Command, unit: UnitHandle, path: Vec<TileId>) -> Option<(Cue, bool)> {
    let figure = roster.figure(observer.game, unit)?;
    let own = figure.owner == observer.player.0;
    if let Some(cue) = air_move_cue(observer, command, figure, &path) {
        return Some((cue, own));
    }
    let path = if own { path } else { visible_run(observer, &path) };
    if path.len() < 2 {
        return None;
    }
    Some((Cue::Move { unit: figure, path }, own))
}

/// Tiles between two places.
fn reach(observer: &Observer<'_>, a: TileId, b: TileId) -> u8 {
    observer.game.auth.map().dimensions.distance(a, b).unwrap_or(1).clamp(1, 60) as u8
}

/// The flier of a unit kind: aircraft, missiles (low and fast) or nuclear weapons.
pub fn flight_class(rules: &CompiledRuleset, kind: u16) -> FlightClass {
    let Some(def) = rules.units.get(kind as usize) else {
        return FlightClass::Plane;
    };
    match def.role {
        Some(UnitRole::Missile) if def.combat.nuke_level > 0 => FlightClass::Ballistic,
        Some(UnitRole::Missile) => FlightClass::Cruise,
        _ => FlightClass::Plane,
    }
}

/// The unit kind drawn as the transport plane of a paradrop or an airlift: the first
/// bomber of the rules.
pub fn transport_kind(rules: &CompiledRuleset) -> u16 {
    rules
        .units
        .iter()
        .position(|u| u.domain == Domain::Air && u.role == Some(UnitRole::Bomber))
        .unwrap_or(0) as u16
}

/// An aircraft changing base, or a land unit flown by airlift or dropped by parachute: a flight of its own.
fn air_move_cue(observer: &Observer<'_>, command: &Command, figure: Figure, path: &[TileId]) -> Option<Cue> {
    let (from, to) = (*path.first()?, *path.last()?);
    if from == to {
        return None;
    }
    let own = figure.owner == observer.player.0;
    let seen = own || observer.sees(from) || observer.sees(to);
    let def = observer.rules.units.get(figure.kind as usize)?;
    let reach = reach(observer, from, to);
    if matches!(command, Command::Airlift { .. }) {
        return seen.then(|| Cue::Airlift { unit: figure, from, to, reach, plane: transport_kind(observer.rules) });
    }
    if matches!(command, Command::Paradrop { .. }) {
        return seen.then(|| Cue::Paradrop { unit: figure, from, to, reach, plane: transport_kind(observer.rules) });
    }
    if def.domain != Domain::Air {
        return None;
    }
    seen.then(|| Cue::Flight { unit: figure, from, to, reach, class: flight_class(observer.rules, figure.kind) })
}

/// A nuclear launch: the weapon flies to the target tile and the blast follows.
fn nuke_cue(observer: &Observer<'_>, roster: &Roster, record: &AppliedCommand) -> Option<(Cue, bool)> {
    let Command::Nuke { unit, target } = record.command else {
        return None;
    };
    let game = observer.game;
    let figure = roster.figure(game, unit)?;
    let from = roster.tile(game, unit)?;
    let own = figure.owner == observer.player.0;
    if !(own || observer.sees(from) || observer.sees(target)) {
        return None;
    }
    let dims = game.auth.map().dimensions;
    let blast = classic_rules::math::tiles_in_hex_radius(&dims, target, classic_rules::combat::NUKE_BLAST_RADIUS);
    let cue = Cue::Nuke { unit: figure, from, target, reach: reach(observer, from, target), blast };
    Some((cue, own || city_owner(game, target) == Some(observer.player)))
}

/// The visible stretch of another player's path (from its first to its last visible tile).
fn visible_run(observer: &Observer<'_>, path: &[TileId]) -> Vec<TileId> {
    let first = path.iter().position(|t| observer.sees(*t));
    let last = path.iter().rposition(|t| observer.sees(*t));
    match (first, last) {
        (Some(first), Some(last)) => path[first..=last].to_vec(),
        _ => Vec::new(),
    }
}

fn attack_cue(observer: &Observer<'_>, roster: &Roster, record: &AppliedCommand) -> Option<(Cue, bool)> {
    let game = observer.game;
    let (attacker, defender, target) = match record.command {
        Command::AttackUnit { attacker, defender } => (attacker, Some(defender), roster.tile(game, defender)?),
        Command::AttackCity { attacker, city } => (attacker, None, city_tile(game, city)?),
        // A ranged attack names a tile: the defender is the unit it hit
        // (none when it hit a city).
        Command::RangedAttack { unit, target } => (unit, struck(record, unit), target),
        // A sweep stays at home: the sortie flies to the tile; the interceptor that
        // fought it (if any) is the defender.
        Command::AirSweep { unit, target } => (unit, struck(record, unit), target),
        _ => return None,
    };
    // A melee winner advances into the tile it cleared: the strike
    // starts where it stood before that step.
    let advanced = record.changes.unit_changes.iter().find_map(|change| match *change {
        UnitChange::Moved { unit, from, .. } if unit == attacker => Some(from),
        _ => None,
    });
    let from = match advanced {
        Some(from) => from,
        None => roster.tile(game, attacker)?,
    };
    let attacker_figure = roster.figure(game, attacker)?;
    let defender_figure = match defender {
        Some(unit) => Some(roster.figure(game, unit)?),
        None => None,
    };
    let attacker_seen = attacker_figure.owner == observer.player.0 || observer.sees(from);
    let target_seen = observer.sees(target) || defender_figure.is_some_and(|d| d.owner == observer.player.0);
    if !attacker_seen && !target_seen {
        return None;
    }
    let attack = Attack {
        attacker: attacker_figure,
        from,
        target,
        defender: defender_figure.filter(|_| target_seen),
        style: attack_style(observer.rules, attacker_figure.kind),
        reach: reach(observer, from, target),
        weapon: attack_weapon(observer.rules, attacker_figure.kind),
        figures: formation_size(observer.rules, attacker_figure.kind),
        defender_hp: match defender {
            Some(unit) if !destroyed(record, unit) => roster.health_change(game, unit, damage(record, unit)),
            _ => None,
        },
        attacker_hp: if destroyed(record, attacker) { None } else { roster.health_change(game, attacker, damage(record, attacker)) },
        dealt: match defender {
            Some(unit) => damage(record, unit),
            None => city_damage(record),
        },
        taken: damage(record, attacker),
        kill: defender.is_some_and(|unit| destroyed(record, unit)),
        attacker_seen,
        target_owner: match defender_figure {
            Some(figure) => figure.owner,
            None => struck_city_owner(game, record, target).map_or(attacker_figure.owner, |p| p.0),
        },
    };
    let concerns = [Some(attacker_figure), defender_figure].into_iter().flatten().any(|f| f.owner == observer.player.0)
        || city_owner(game, target) == Some(observer.player);
    Some((Cue::Attack(attack), concerns))
}

/// Hit points the cities of a record lost (`ClassicChange::CityDamaged`).
fn city_damage(record: &AppliedCommand) -> Option<u16> {
    let mut total = None;
    for change in &record.changes.module_changes {
        if let ClassicChange::CityDamaged { damage, .. } = *change {
            total = Some(total.unwrap_or(0) + damage);
        }
    }
    total
}

/// The owner of the city on `tile` when it was struck (its former owner
/// when the strike took it).
fn struck_city_owner(game: GameView<'_>, record: &AppliedCommand, tile: TileId) -> Option<PlayerId> {
    let city = game.auth.indexes().city_on_tile(tile)?;
    let taken = record.changes.city_changes.iter().find_map(|change| match *change {
        CityChange::OwnerChanged { city: c, from, .. } if c == city => Some(from),
        _ => None,
    });
    taken.or_else(|| city_owner(game, tile))
}

/// A city's ranged strike as the observer sees it: the target's tile must
/// be in sight, or the city or the target the observer's.
fn city_strike_cue(observer: &Observer<'_>, roster: &Roster, record: &AppliedCommand) -> Option<(Cue, bool)> {
    let Command::CityStrike { city, target } = record.command else {
        return None;
    };
    let game = observer.game;
    let core = game.auth.cities().get(city)?;
    // The unit the strike damaged or killed (a city takes no answer).
    let defender = record.changes.unit_changes.iter().find_map(|change| match *change {
        UnitChange::Damaged { unit, .. } | UnitChange::Destroyed { unit, .. } => Some(unit),
        _ => None,
    });
    let defender_figure = defender.and_then(|unit| roster.figure(game, unit));
    let own = core.owner == observer.player || defender_figure.is_some_and(|d| d.owner == observer.player.0);
    if !own && !observer.sees(target) && !observer.sees(core.tile) {
        return None;
    }
    let strike = CityStrike {
        city: core.tile,
        owner: core.owner.0,
        target,
        defender: defender_figure,
        dealt: defender.and_then(|unit| damage(record, unit)),
        defender_hp: match defender {
            Some(unit) if !destroyed(record, unit) => roster.health_change(game, unit, damage(record, unit)),
            _ => None,
        },
        kill: defender.is_some_and(|unit| destroyed(record, unit)),
    };
    Some((Cue::CityStrike(strike), own))
}

fn city_tile(game: GameView<'_>, city: CityHandle) -> Option<TileId> {
    game.auth.cities().get(city).map(|c| c.tile)
}

fn city_owner(game: GameView<'_>, tile: TileId) -> Option<PlayerId> {
    let city = game.auth.indexes().city_on_tile(tile)?;
    game.auth.cities().get(city).map(|c| c.owner)
}

/// The unit a ranged attack of `attacker` damaged or killed.
fn struck(record: &AppliedCommand, attacker: UnitHandle) -> Option<UnitHandle> {
    record.changes.unit_changes.iter().find_map(|change| match *change {
        UnitChange::Damaged { unit, .. } | UnitChange::Destroyed { unit, .. } if unit != attacker => Some(unit),
        _ => None,
    })
}

fn damage(record: &AppliedCommand, unit: UnitHandle) -> Option<u16> {
    record.changes.unit_changes.iter().find_map(|change| match *change {
        UnitChange::Damaged { unit: hit, damage } if hit == unit => Some(damage),
        _ => None,
    })
}

fn destroyed(record: &AppliedCommand, unit: UnitHandle) -> bool {
    record
        .changes
        .unit_changes
        .iter()
        .any(|change| matches!(change, UnitChange::Destroyed { unit: gone, .. } if *gone == unit))
}

/// The model a unit kind is drawn with (None: the art has no model for it).
fn model_of(rules: &CompiledRuleset, kind: u16) -> Option<&'static render::unit_models::UnitModel> {
    let def = rules.units.get(kind as usize)?;
    render::unit_models::resolve(&def.key).map(|id| id.model())
}

/// What a unit fights with, from its art: the model's weapon decides how the attack looks
/// (an anti-tank gun and a tank fire shells whatever the rules call their range).
pub fn attack_weapon(rules: &CompiledRuleset, kind: u16) -> Weapon {
    match model_of(rules, kind).map(|m| m.weapon) {
        Some(Weapon::None) | None => match attack_style_by_rules(rules, kind) {
            AttackStyle::Archery => Weapon::Bow,
            AttackStyle::Gunpowder => Weapon::Firearm,
            AttackStyle::Artillery => Weapon::Cannon,
            AttackStyle::Naval => Weapon::Broadside,
            AttackStyle::Air => Weapon::Bomb,
            AttackStyle::Melee | AttackStyle::Mounted => Weapon::Blade,
        },
        Some(weapon) => weapon,
    }
}

/// Figures of the unit's formation.
pub fn formation_size(rules: &CompiledRuleset, kind: u16) -> u8 {
    model_of(rules, kind).map_or(1, |m| m.slots.len() as u8)
}

/// Look and sound of a unit's attack (the sound and the coarse class): from its weapon.
pub fn attack_style(rules: &CompiledRuleset, kind: u16) -> AttackStyle {
    match model_of(rules, kind).map(|m| m.weapon) {
        Some(Weapon::Bow | Weapon::Crossbow | Weapon::Thrown) => AttackStyle::Archery,
        Some(Weapon::Firearm | Weapon::Autogun | Weapon::Rocket) => AttackStyle::Gunpowder,
        Some(Weapon::Cannon | Weapon::Catapult | Weapon::Ballista) => AttackStyle::Artillery,
        Some(Weapon::Broadside | Weapon::Torpedo) => AttackStyle::Naval,
        Some(Weapon::Bomb | Weapon::Missile) => AttackStyle::Air,
        Some(Weapon::Blade | Weapon::Club | Weapon::Polearm | Weapon::Ram | Weapon::None) | None => attack_style_by_rules(rules, kind),
    }
}

/// Look and sound of a unit's attack, from its role and domain.
fn attack_style_by_rules(rules: &CompiledRuleset, kind: u16) -> AttackStyle {
    let Some(def) = rules.units.get(kind as usize) else {
        return AttackStyle::Melee;
    };
    match (def.domain, def.role) {
        (Domain::Air, _) => AttackStyle::Air,
        (Domain::Sea, _) => AttackStyle::Naval,
        (_, Some(UnitRole::Siege)) => AttackStyle::Artillery,
        (_, Some(UnitRole::Archer)) => AttackStyle::Archery,
        (_, Some(UnitRole::Mounted | UnitRole::Armor)) => AttackStyle::Mounted,
        (_, Some(UnitRole::Gun)) => AttackStyle::Gunpowder,
        _ if def.ranged_strength > 0 && def.range > 0 => AttackStyle::Archery,
        _ => AttackStyle::Melee,
    }
}

/// Deaths and departures, and fortification.
fn unit_cues(observer: &Observer<'_>, roster: &Roster, record: &AppliedCommand) -> Vec<(Cue, bool)> {
    let fight = matches!(
        record.command,
        Command::AttackUnit { .. }
            | Command::AttackCity { .. }
            | Command::RangedAttack { .. }
            | Command::CityStrike { .. }
            | Command::AirSweep { .. }
    );
    let mut cues = Vec::new();
    for change in &record.changes.unit_changes {
        match *change {
            UnitChange::Destroyed { id, owner, tile, kind, .. } => {
                let own = owner == observer.player;
                if !(own || observer.sees(tile)) {
                    continue;
                }
                let unit = Figure { id, kind: kind.0, owner: owner.0 };
                let cue = match fight {
                    true => Cue::Death { unit, tile },
                    false => Cue::Vanish { unit, tile },
                };
                cues.push((cue, own));
            }
            UnitChange::StanceChanged { unit } => {
                let (Some(figure), Some(tile)) = (roster.figure(observer.game, unit), roster.tile(observer.game, unit)) else {
                    continue;
                };
                let own = figure.owner == observer.player.0;
                if own || observer.sees(tile) {
                    cues.push((Cue::Fortify { unit: figure, tile }, own));
                }
            }
            _ => {}
        }
    }
    cues
}

fn city_cues(observer: &Observer<'_>, record: &AppliedCommand) -> Vec<(Cue, bool)> {
    let founding = matches!(record.command, Command::FoundCity { .. });
    let mut cues = Vec::new();
    for change in &record.changes.city_changes {
        let (city, cue) = match *change {
            CityChange::Founded(city) => (city, CityCue::Founded),
            CityChange::PopulationChanged { city } => (city, CityCue::Grew),
            CityChange::OwnerChanged { city, .. } => (city, CityCue::Captured),
            // The palace of a new capital is part of the founding.
            CityChange::BuildingAdded { city, building } if !founding => {
                let wonder = observer.rules.buildings.get(building.0 as usize).is_some_and(|b| b.wonder);
                (city, CityCue::Built { wonder })
            }
            _ => continue,
        };
        let Some(core) = observer.game.auth.cities().get(city) else {
            continue;
        };
        let own = core.owner == observer.player;
        if !(own || observer.sees(core.tile)) {
            continue;
        }
        cues.push((cue.cue(core.tile, core.owner.0), own));
    }
    cues
}

#[derive(Clone, Copy)]
enum CityCue {
    Founded,
    Grew,
    Captured,
    Built { wonder: bool },
}

impl CityCue {
    fn cue(self, tile: TileId, owner: u8) -> Cue {
        match self {
            CityCue::Founded => Cue::CityFounded { tile, owner },
            CityCue::Grew => Cue::CityGrew { tile },
            CityCue::Captured => Cue::CityCaptured { tile, owner },
            CityCue::Built { wonder } => Cue::BuildingCompleted { tile, wonder },
        }
    }
}

/// The observer's research, policies and faith.
fn announcements(observer: &Observer<'_>, record: &AppliedCommand, origin: Origin) -> Vec<Cue> {
    let mut cues = Vec::new();
    let mine = |player: PlayerId| player == observer.player;
    for change in &record.changes.player_changes {
        match *change {
            // Choosing a technology journals the same entry; only learning is news.
            PlayerChange::TechnologyChosen { player } if mine(player) => {
                if !matches!(record.command, Command::ChooseTechnology { .. }) {
                    cues.push(Cue::Announcement(Sound::Tech));
                }
            }
            _ => {}
        }
    }
    for change in &record.changes.module_changes {
        if let ClassicChange::PolicyAdopted { player } = *change {
            if mine(player) {
                cues.push(Cue::Announcement(Sound::Decree));
            }
        }
    }
    match record.command {
        Command::FoundPantheon { player, .. } if mine(player) => {
            cues.push(Cue::Announcement(Sound::Hymn));
        }
        Command::FoundReligion { .. } if origin == Origin::Own => {
            cues.push(Cue::Announcement(Sound::Hymn));
        }
        _ => {}
    }
    cues
}
