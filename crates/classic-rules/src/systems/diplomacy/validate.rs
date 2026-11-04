//! Validation of the diplomacy commands (pure).

use crate::command::{CommandError, ResolvedCommand};
use crate::ids::PlayerId;
use crate::rules::CompiledRuleset;
use crate::systems::diplomacy::deals::offer_problem;
use crate::systems::diplomacy::has_met;
use crate::world::relations::DealItem;
use crate::world::state::GameView;
use crate::world::player::ClassicPlayers;

fn violation(reason: &str) -> CommandError {
    CommandError::RuleViolation(reason.into())
}

fn alive_pair(view: GameView<'_>, a: PlayerId, b: PlayerId) -> Result<(), CommandError> {
    let players = view.auth.players();
    if !players.is_alive(a) || !players.is_alive(b) {
        return Err(CommandError::UnknownPlayer);
    }
    if a == b {
        return Err(violation("same player"));
    }
    Ok(())
}

/// War (§5.1): a major nation declares war on a player it met, not
/// already at war, not under a peace treaty.
pub(crate) fn validate_declare_war(
    view: GameView<'_>,
    player: PlayerId,
    enemy: PlayerId,
) -> Result<ResolvedCommand, CommandError> {
    alive_pair(view, player, enemy)?;
    let auth = view.auth;
    if !auth.players().is_major(player) {
        return Err(violation("only nations declare war"));
    }
    if auth.diplomacy().at_war(player, enemy) {
        return Err(violation("already at war"));
    }
    if !has_met(auth, player, enemy) {
        return Err(violation("not met"));
    }
    if auth.turn().round < auth.module().relations().peace_treaty_until(player, enemy) {
        return Err(violation("peace treaty"));
    }
    Ok(ResolvedCommand::DeclareWar { player, enemy })
}

pub(crate) fn validate_make_peace(
    view: GameView<'_>,
    player: PlayerId,
    enemy: PlayerId,
) -> Result<ResolvedCommand, CommandError> {
    alive_pair(view, player, enemy)?;
    let auth = view.auth;
    if !auth.diplomacy().at_war(player, enemy) {
        return Err(violation("not at war"));
    }
    if auth.players().is_barbarian(player) || auth.players().is_barbarian(enemy) {
        return Err(violation("no peace with barbarians"));
    }
    if crate::systems::diplomacy::deals::peace_too_early(auth, player, enemy) {
        return Err(violation("peace needs 10 rounds of war"));
    }
    if crate::systems::city_states::peace_blocked(auth, player, enemy) {
        return Err(violation("the city-state's ally is at war with you"));
    }
    Ok(ResolvedCommand::MakePeace { player, enemy })
}

/// Open borders as a gift of the host (one side of a deal).
pub(crate) fn validate_open_borders(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    host: PlayerId,
    guest: PlayerId,
) -> Result<ResolvedCommand, CommandError> {
    alive_pair(view, host, guest)?;
    if view.auth.diplomacy().grants_entry(host, guest) {
        return Err(violation("already open"));
    }
    if !view.auth.players().is_major(host) {
        return Err(violation("only nations open borders"));
    }
    let problem = crate::systems::diplomacy::deals::item_problem(
        view.auth,
        rules,
        host,
        guest,
        DealItem::OpenBorders,
        false,
    );
    if let Some(block) = problem {
        return Err(violation(block.reason()));
    }
    Ok(ResolvedCommand::OpenBorders { host, guest })
}

pub(crate) fn validate_denounce(
    view: GameView<'_>,
    player: PlayerId,
    target: PlayerId,
) -> Result<ResolvedCommand, CommandError> {
    alive_pair(view, player, target)?;
    let auth = view.auth;
    if !auth.players().is_major(player) || !auth.players().is_major(target) {
        return Err(violation("only nations denounce nations"));
    }
    if !has_met(auth, player, target) {
        return Err(violation("not met"));
    }
    if auth
        .module()
        .relations()
        .denounced(player, target, auth.turn().round)
    {
        return Err(violation("already denounced"));
    }
    Ok(ResolvedCommand::Denounce { player, target })
}

pub(crate) fn validate_propose_deal(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    from: PlayerId,
    to: PlayerId,
    items: &[(PlayerId, DealItem)],
) -> Result<ResolvedCommand, CommandError> {
    alive_pair(view, from, to)?;
    let auth = view.auth;
    if !auth.players().is_major(from) || !auth.players().is_major(to) {
        return Err(violation("deals are between nations"));
    }
    if !has_met(auth, from, to) {
        return Err(violation("not met"));
    }
    if let Some(reason) = offer_problem(auth, rules, from, to, items) {
        return Err(violation(reason));
    }
    let pending = auth
        .module()
        .relations()
        .offers()
        .iter()
        .filter(|o| o.from == from && o.to == to)
        .count();
    if pending >= 3 {
        return Err(violation("too many pending offers"));
    }
    Ok(ResolvedCommand::ProposeDeal {
        from,
        to,
        items: items.to_vec(),
    })
}

/// The receiver answers (on any turn, not only during the proposer's).
pub(crate) fn validate_respond_deal(
    view: GameView<'_>,
    rules: &CompiledRuleset,
    player: PlayerId,
    offer: u32,
    accept: bool,
) -> Result<ResolvedCommand, CommandError> {
    let found = view
        .auth
        .module()
        .relations()
        .offer(offer)
        .ok_or(violation("no such offer"))?;
    if found.to != player {
        return Err(violation("not your offer"));
    }
    // The deal is checked again when it runs; asking now lets a screen and
    // an AI see that accepting is refused (the offer stays until it expires).
    if accept && super::deals::offer_problem(view.auth, rules, found.from, found.to, &found.items).is_some() {
        return Err(violation("the deal is no longer possible"));
    }
    Ok(ResolvedCommand::RespondDeal {
        player,
        offer,
        accept,
    })
}
