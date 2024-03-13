use astroport::asset::MINIMUM_LIQUIDITY_AMOUNT;
use astroport_circular_buffer::error::BufferError;
use astroport_pcl_common::error::PclError;
use cosmwasm_std::{ConversionOverflowError, OverflowError, StdError};
use cw_utils::PaymentError;
use thiserror::Error;

/// This enum describes pair contract errors
#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    ConversionOverflowError(#[from] ConversionOverflowError),

    #[error("{0}")]
    OverflowError(#[from] OverflowError),

    #[error("{0}")]
    PaymentError(#[from] PaymentError),

    #[error("{0}")]
    CircularBuffer(#[from] BufferError),

    #[error("{0}")]
    PclError(#[from] PclError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("You need to provide init params")]
    InitParamsNotFound {},

    #[error("Initial provide can not be one-sided")]
    InvalidZeroAmount {},

    #[error("Initial liquidity must be more than {}", MINIMUM_LIQUIDITY_AMOUNT)]
    MinimumLiquidityAmountError {},

    #[error("Pair is not registered in the factory. Only swap and withdraw are allowed")]
    PairIsNotRegistered {},

    #[error("Invalid number of assets. This pair supports only {0} assets")]
    InvalidNumberOfAssets(usize),

    #[error("The asset {0} does not belong to the pair")]
    InvalidAsset(String),

    #[error("Asset balances tracking is already enabled")]
    AssetBalancesTrackingIsAlreadyEnabled {},

    #[error("Pool id is already set")]
    PoolIdAlreadySet {},

    #[error("Failed to migrate contract")]
    MigrationError {},
}
