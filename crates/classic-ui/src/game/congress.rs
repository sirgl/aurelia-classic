//! The World Congress (later the United Nations) as the player's screen shows it: host,
//! session timer, delegates, proposals with their votes, resolutions in force, what the
//! player may propose, and what a World Leader needs.
use classic_rules::ClassicGame;
use classic_rules::ids::{PlayerId, ResourceId, TileId};
use classic_rules::knowledge::observation::CongressInfo;
use classic_rules::rules::content::ResourceClass;
use classic_rules::world::congress::{ResolutionKind, NO, YES};
use classic_rules::Command;

use i18n::{t, tf};

use super::names;
use super::session::Session;
use crate::ui::model::{
    ChoiceButton, CongressView, GameRequest, NoticeKind, ProposalCard, ProposeRow,
};

impl Session {
    pub fn congress_view(&mut self) -> CongressView {
        let observation = self.game().ai_observation(self.rules(), self.me());
        let Some(info) = observation.congress.clone() else {
            return self.unfounded_congress();
        };
        let title = if info.united_nations {
            t!("classic.game.congress.title-un")
        } else {
            t!("classic.game.congress.title-congress")
        };
        let session = if info.in_session {
            t!("classic.game.congress.in-session").to_string()
        } else {
            tf!("classic.game.congress.next-session", n = info.turns_until_session)
        };
        CongressView {
            title: Some(title),
            host: self.host_name(&info),
            session,
            in_session: info.in_session,
            delegates: self.delegate_counts(),
            delegates_left: info.delegates_left,
            needed: info.needed,
            world_leader: self.world_leader_line(&info),
            proposals: self.proposal_cards(&info),
            active: info
                .active
                .iter()
                .map(|a| self.resolution_title(a.kind, Some(a.choice), false))
                .collect(),
            can_propose: info.can_propose,
            propose: self.propose_rows(&info),
        }
    }

    fn unfounded_congress(&self) -> CongressView {
        CongressView {
            title: None,
            host: String::new(),
            session: tf!("classic.game.congress.unfounded", tech = self.founding_technology()),
            in_session: false,
            delegates: Vec::new(),
            delegates_left: 0,
            needed: classic_rules::world::congress::votes_needed(self.game().view().auth),
            world_leader: String::new(),
            proposals: Vec::new(),
            active: Vec::new(),
            can_propose: false,
            propose: Vec::new(),
        }
    }

    /// The technology that lets a nation found the World Congress, by name.
    fn founding_technology(&self) -> String {
        let found = self.rules().technologies.iter().find(|d| d.module.allows_world_congress);
        found.map(|d| names::named("tech", &d.name, &d.key)).unwrap_or_default()
    }

    fn host_name(&self, info: &CongressInfo) -> String {
        match info.host {
            Some(host) => self.player_name(host),
            None => t!("classic.game.congress.no-host").into(),
        }
    }

    /// The player first, then every nation it met.
    fn congress_members(&self) -> Vec<PlayerId> {
        let mut out = vec![self.me()];
        out.extend(self.met_nations().iter().map(|r| r.other));
        out
    }

    fn delegate_counts(&self) -> Vec<(String, u16)> {
        let auth = self.game().view().auth;
        let rules = self.rules();
        self.congress_members()
            .into_iter()
            .map(|p| {
                let count = classic_rules::world::congress::delegates(auth, rules, p);
                (self.player_name(p), count)
            })
            .collect()
    }

    fn world_leader_line(&self, info: &CongressInfo) -> String {
        let leader = self.game().view().auth.module().victory().world_leader();
        if let Some(leader) = leader {
            return tf!("classic.game.congress.world-leader", name = self.player_name(leader));
        }
        if info.united_nations {
            return tf!("classic.game.congress.leader-needs", n = info.needed);
        }
        tf!("classic.game.congress.victory-needs", n = info.needed)
    }

    fn proposal_cards(&mut self, info: &CongressInfo) -> Vec<ProposalCard> {
        let mut out = Vec::new();
        for (index, proposal) in info.proposals.iter().enumerate() {
            let proposer = match proposal.proposer {
                Some(p) => self.player_name(p),
                None => t!("classic.game.congress.the-league").into(),
            };
            out.push(ProposalCard {
                index: index as u8,
                title: self.resolution_title(proposal.kind, Some(proposal.choice), proposal.repeal),
                proposer,
                choices: self.vote_choices(index as u8, proposal.kind),
            });
        }
        out
    }

    /// Yes and no, or the candidates of an election; one delegate per click.
    fn vote_choices(&mut self, proposal: u8, kind: ResolutionKind) -> Vec<ChoiceButton> {
        let options: Vec<(u32, String)> = if kind.elects() {
            let members = self.congress_members();
            members
                .into_iter()
                .map(|p| (u32::from(p.0), self.player_name(p)))
                .collect()
        } else {
            vec![(YES, t!("classic.game.congress.yes").to_string()), (NO, t!("classic.game.congress.no").to_string())]
        };
        let player = self.me();
        let mut out = Vec::new();
        for (choice, label) in options {
            let command = Command::CastVotes {
                player,
                proposal,
                choice,
                delegates: 1,
            };
            out.push(ChoiceButton {
                choice,
                label,
                availability: self.availability(&command),
            });
        }
        out
    }

    /// Every resolution the player may propose (with its choices), then the repeal of
    /// every resolution in force.
    fn propose_rows(&mut self, info: &CongressInfo) -> Vec<ProposeRow> {
        let mut rows = Vec::new();
        for kind in ResolutionKind::ALL {
            if kind.automatic() {
                continue;
            }
            let choices = self.proposal_choices(kind);
            rows.push(self.propose_row(kind, choices, false));
        }
        for active in &info.active {
            let label = vec![(active.choice, t!("classic.game.congress.repeal").to_string())];
            rows.push(self.propose_row(active.kind, label, true));
        }
        rows
    }

    fn propose_row(
        &mut self,
        kind: ResolutionKind,
        choices: Vec<(u32, String)>,
        repeal: bool,
    ) -> ProposeRow {
        let player = self.me();
        let mut buttons = Vec::new();
        for (choice, label) in choices {
            let command = Command::ProposeResolution {
                player,
                kind,
                choice,
                repeal,
            };
            buttons.push(ChoiceButton {
                choice,
                label,
                availability: self.availability(&command),
            });
        }
        let named = if repeal {
            buttons.first().map(|b| b.choice)
        } else {
            None
        };
        ProposeRow {
            kind,
            title: self.resolution_title(kind, named, repeal),
            text: resolution_text(kind),
            repeal,
            choices: buttons,
        }
    }

    /// What a new proposal of this kind may name: a nation, a luxury, or nothing.
    fn proposal_choices(&self, kind: ResolutionKind) -> Vec<(u32, String)> {
        match kind {
            ResolutionKind::PlayerEmbargo => self
                .met_nations()
                .iter()
                .map(|r| (u32::from(r.other.0), self.player_name(r.other)))
                .collect(),
            ResolutionKind::BanLuxury => self
                .known_luxuries()
                .into_iter()
                .map(|r| (u32::from(r.0), self.resource_name(r)))
                .collect(),
            ResolutionKind::InternationalProject => self
                .rules()
                .module.projects
                .iter()
                .enumerate()
                .filter(|(_, p)| p.league.is_some())
                .map(|(i, p)| (i as u32, names::named("project", &p.name, &p.key)))
                .collect(),
            _ => vec![(0, t!("classic.game.congress.propose").to_string())],
        }
    }

    /// Luxuries on the tiles the player knows, in id order.
    fn known_luxuries(&self) -> Vec<ResourceId> {
        let view = self.game().player_view(self.rules(), self.me());
        let mut out: Vec<ResourceId> = Vec::new();
        for tile in 0..view.tile_count() as u32 {
            let known = view.tile(TileId(tile));
            let Some(resource) = known.snapshot().and_then(|s| s.resource) else {
                continue;
            };
            let luxury = self
                .rules()
                .resources
                .get(resource.0 as usize)
                .is_some_and(|d| d.class == Some(ResourceClass::Luxury));
            if luxury && !out.contains(&resource) {
                out.push(resource);
            }
        }
        out.sort_by_key(|r| r.0);
        out
    }

    /// The resolution's name with its choice (a proposal not made yet has none).
    fn resolution_title(&self, kind: ResolutionKind, choice: Option<u32>, repeal: bool) -> String {
        let name = match (kind, choice) {
            (ResolutionKind::PlayerEmbargo, Some(player)) => {
                tf!("classic.game.congress.title-embargo", name = self.player_name(PlayerId(player as u8)))
            }
            (ResolutionKind::BanLuxury, Some(resource)) => {
                tf!("classic.game.congress.title-ban", name = self.resource_name(ResourceId(resource as u16)))
            }
            (ResolutionKind::InternationalProject, Some(project)) => {
                match self.rules().module.projects.get(project as usize) {
                    Some(p) => tf!("classic.game.congress.title-project", name = names::named("project", &p.name, &p.key)),
                    None => t!("classic.game.congress.title-project-unknown").to_string(),
                }
            }
            _ => resolution_name(kind).to_string(),
        };
        if repeal {
            return tf!("classic.game.congress.title-repeal", name = name);
        }
        name
    }

    pub(super) fn congress_request(&mut self, request: GameRequest) {
        let player = self.me();
        let (command, text) = match request {
            GameRequest::ProposeResolution {
                kind,
                choice,
                repeal,
            } => (
                Command::ProposeResolution {
                    player,
                    kind,
                    choice,
                    repeal,
                },
                tf!("classic.game.congress.proposed",
                    title = self.resolution_title(kind, Some(choice), repeal)
                ),
            ),
            GameRequest::CastVotes {
                proposal,
                choice,
                delegates,
            } => (
                Command::CastVotes {
                    player,
                    proposal,
                    choice,
                    delegates,
                },
                tf!("classic.game.congress.votes-cast", n = delegates),
            ),
            _ => return,
        };
        if self.submit(command) {
            self.notify(NoticeKind::Diplomacy, text);
        }
    }
}

fn resolution_name(kind: ResolutionKind) -> &'static str {
    match kind {
        ResolutionKind::DiplomaticVictory => t!("classic.game.congress.name-world-leader"),
        ResolutionKind::ChangeHost => t!("classic.game.congress.name-change-host"),
        ResolutionKind::StandingArmyTax => t!("classic.game.congress.name-standing-army-tax"),
        ResolutionKind::ScholarsInResidence => t!("classic.game.congress.name-scholars-in-residence"),
        ResolutionKind::NuclearNonProliferation => t!("classic.game.congress.name-nuclear-non-proliferation"),
        ResolutionKind::CulturalHeritageSites => t!("classic.game.congress.name-cultural-heritage-sites"),
        ResolutionKind::CityStateEmbargo => t!("classic.game.congress.name-city-state-embargo"),
        ResolutionKind::PlayerEmbargo => t!("classic.game.congress.name-player-embargo"),
        ResolutionKind::BanLuxury => t!("classic.game.congress.name-ban-luxury"),
        ResolutionKind::ArtsFunding => t!("classic.game.congress.name-arts-funding"),
        ResolutionKind::SciencesFunding => t!("classic.game.congress.name-sciences-funding"),
        ResolutionKind::InternationalProject => t!("classic.game.congress.name-international-project"),
    }
}

fn resolution_text(kind: ResolutionKind) -> &'static str {
    match kind {
        ResolutionKind::DiplomaticVictory => t!("classic.game.congress.text-world-leader"),
        ResolutionKind::ChangeHost => t!("classic.game.congress.text-change-host"),
        ResolutionKind::StandingArmyTax => t!("classic.game.congress.text-standing-army-tax"),
        ResolutionKind::ScholarsInResidence => t!("classic.game.congress.text-scholars-in-residence"),
        ResolutionKind::NuclearNonProliferation => t!("classic.game.congress.text-nuclear-non-proliferation"),
        ResolutionKind::CulturalHeritageSites => t!("classic.game.congress.text-cultural-heritage-sites"),
        ResolutionKind::CityStateEmbargo => t!("classic.game.congress.text-city-state-embargo"),
        ResolutionKind::PlayerEmbargo => t!("classic.game.congress.text-player-embargo"),
        ResolutionKind::BanLuxury => t!("classic.game.congress.text-ban-luxury"),
        ResolutionKind::ArtsFunding => t!("classic.game.congress.text-arts-funding"),
        ResolutionKind::SciencesFunding => t!("classic.game.congress.text-sciences-funding"),
        ResolutionKind::InternationalProject => t!("classic.game.congress.text-international-project"),
    }
}
