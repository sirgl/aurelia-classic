//! What a player knows: the kernel's knowledge, fog and view, plus the
//! classic observation the AI reads.

pub use kernel::knowledge::*;

pub mod actions;
pub mod observation;
pub mod public;
pub mod view;

pub use view::PlayerView;

/// The key of `PlayerView::world` of the classic module: only the code below
/// `knowledge` (the sensors: the observation builders and the public
/// queries) can name it, so `ai::*` cannot reach the authoritative world.
///
/// ```compile_fail
/// // The planners cannot even name the key:
/// use classic_rules::knowledge::Sensor;
/// ```
pub(in crate::knowledge) struct Sensor;

impl kernel::knowledge::SensorKey<crate::classic::Classic> for Sensor {}
