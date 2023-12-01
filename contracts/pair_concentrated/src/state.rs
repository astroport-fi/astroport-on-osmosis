use astroport::asset::AssetInfo;
use astroport::common::OwnershipProposal;
use astroport::observation::Observation;
use astroport_circular_buffer::CircularBuffer;
use astroport_pcl_common::state::Config;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Uint128};
use cw_storage_plus::{Item, SnapshotMap};

/// Astroport swap parameters
#[cw_serde]
pub struct SwapParams {
    pub belief_price: Option<Decimal>,
    pub max_spread: Option<Decimal>,
    pub sender: Addr,
    pub to: Option<Addr>,
}

/// Structure stores Astroport swap parameters in the contract state to pass these params to the
/// sudo call where real swap happens.
pub const SWAP_PARAMS: Item<SwapParams> = Item::new("swap_params");

/// Stores pool id which the pair contract belongs to.
pub const POOL_ID: Item<u64> = Item::new("pool_id");

/// Stores pool parameters and state.
pub const CONFIG: Item<Config> = Item::new("config");

/// Stores the latest contract ownership transfer proposal
pub const OWNERSHIP_PROPOSAL: Item<OwnershipProposal> = Item::new("ownership_proposal");

/// Circular buffer to store trade size observations
pub const OBSERVATIONS: CircularBuffer<Observation> =
    CircularBuffer::new("observations_state", "observations_buffer");

/// Stores asset balances to query them later at any block height
pub const BALANCES: SnapshotMap<&AssetInfo, Uint128> = SnapshotMap::new(
    "balances",
    "balances_check",
    "balances_change",
    cw_storage_plus::Strategy::EveryBlock,
);
