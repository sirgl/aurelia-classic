//! Save of the diplomatic relations (`world::relations`): pair matrices
//! as they are, trades and offers as plain tuples. Cities named by offers
//! are positions in the saved city list.

use crate::ids::{CityHandle, PlayerId, ResourceId};
use crate::persistence::decode::DecodeError;
use crate::persistence::schema::SavedRelations;
use crate::rules::CompiledRuleset;
use crate::world::relations::{DealItem, DealOffer, OngoingTrade, RelationsState, TradeFlow};

/// (tag, value, extra) of a deal item.
fn encode_item(item: DealItem, city_ordinals: &[u32]) -> (u8, u32, u16) {
    match item {
        DealItem::Gold(g) => (0, g, 0),
        DealItem::GoldPerTurn(g) => (1, g, 0),
        DealItem::Resource { resource, amount } => (2, u32::from(resource.0), amount),
        DealItem::City(c) => (
            3,
            city_ordinals
                .get(c.index as usize)
                .copied()
                .unwrap_or(u32::MAX),
            0,
        ),
        DealItem::Embassy => (4, 0, 0),
        DealItem::OpenBorders => (5, 0, 0),
        DealItem::DefensivePact => (6, 0, 0),
        DealItem::ResearchAgreement => (7, 0, 0),
        DealItem::Friendship => (8, 0, 0),
        DealItem::Peace => (9, 0, 0),
        DealItem::ThirdPartyWar(p) => (10, u32::from(p.0), 0),
        DealItem::ThirdPartyPeace(p) => (11, u32::from(p.0), 0),
    }
}

fn decode_item(
    (tag, value, extra): (u8, u32, u16),
    rules: &CompiledRuleset,
    players: usize,
    cities: &[CityHandle],
) -> Option<DealItem> {
    let player = |v: u32| (v as usize) < players;
    Some(match tag {
        0 => DealItem::Gold(value),
        1 => DealItem::GoldPerTurn(value),
        2 if (value as usize) < rules.resources.len() => DealItem::Resource {
            resource: ResourceId(value as u16),
            amount: extra,
        },
        3 => DealItem::City(*cities.get(value as usize)?),
        4 => DealItem::Embassy,
        5 => DealItem::OpenBorders,
        6 => DealItem::DefensivePact,
        7 => DealItem::ResearchAgreement,
        8 => DealItem::Friendship,
        9 => DealItem::Peace,
        10 if player(value) => DealItem::ThirdPartyWar(PlayerId(value as u8)),
        11 if player(value) => DealItem::ThirdPartyPeace(PlayerId(value as u8)),
        _ => return None,
    })
}

pub(crate) fn encode_relations(r: &RelationsState, city_ordinals: &[u32]) -> SavedRelations {
    SavedRelations {
        peace_until: r.peace_until.clone(),
        war_since: r.war_since.clone(),
        embassy: r.embassy.clone(),
        pact_until: r.pact_until.clone(),
        research_until: r.research_until.clone(),
        research_points: r.research_points.clone(),
        friends_until: r.friends_until.clone(),
        denounced_until: r.denounced_until.clone(),
        open_until: r.open_until.clone(),
        warmonger: r.warmonger.iter().map(|w| *w as u32).collect(),
        trades: r
            .trades
            .iter()
            .map(|t| {
                let (tag, value, extra) = match t.flow {
                    TradeFlow::GoldPerTurn(g) => (0u8, g, 0u16),
                    TradeFlow::Resource { resource, amount } => (1, u32::from(resource.0), amount),
                };
                (t.from.0, t.to.0, (tag, value, extra), t.until)
            })
            .collect(),
        offers: r
            .offers
            .iter()
            .map(|o| {
                let items = o
                    .items
                    .iter()
                    .map(|(giver, item)| (giver.0, encode_item(*item, city_ordinals)))
                    .collect();
                (o.id, o.from.0, o.to.0, o.expires, items)
            })
            .collect(),
        next_offer: r.next_offer,
    }
}

fn corrupt(what: &str) -> DecodeError {
    DecodeError::Corrupt(format!("relations {what}"))
}

pub(crate) fn decode_relations(
    s: &SavedRelations,
    players: usize,
    rules: &CompiledRuleset,
    cities: &[CityHandle],
) -> Result<RelationsState, DecodeError> {
    let n = players * players;
    let sizes = [
        s.peace_until.len(),
        s.war_since.len(),
        s.embassy.len(),
        s.pact_until.len(),
        s.research_until.len(),
        s.research_points.len(),
        s.friends_until.len(),
        s.denounced_until.len(),
        s.open_until.len(),
        s.warmonger.len(),
    ];
    if sizes.iter().any(|len| *len != n) || s.embassy.iter().any(|e| *e > 1) {
        return Err(corrupt("matrices"));
    }
    let player = |p: u8| -> Result<PlayerId, DecodeError> {
        if (p as usize) < players {
            Ok(PlayerId(p))
        } else {
            Err(corrupt("player"))
        }
    };
    let mut trades = Vec::with_capacity(s.trades.len());
    for (from, to, (tag, value, extra), until) in &s.trades {
        let flow = match tag {
            0 => TradeFlow::GoldPerTurn(*value),
            1 if (*value as usize) < rules.resources.len() => TradeFlow::Resource {
                resource: ResourceId(*value as u16),
                amount: *extra,
            },
            _ => return Err(corrupt("trade")),
        };
        trades.push(OngoingTrade {
            from: player(*from)?,
            to: player(*to)?,
            flow,
            until: *until,
        });
    }
    let mut offers = Vec::with_capacity(s.offers.len());
    for (id, from, to, expires, items) in &s.offers {
        if *id >= s.next_offer {
            return Err(corrupt("offer id"));
        }
        let mut decoded = Vec::with_capacity(items.len());
        for (giver, item) in items {
            let item =
                decode_item(*item, rules, players, cities).ok_or_else(|| corrupt("offer item"))?;
            decoded.push((player(*giver)?, item));
        }
        offers.push(DealOffer {
            id: *id,
            from: player(*from)?,
            to: player(*to)?,
            items: decoded,
            expires: *expires,
        });
    }
    Ok(RelationsState {
        players,
        peace_until: s.peace_until.clone(),
        war_since: s.war_since.clone(),
        embassy: s.embassy.clone(),
        pact_until: s.pact_until.clone(),
        research_until: s.research_until.clone(),
        research_points: s.research_points.clone(),
        friends_until: s.friends_until.clone(),
        denounced_until: s.denounced_until.clone(),
        open_until: s.open_until.clone(),
        warmonger: s
            .warmonger
            .iter()
            .map(|w| (*w).min(i32::MAX as u32) as i32)
            .collect(),
        trades,
        offers,
        next_offer: s.next_offer,
    })
}
