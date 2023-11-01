use astroport::asset::PairInfo;
use astroport::asset::{Asset, AssetInfo};
use astroport::observation::OracleObservation;
use astroport::pair::{
    ConfigResponse, PoolResponse, ReverseSimulationResponse, SimulationResponse,
};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Empty;
use cosmwasm_std::{Binary, Coin, Decimal, Decimal256, Uint128, Uint64};

/// This structure describes the execute messages available in the contract.
#[cw_serde]
pub enum ExecuteMsg {
    /// This point is meant to be executed only once during instantiation context by factory contract.
    SetPoolId { pool_id: u64 },
    /// ProvideLiquidity allows someone to provide liquidity in the pool
    ProvideLiquidity {
        /// The assets available in the pool
        assets: Vec<Asset>,
        /// The slippage tolerance that allows liquidity provision only if the price in the pool doesn't move too much
        slippage_tolerance: Option<Decimal>,
        /// Determines whether the LP tokens minted for the user is auto_staked in the Generator contract
        auto_stake: Option<bool>,
        /// The receiver of LP tokens
        receiver: Option<String>,
    },
    /// Withdraw liquidity from the pool
    WithdrawLiquidity {
        #[serde(default)]
        assets: Vec<Asset>,
    },
    /// Perform a swap in the pool
    Swap {
        offer_asset: Asset,
        ask_asset_info: Option<AssetInfo>,
        belief_price: Option<Decimal>,
        max_spread: Option<Decimal>,
        to: Option<String>,
    },
    /// Update the pair configuration
    UpdateConfig { params: Binary },
    /// ProposeNewOwner creates a proposal to change contract ownership.
    /// The validity period for the proposal is set in the `expires_in` variable.
    ProposeNewOwner {
        /// Newly proposed contract owner
        owner: String,
        /// The date after which this proposal expires
        expires_in: u64,
    },
    /// DropOwnershipProposal removes the existing offer to change contract ownership.
    DropOwnershipProposal {},
    /// Used to claim contract ownership.
    ClaimOwnership {},
}

/// This structure describes all sudo messages needed to integrate with Osmosis
#[cw_serde]
pub enum SudoMessage {
    /// SetActive sets the active status of the pool.
    SetActive { is_active: bool },
    /// SwapExactAmountIn swaps an exact amount of tokens in for as many tokens out as possible.
    /// The amount of tokens out is determined by the current exchange rate and the swap fee.
    /// The user specifies a minimum amount of tokens out, and the transaction will revert if that amount of tokens
    /// is not received.
    SwapExactAmountIn {
        sender: String,
        token_in: Coin,
        token_out_denom: String,
        token_out_min_amount: Uint128,
        swap_fee: Decimal,
    },
    /// SwapExactAmountOut swaps as many tokens in as possible for an exact amount of tokens out.
    /// The amount of tokens in is determined by the current exchange rate and the swap fee.
    /// The user specifies a maximum amount of tokens in, and the transaction will revert if that amount of tokens
    /// is exceeded.
    SwapExactAmountOut {
        sender: String,
        token_in_denom: String,
        token_in_max_amount: Uint128,
        token_out: Coin,
        swap_fee: Decimal,
    },
}

/// This structure describes the query messages available in the contract.
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Returns information about a pair
    #[returns(PairInfo)]
    Pair {},
    /// Returns information about a pool
    #[returns(PoolResponse)]
    Pool {},
    /// Returns contract configuration
    #[returns(ConfigResponse)]
    Config {},
    /// Returns information about the share of the pool in a vector that contains objects of type [`Asset`].
    #[returns(Vec<Asset>)]
    Share { amount: Uint128 },
    /// Returns information about a swap simulation
    #[returns(SimulationResponse)]
    Simulation {
        offer_asset: Asset,
        ask_asset_info: Option<AssetInfo>,
    },
    /// Returns information about a reverse swap simulation
    #[returns(ReverseSimulationResponse)]
    ReverseSimulation {
        offer_asset_info: Option<AssetInfo>,
        ask_asset: Asset,
    },
    /// Returns information about the cumulative prices
    #[returns(Empty)]
    CumulativePrices {},
    /// Returns current D invariant
    #[returns(Decimal256)]
    ComputeD {},
    /// Query LP token virtual price
    #[returns(Decimal256)]
    LpPrice {},
    /// Returns the balance of the specified asset that was in the pool just preceding the moment
    /// of the specified block height creation.
    #[returns(Option<Uint128>)]
    AssetBalanceAt {
        asset_info: AssetInfo,
        block_height: Uint64,
    },
    /// Query price from observations
    #[returns(OracleObservation)]
    Observe { seconds_ago: u64 },

    ///
    /// # QueryMsg enum variants defined specifically for Osmosis
    ///

    /// GetSwapFee returns the pool's swap fee, based on the current state.
    /// Pools may choose to make their swap fees dependent upon state
    /// (prior TWAPs, network downtime, other pool states, etc.)
    /// This is intended to be fee that is collected by liquidity providers.
    /// If the contract provider wants to collect fee for itself, it should implement its own fee collection mechanism.
    #[returns(GetSwapFeeResponse)]
    GetSwapFee {},

    /// Returns whether the pool has swaps enabled at the moment
    #[returns(IsActiveResponse)]
    IsActive {},

    /// GetTotalShares returns the total number of LP shares in the pool

    /// GetTotalPoolLiquidity returns the coins in the pool owned by all LPs
    #[returns(TotalPoolLiquidityResponse)]
    GetTotalPoolLiquidity {},

    /// Returns the spot price of the 'base asset' in terms of the 'quote asset' in the pool,
    /// errors if either baseAssetDenom, or quoteAssetDenom does not exist.
    /// For example, if this was a UniV2 50-50 pool, with 2 ETH, and 8000 UST
    /// pool.SpotPrice(ctx, "eth", "ust") = 4000.00
    #[returns(SpotPriceResponse)]
    SpotPrice {
        quote_asset_denom: String,
        base_asset_denom: String,
    },

    /// CalcOutAmtGivenIn calculates the amount of tokenOut given tokenIn and the pool's current state.
    /// Returns error if the given pool is not a CFMM pool. Returns error on internal calculations.
    #[returns(CalcOutAmtGivenInResponse)]
    CalcOutAmtGivenIn {
        token_in: Coin,
        token_out_denom: String,
        swap_fee: Decimal,
    },

    /// CalcInAmtGivenOut calculates the amount of tokenIn given tokenOut and the pool's current state.
    /// Returns error if the given pool is not a CFMM pool. Returns error on internal calculations.
    #[returns(CalcInAmtGivenOutResponse)]
    CalcInAmtGivenOut {
        token_out: Coin,
        token_in_denom: String,
        swap_fee: Decimal,
    },
}

#[cw_serde]
#[derive(Default)]
pub struct GetSwapFeeResponse {
    pub swap_fee: Decimal,
}

#[cw_serde]
pub struct IsActiveResponse {
    pub is_active: bool,
}

#[cw_serde]
pub struct TotalPoolLiquidityResponse {
    pub total_pool_liquidity: Vec<Coin>,
}

#[cw_serde]
pub struct SpotPriceResponse {
    pub spot_price: Decimal,
}

#[cw_serde]
pub struct CalcOutAmtGivenInResponse {
    pub token_out: Coin,
}

#[cw_serde]
pub struct CalcInAmtGivenOutResponse {
    pub token_in: Coin,
}

#[cw_serde]
pub struct SwapExactAmountInResponseData {
    pub token_out_amount: Uint128,
}

#[cw_serde]
pub struct SwapExactAmountOutResponseData {
    pub token_in_amount: Uint128,
}
