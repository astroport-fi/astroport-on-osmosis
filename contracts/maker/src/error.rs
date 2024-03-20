use cosmwasm_std::{CheckedMultiplyRatioError, OverflowError, StdError};
use thiserror::Error;

use astroport_on_osmosis::maker::{PoolRoute, MAX_ALLOWED_SPREAD, MAX_SWAPS_DEPTH};

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    OverflowError(#[from] OverflowError),

    #[error("{0}")]
    CheckedMultiplyRatioError(#[from] CheckedMultiplyRatioError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Max spread too high. Max allowed: {MAX_ALLOWED_SPREAD}")]
    MaxSpreadTooHigh {},

    #[error("Incorrect cooldown. Min: {min}, Max: {max}")]
    IncorrectCooldown { min: u64, max: u64 },

    #[error("Empty routes")]
    EmptyRoutes {},

    #[error("Pool {pool_id} doesn't have denom {denom}")]
    InvalidPoolDenom { pool_id: u64, denom: String },

    #[error("Message contains duplicated routes")]
    DuplicatedRoutes {},

    #[error("Route cannot start with ASTRO. Error in route: {route:?}")]
    AstroInRoute { route: PoolRoute },

    #[error("No registered route for {denom}")]
    RouteNotFound { denom: String },

    #[error("Collect cooldown has not elapsed. Next collect is possible at {next_collect_ts}")]
    Cooldown { next_collect_ts: u64 },

    #[error("Failed to build route for {denom}. Max swap depth {MAX_SWAPS_DEPTH}. Check for possible loops. Route taken: {route_taken}")]
    FailedToBuildRoute { denom: String, route_taken: String },

    #[error("Invalid reply id")]
    InvalidReplyId {},

    #[error("Empty collectable assets vector")]
    EmptyAssets {},

    #[error("Nothing to collect")]
    NothingToCollect {},
}
