//! WorldTransaction: the single point of authoritative mutation (§13).
//!
//! This file only dispatches a resolved command to the system that owns the
//! mechanic (`crate::systems::*`) and closes the commit. Every system keeps
//! its own functions and journal entries; there is no callback or event bus
//! between them (§10, §56): a system that causes another one's work calls it
//! explicitly (e.g. production fires OnBuilt through `systems::effects`).

use std::ops::{Deref, DerefMut};

use crate::classic::Classic;
use crate::command::CommandError;
use crate::command::ResolvedCommand;
use crate::derived::DerivedWorld;
use crate::navigation::PathScratch;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::ChangeJournal;
use crate::world::state::AuthWorld;

/// The kernel's transaction instantiated with the classic module.
pub type KernelTransaction<'a> = kernel::transaction::WorldTransaction<'a, Classic>;

/// The classic transaction: the kernel's (its world, journal and systems,
/// reached through `Deref`) with the module's systems as its methods.
/// The only point of mutation: outwards only through GameState::execute.
#[repr(transparent)]
pub struct WorldTransaction<'a>(pub(crate) KernelTransaction<'a>);

impl<'a> WorldTransaction<'a> {
    pub fn new(world: &'a mut AuthWorld) -> Self {
        Self(KernelTransaction::new(world))
    }

    /// The classic view of a kernel transaction: the kernel's systems call
    /// the module's (`classic::systems`) with their own transaction.
    pub(crate) fn of<'t>(tx: &'t mut KernelTransaction<'a>) -> &'t mut Self {
        // SAFETY: `WorldTransaction` is `repr(transparent)` over
        // `KernelTransaction`, so both have one layout; the reference keeps
        // the lifetime and exclusivity of `tx`.
        unsafe { &mut *(tx as *mut KernelTransaction<'a> as *mut Self) }
    }

    /// One logical commit: primary storage + authoritative indexes, atomically.
    /// rules is needed by combat resolution and effects; scratch+derived by the march.
    pub fn apply_resolved(
        &mut self,
        resolved: &ResolvedCommand,
        rules: &CompiledRuleset,
        scratch: &mut PathScratch,
        derived: &DerivedWorld,
    ) -> Result<(), CommandError> {
        match *resolved {
            ResolvedCommand::Kernel(ref resolved) => self.apply_kernel(resolved, rules, scratch, derived),
            ResolvedCommand::Attack(ref plan) => self.attack(plan, rules),
            ResolvedCommand::CityStrike(ref plan) => self.city_strike(plan, rules),
            ResolvedCommand::SetUpRangedAttack { unit } => self.set_up_ranged(unit),
            ResolvedCommand::Promote { unit, promotion } => self.promote(unit, promotion, rules),
            ResolvedCommand::UpgradeUnit { unit, to, price } => {
                self.upgrade_unit(unit, to, price, rules)
            }
            ResolvedCommand::DecideCity { city, decision, .. } => {
                self.decide_city(city, decision, rules)
            }
            ResolvedCommand::Pillage { unit, tile } => self.pillage(unit, tile, rules),
            ResolvedCommand::SellExoticGoods { unit, cargo, gold, experience } => {
                self.sell_exotic_goods(unit, cargo, gold, experience)
            }
            ResolvedCommand::Rebase { unit, to, carrier } => self.rebase(unit, to, carrier, rules),
            ResolvedCommand::AirSweep { unit, target } => self.air_sweep(unit, target, rules),
            ResolvedCommand::Intercept { unit } => self.set_intercept(unit),
            ResolvedCommand::Nuke { unit, target } => self.nuke(unit, target, rules),
            ResolvedCommand::BuildCitadel { unit, tile } => self.build_citadel(unit, tile, rules),
            ResolvedCommand::EstablishTradeRoute { unit, destination, kind } => {
                self.establish_trade_route(unit, destination, kind, rules)
            }
            ResolvedCommand::ChangeTradeHome { unit, city } => self.change_trade_home(unit, city, rules),
            ResolvedCommand::PlunderTradeRoute { unit } => self.plunder_trade_route(unit, rules),
            ResolvedCommand::Airlift { unit, city } => self.airlift(unit, city, rules),
            ResolvedCommand::Paradrop { unit, to } => self.paradrop(unit, to, rules),
            ResolvedCommand::DeclareWar { player, enemy } => self.declare_war(player, enemy, rules),
            ResolvedCommand::MakePeace { player, enemy } => self.make_peace(player, enemy, rules),
            ResolvedCommand::OpenBorders { host, guest } => self.open_borders(host, guest, rules),
            ResolvedCommand::Denounce { player, target } => self.denounce(player, target),
            ResolvedCommand::ProposeDeal { from, to, ref items } => {
                self.propose_deal(from, to, items.clone())
            }
            ResolvedCommand::RespondDeal { offer, accept, .. } => {
                self.respond_deal(offer, accept, rules)
            }
            ResolvedCommand::ProposeResolution {
                player,
                kind,
                choice,
                repeal,
            } => self.propose_resolution(player, kind, choice, repeal),
            ResolvedCommand::CastVotes {
                player,
                proposal,
                choice,
                delegates,
            } => self.cast_votes(player, proposal, choice, delegates),
            ResolvedCommand::MoveSpy {
                player,
                spy,
                city,
                diplomat,
            } => self.move_spy(player, spy as usize, city, diplomat),
            ResolvedCommand::StealTechnology { player, from, tech } => {
                self.steal_technology(player, from, tech, rules)
            }
            ResolvedCommand::StageCoup { player, spy } => self.stage_coup(player, spy as usize, rules),
            ResolvedCommand::InstallSpaceshipPart { unit, project } => {
                self.install_spaceship_part(unit, project, rules)
            }
            ResolvedCommand::GiftGold {
                player,
                city_state,
                gold,
            } => self.gift_gold(player, city_state, gold, rules),
            ResolvedCommand::GiftUnit { unit, city_state } => self.gift_unit(unit, city_state, rules),
            ResolvedCommand::GiftImprovement {
                player,
                tile,
                improvement,
            } => self.gift_improvement(player, tile, improvement),
            ResolvedCommand::BullyGold { player, city_state } => self.bully_gold(player, city_state, rules),
            ResolvedCommand::BullyWorker { player, city_state } => self.bully_worker(player, city_state, rules),
            ResolvedCommand::Pledge {
                player,
                city_state,
                pledge,
            } => self.pledge(player, city_state, pledge, rules),
            ResolvedCommand::BuyoutCityState { player, city_state } => {
                self.buyout_city_state(player, city_state, rules)
            }
            ResolvedCommand::GreatPersonAction { unit, action } => {
                self.great_person_action(unit, action, rules)
            }
            ResolvedCommand::SetProcess { player, city, process } => {
                self.set_process(player, city, process, rules)
            }
            ResolvedCommand::UnlockPolicyBranch { player, branch } => {
                self.unlock_policy_branch(player, branch, rules)
            }
            ResolvedCommand::AdoptPolicy { player, policy } => {
                self.adopt_policy(player, policy, rules)
            }
            ResolvedCommand::ChooseIdeology { player, ideology } => {
                self.choose_ideology(player, ideology, rules)
            }
            ResolvedCommand::FoundPantheon { player, belief } => {
                self.found_pantheon(player, belief, rules)
            }
            ResolvedCommand::FoundReligion {
                unit,
                religion,
                founder,
                follower,
                pantheon,
                bonus,
            } => {
                let choice = crate::derived::religion::FoundingChoice {
                    religion,
                    founder,
                    follower,
                    pantheon,
                    bonus,
                };
                self.found_religion(unit, &choice, rules)
            }
            ResolvedCommand::EnhanceReligion {
                unit,
                follower,
                enhancer,
            } => self.enhance_religion(unit, follower, enhancer, rules),
            ResolvedCommand::AddReformationBelief { player, belief } => {
                self.add_reformation_belief(player, belief, rules)
            }
            ResolvedCommand::SpreadReligion { unit } => self.spread_religion(unit, rules),
            ResolvedCommand::RemoveHeresy { unit } => self.remove_heresy(unit, rules),
            ResolvedCommand::PurchaseWithFaith { player, city, item } => {
                self.purchase_with_faith(player, city, item, rules)
            }
            ResolvedCommand::CreateGreatImprovement { unit } => {
                self.create_great_improvement(unit, rules)
            }
            ResolvedCommand::CreateGreatWork { unit } => self.create_great_work(unit, rules),
            ResolvedCommand::WriteTreatise { unit } => self.write_treatise(unit, rules),
            ResolvedCommand::PerformConcert { unit } => self.perform_concert(unit, rules),
            ResolvedCommand::SwitchIdeology { player } => self.switch_ideology(player, rules),
            ResolvedCommand::Excavate { unit } => self.excavate(unit, rules),
            ResolvedCommand::ChooseArchaeology { player, tile, choice } => {
                self.choose_archaeology(player, tile, choice, rules)
            }
            ResolvedCommand::ChooseFreeTechnology { player, tech } => {
                self.choose_free_technology(player, tech, rules)
            }
            ResolvedCommand::ChooseFreeGreatPerson { player, kind } => {
                self.choose_free_great_person(player, kind, rules)
            }
            ResolvedCommand::ClaimRuins { unit, tile, reward } => self.claim_ruins(unit, tile, reward),
        }
    }

    /// Commit invariants that hold after any mutation (elimination), then
    /// the journal for derived systems.
    pub fn commit(self) -> Result<ChangeJournal, CommandError> {
        Ok(self.0.commit())
    }

    /// Commit of a game command: elimination, and the turn moves on when
    /// the active player was eliminated.
    pub fn commit_command(self, rules: &CompiledRuleset) -> Result<ChangeJournal, CommandError> {
        Ok(self.0.commit_command(rules))
    }
}

impl<'a> Deref for WorldTransaction<'a> {
    type Target = KernelTransaction<'a>;

    fn deref(&self) -> &KernelTransaction<'a> {
        &self.0
    }
}

impl<'a> DerefMut for WorldTransaction<'a> {
    fn deref_mut(&mut self) -> &mut KernelTransaction<'a> {
        &mut self.0
    }
}
