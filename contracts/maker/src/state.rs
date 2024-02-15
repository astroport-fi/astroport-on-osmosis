use astroport::common::OwnershipProposal;
use cosmwasm_schema::cw_serde;
use cw_storage_plus::{Item, Map};

use astroport_on_osmosis::maker::Config;

/// Routes is a map of denom_in and denom_out to pool_id.
/// Key: (denom_in), Value: RouteStep object {denom_out, pool_id}
pub const ROUTES: Map<&str, RouteStep> = Map::new("routes");
/// Config is the general settings of the contract.
pub const CONFIG: Item<Config> = Item::new("config");
/// Stores the latest timestamp when fees were collected
pub const LAST_COLLECT_TS: Item<u64> = Item::new("last_collect_ts");
/// Stores the latest proposal to change contract ownership
pub const OWNERSHIP_PROPOSAL: Item<OwnershipProposal> = Item::new("ownership_proposal");

#[cw_serde]
pub struct RouteStep {
    pub denom_out: String,
    pub pool_id: u64,
}
