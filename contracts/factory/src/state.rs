use astroport::asset::AssetInfo;
use astroport::common::OwnershipProposal;
use astroport::factory::{Config, PairConfig};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Deps, Order, QuerierWrapper, StdResult};
use cw_storage_plus::{Bound, Item, Map};
use itertools::Itertools;

use crate::error::ContractError;

/// This is an intermediate structure for storing a pair's key. It is used in a submessage response.
#[cw_serde]
pub struct TmpPairInfo {
    pub pair_key: Vec<u8>,
}

/// Saves a pair's key
pub const TMP_PAIR_INFO: Item<TmpPairInfo> = Item::new("tmp_pair_info");

/// Saves factory settings
pub const CONFIG: Item<Config> = Item::new("config");

/// Saves created pairs (from olders to latest)
pub const PAIRS: Map<&[u8], Addr> = Map::new("pair_info");

/// Calculates a pair key from the specified parameters in the `asset_infos` variable.
///
/// `asset_infos` is an array with multiple items of type [`AssetInfo`].
pub fn pair_key(asset_infos: &[AssetInfo]) -> Vec<u8> {
    asset_infos
        .iter()
        .map(AssetInfo::as_bytes)
        .sorted()
        .flatten()
        .copied()
        .collect()
}

/// Saves pair type configurations
pub const PAIR_CONFIGS: Map<String, PairConfig> = Map::new("pair_configs");

/// ## Pagination settings
/// The maximum limit for reading pairs from [`PAIRS`]
const MAX_LIMIT: u32 = 30;
/// The default limit for reading pairs from [`PAIRS`]
const DEFAULT_LIMIT: u32 = 10;

/// Reads pairs from the [`PAIRS`] vector according to the `start_after` and `limit` variables.
/// Otherwise, it returns the default number of pairs, starting from the oldest one.
///
/// `start_after` is the pair from which the function starts to fetch results.
///
/// `limit` is the number of items to retrieve.
pub fn read_pairs(
    deps: Deps,
    start_after: Option<Vec<AssetInfo>>,
    limit: Option<u32>,
) -> StdResult<Vec<Addr>> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;

    if let Some(start) = calc_range_start(start_after) {
        PAIRS
            .range(
                deps.storage,
                Some(Bound::exclusive(start.as_slice())),
                None,
                Order::Ascending,
            )
            .take(limit)
            .map(|item| {
                let (_, pair_addr) = item?;
                Ok(pair_addr)
            })
            .collect()
    } else {
        PAIRS
            .range(deps.storage, None, None, Order::Ascending)
            .take(limit)
            .map(|item| {
                let (_, pair_addr) = item?;
                Ok(pair_addr)
            })
            .collect()
    }
}

/// Calculates the key of a pair from which to start reading data.
///
/// `start_after` is an [`Option`] type that accepts [`AssetInfo`] elements.
/// It is the token pair which we use to determine the start index for a range when returning data for multiple pairs
fn calc_range_start(start_after: Option<Vec<AssetInfo>>) -> Option<Vec<u8>> {
    start_after.map(|ref asset| {
        let mut key = pair_key(asset);
        key.push(1);
        key
    })
}

/// CW20 tokens are prohibited on Osmosis. This function checks that all asset infos are native tokens.
/// It also sends stargate query to ensure denom exists.
pub(crate) fn check_asset_infos(
    querier: QuerierWrapper,
    asset_infos: &[AssetInfo],
) -> Result<(), ContractError> {
    if !asset_infos.iter().all_unique() {
        return Err(ContractError::DoublingAssets {});
    }

    asset_infos
        .iter()
        .try_for_each(|asset_info| match asset_info {
            AssetInfo::NativeToken { denom } => Ok(querier.query_supply(denom).map(|_| ())?),
            AssetInfo::Token { .. } => Err(ContractError::NonNativeToken {}),
        })
}

/// Stores the latest contract ownership transfer proposal
pub const OWNERSHIP_PROPOSAL: Item<OwnershipProposal> = Item::new("ownership_proposal");

/// This state key isn't used anymore but left for backward compatability with old pairs
pub const PAIRS_TO_MIGRATE: Item<Vec<Addr>> = Item::new("pairs_to_migrate");
