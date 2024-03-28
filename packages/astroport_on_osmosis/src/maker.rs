use std::ops::RangeInclusive;

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Coin, Decimal, Uint128};

/// Validation limit for max spread. From 0 to 50%.
pub const MAX_ALLOWED_SPREAD: Decimal = Decimal::percent(50);
/// Validations limits for cooldown period. From 30 to 600 seconds.
pub const COOLDOWN_LIMITS: RangeInclusive<u64> = 30..=600;
/// Maximum allowed route hops
pub const MAX_SWAPS_DEPTH: u8 = 5;

/// Default pagination limit
pub const DEFAULT_PAGINATION_LIMIT: u32 = 50;

#[cw_serde]
pub struct InstantiateMsg {
    /// The contract's owner, who can update config
    pub owner: String,
    /// ASTRO denom
    pub astro_denom: String,
    /// Address which receives all swapped Astro. On Osmosis this is the address of the satellite contract
    pub satellite: String,
    /// The maximum spread used when swapping fee tokens to ASTRO
    pub max_spread: Decimal,
    /// If set defines the period when maker collect can be called
    pub collect_cooldown: Option<u64>,
}

#[cw_serde]
#[derive(Eq, Hash)]
pub struct PoolRoute {
    pub denom_in: String,
    pub denom_out: String,
    pub pool_id: u64,
}

#[cw_serde]
pub struct CoinWithLimit {
    pub denom: String,
    pub amount: Option<Uint128>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Collects and swaps fee tokens to ASTRO
    Collect {
        /// Coins to swap to ASTRO. coin.amount is the amount to swap. If amount is omitted then whole balance will be used.
        /// If amount is more than the balance, it will swap the whole balance.
        assets: Vec<CoinWithLimit>,
    },
    /// Updates general settings. Only the owner can execute this.
    UpdateConfig {
        /// ASTRO denom
        astro_denom: Option<String>,
        /// Fee receiver address.
        fee_receiver: Option<String>,
        /// The maximum spread used when swapping fee tokens to ASTRO
        max_spread: Option<Decimal>,
        /// Defines the period when maker collect can be called
        collect_cooldown: Option<u64>,
    },
    /// Configure specific pool ids for swapping asset_in to asset_out.
    /// If route already exists, it will be overwritten.
    SetPoolRoutes(Vec<PoolRoute>),
    /// Creates a request to change the contract's ownership
    ProposeNewOwner {
        /// The newly proposed owner
        owner: String,
        /// The validity period of the proposal to change the owner
        expires_in: u64,
    },
    /// Removes a request to change contract ownership
    DropOwnershipProposal {},
    /// Claims contract ownership
    ClaimOwnership {},
}

#[cw_serde]
pub struct Config {
    /// The contract's owner, who can update config
    pub owner: Addr,
    /// ASTRO denom
    pub astro_denom: String,
    /// Address which receives all swapped Astro. On Osmosis this is the address of the satellite contract
    pub satellite: Addr,
    /// The maximum spread used when swapping fee tokens to ASTRO
    pub max_spread: Decimal,
    /// If set defines the period when maker collect can be called
    pub collect_cooldown: Option<u64>,
}

#[cw_serde]
pub struct SwapRouteResponse {
    pub pool_id: u64,
    pub token_out_denom: String,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Query contract owner config
    #[returns(Config)]
    Config {},
    /// Get route for swapping an input denom into an output denom
    #[returns(Vec<SwapRouteResponse>)]
    Route { denom_in: String, denom_out: String },
    /// List all maker routes
    #[returns(Vec<PoolRoute>)]
    Routes {
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// Return current spot price swapping In for Out
    #[returns(Uint128)]
    EstimateExactInSwap { coin_in: Coin },
}
