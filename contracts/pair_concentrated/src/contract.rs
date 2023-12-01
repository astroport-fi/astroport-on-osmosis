use std::vec;

use astroport::asset::AssetInfoExt;
use astroport::asset::{
    addr_opt_validate, Asset, AssetInfo, CoinsExt, Decimal256Ext, PairInfo,
    MINIMUM_LIQUIDITY_AMOUNT,
};
use astroport::common::{claim_ownership, drop_ownership_proposal, propose_new_owner};
use astroport::cosmwasm_ext::{AbsDiff, DecimalToInteger, IntegerToDecimal};
use astroport::factory::PairType;
use astroport::observation::{PrecommitObservation, OBSERVATIONS_SIZE};
use astroport::pair::{InstantiateMsg, MIN_TRADE_SIZE};
use astroport::pair_concentrated::{
    ConcentratedPoolParams, ConcentratedPoolUpdateParams, UpdatePoolParams,
};
use astroport::querier::{query_factory_config, query_fee_info};
use astroport_circular_buffer::BufferManager;
use astroport_pcl_common::state::{
    AmpGamma, Config, PoolParams, PoolState, Precisions, PriceState,
};
use astroport_pcl_common::utils::{
    assert_max_spread, assert_slippage_tolerance, before_swap_check, calc_provide_fee,
    check_assets, check_pair_registered, compute_swap, get_share_in_assets,
};
use astroport_pcl_common::{calc_d, get_xcp};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, coin, ensure, from_json, to_json_binary, Addr, Binary, Decimal, Decimal256, DepsMut, Env,
    MessageInfo, Reply, Response, StdError, StdResult, SubMsg, Uint128,
};
use cw2::set_contract_version;
use cw_utils::must_pay;
use itertools::Itertools;
use osmosis_std::types::osmosis::poolmanager::v1beta1::{MsgSwapExactAmountIn, SwapAmountInRoute};
use osmosis_std::types::osmosis::tokenfactory::v1beta1::{
    MsgBurn, MsgCreateDenom, MsgCreateDenomResponse, MsgMint,
};

use astroport_on_osmosis::pair_pcl::{ExecuteMsg, SwapExactAmountInResponseData};

use crate::error::ContractError;
use crate::state::{
    SwapParams, BALANCES, CONFIG, OBSERVATIONS, OWNERSHIP_PROPOSAL, POOL_ID, SWAP_PARAMS,
};
use crate::utils::{accumulate_swap_sizes, query_native_supply, query_pools};

/// Contract name that is used for migration.
const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Hardcoded factory address
const FACTORY_ADDRESS: &str = include_str!("factory_address");
/// Tokenfactory LP token subdenom
const LP_SUBDENOM: &str = "astroport/share";
/// Reply ID for create denom reply
const CREATE_DENOM_REPLY_ID: u64 = 1;
/// An LP token's precision.
pub(crate) const LP_TOKEN_PRECISION: u8 = 6;

/// Creates a new contract with the specified parameters in the [`InstantiateMsg`].
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    mut deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    ensure!(
        msg.asset_infos.len() == 2,
        StdError::generic_err("asset_infos must contain two elements")
    );
    let denoms = msg
        .asset_infos
        .iter()
        .filter_map(|asset_info| match asset_info {
            AssetInfo::Token { .. } => None,
            AssetInfo::NativeToken { denom } => Some(denom),
        })
        .collect_vec();
    ensure!(
        denoms.len() == 2,
        StdError::generic_err("CW20 tokens are not supported")
    );

    // Check that all denoms exist on chain.
    // This query requires a chain to run cosmwasm VM >= 1.1
    for denom in denoms {
        deps.querier
            .query_supply(denom)
            .map_err(|_| StdError::generic_err(format!("Denom {denom} doesn't exist on chain")))?;
    }

    let params: ConcentratedPoolParams = from_json(
        &msg.init_params
            .ok_or(ContractError::InitParamsNotFound {})?,
    )?;
    ensure!(
        !params.price_scale.is_zero(),
        StdError::generic_err("Initial price scale can not be zero")
    );

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // We pin factory address into wasm binary to prevent using Astroport PCL pools on Osmosis without
    // paying fees to Astroport protocol.
    // Users are discouraged from instantiating PCL pools using usual Osmosis tools as such pool won't be included
    // in Astroport routing as well as swap and withdraw endpoints will be broken forever.
    let factory_addr = Addr::unchecked(FACTORY_ADDRESS);
    Precisions::store_precisions(deps.branch(), &msg.asset_infos, &factory_addr)?;

    let mut pool_params = PoolParams::default();
    pool_params.update_params(UpdatePoolParams {
        mid_fee: Some(params.mid_fee),
        out_fee: Some(params.out_fee),
        fee_gamma: Some(params.fee_gamma),
        repeg_profit_threshold: Some(params.repeg_profit_threshold),
        min_price_scale_delta: Some(params.min_price_scale_delta),
        ma_half_time: Some(params.ma_half_time),
    })?;

    let pool_state = PoolState {
        initial: AmpGamma::default(),
        future: AmpGamma::new(params.amp, params.gamma)?,
        future_time: env.block.time.seconds(),
        initial_time: 0,
        price_state: PriceState {
            oracle_price: params.price_scale.into(),
            last_price: params.price_scale.into(),
            price_scale: params.price_scale.into(),
            last_price_update: env.block.time.seconds(),
            xcp_profit: Decimal256::zero(),
            xcp_profit_real: Decimal256::zero(),
        },
    };

    // NOTE: we are keeping Config as general as possible across all PCL implementations.
    // However, liquidity_token on osmosis is not a cw20 contract, but a native token.
    // Addr::unchecked() is a little hack but devs shouldn't consider it as a cw20 contract on Osmosis.
    let config = Config {
        pair_info: PairInfo {
            contract_addr: env.contract.address.clone(),
            liquidity_token: Addr::unchecked(""),
            asset_infos: msg.asset_infos,
            pair_type: PairType::Custom("concentrated".to_string()),
        },
        factory_addr,
        pool_params,
        pool_state,
        owner: None,
        track_asset_balances: params.track_asset_balances.unwrap_or_default(),
        fee_share: None,
    };

    if config.track_asset_balances {
        for asset in &config.pair_info.asset_infos {
            BALANCES.save(deps.storage, asset, &Uint128::zero(), env.block.height)?;
        }
    }

    CONFIG.save(deps.storage, &config)?;

    BufferManager::init(deps.storage, OBSERVATIONS, OBSERVATIONS_SIZE)?;

    // create lp denom
    let msg_create_lp_denom = SubMsg::reply_on_success(
        MsgCreateDenom {
            sender: env.contract.address.to_string(),
            subdenom: LP_SUBDENOM.to_owned(),
        },
        CREATE_DENOM_REPLY_ID,
    );

    Ok(Response::new()
        .add_submessage(msg_create_lp_denom)
        .add_attribute(
            "asset_balances_tracking",
            if config.track_asset_balances {
                "enabled"
            } else {
                "disabled"
            },
        ))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        CREATE_DENOM_REPLY_ID => {
            let MsgCreateDenomResponse { new_token_denom } = msg.result.try_into()?;
            CONFIG.update(deps.storage, |mut config| {
                if config.pair_info.liquidity_token.as_str().is_empty() {
                    config.pair_info.liquidity_token = Addr::unchecked(&new_token_denom);
                    Ok(config)
                } else {
                    Err(StdError::generic_err(
                        "Liquidity token denom is already set",
                    ))
                }
            })?;

            Ok(Response::new().add_attribute("lp_denom", new_token_denom))
        }
        _ => Err(StdError::generic_err(format!("Unknown reply id: {}", msg.id)).into()),
    }
}

/// Exposes all the execute functions available in the contract.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::SetPoolId { pool_id } => set_pool_id(deps, info, pool_id),
        ExecuteMsg::ProvideLiquidity {
            assets,
            slippage_tolerance,
            receiver,
            ..
        } => provide_liquidity(deps, env, info, assets, slippage_tolerance, receiver),
        ExecuteMsg::Swap {
            offer_asset,
            belief_price,
            max_spread,
            to,
            ..
        } => execute_swap(deps, env, info, offer_asset, belief_price, max_spread, to),
        ExecuteMsg::WithdrawLiquidity { assets } => withdraw_liquidity(deps, env, info, assets),
        ExecuteMsg::UpdateConfig { params } => update_config(deps, env, info, params),
        ExecuteMsg::ProposeNewOwner { owner, expires_in } => {
            let config = CONFIG.load(deps.storage)?;
            let factory_config = query_factory_config(&deps.querier, config.factory_addr)?;

            propose_new_owner(
                deps,
                info,
                env,
                owner,
                expires_in,
                config.owner.unwrap_or(factory_config.owner),
                OWNERSHIP_PROPOSAL,
            )
            .map_err(Into::into)
        }
        ExecuteMsg::DropOwnershipProposal {} => {
            let config = CONFIG.load(deps.storage)?;
            let factory_config = query_factory_config(&deps.querier, config.factory_addr)?;

            drop_ownership_proposal(
                deps,
                info,
                config.owner.unwrap_or(factory_config.owner),
                OWNERSHIP_PROPOSAL,
            )
            .map_err(Into::into)
        }
        ExecuteMsg::ClaimOwnership {} => {
            claim_ownership(deps, info, env, OWNERSHIP_PROPOSAL, |deps, new_owner| {
                CONFIG.update::<_, StdError>(deps.storage, |mut config| {
                    config.owner = Some(new_owner);
                    Ok(config)
                })?;

                Ok(())
            })
            .map_err(Into::into)
        }
    }
}

fn set_pool_id(deps: DepsMut, info: MessageInfo, pool_id: u64) -> Result<Response, ContractError> {
    if info.sender.as_str() != FACTORY_ADDRESS {
        return Err(ContractError::Unauthorized {});
    }

    POOL_ID.may_load(deps.storage)?.map_or_else(
        || {
            POOL_ID.save(deps.storage, &pool_id)?;
            Ok(())
        },
        |_| Err(ContractError::PoolIdAlreadySet {}),
    )?;

    Ok(Response::new().add_attribute("set_pool_id", pool_id.to_string()))
}

/// Provides liquidity in the pair with the specified input parameters.
///
/// * **assets** is an array with assets available in the pool.
///
/// * **slippage_tolerance** is an optional parameter which is used to specify how much
/// the pool price can move until the provide liquidity transaction goes through.
///
/// * **auto_stake** is an optional parameter which determines whether the LP tokens minted after
/// liquidity provision are automatically staked in the Generator contract on behalf of the LP token receiver.
///
/// * **receiver** is an optional parameter which defines the receiver of the LP tokens.
/// If no custom receiver is specified, the pair will mint LP tokens for the function caller.
pub fn provide_liquidity(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mut assets: Vec<Asset>,
    slippage_tolerance: Option<Decimal>,
    receiver: Option<String>,
) -> Result<Response, ContractError> {
    check_assets(deps.api, &assets)?;

    let mut config = CONFIG.load(deps.storage)?;
    if !check_pair_registered(
        deps.querier,
        &config.factory_addr,
        &config.pair_info.asset_infos,
    )? {
        return Err(ContractError::PairIsNotRegistered {});
    }

    match assets.len() {
        0 => {
            return Err(StdError::generic_err("Nothing to provide").into());
        }
        1 => {
            // Append omitted asset with explicit zero amount
            let (given_ind, _) = config
                .pair_info
                .asset_infos
                .iter()
                .find_position(|pool| pool.equal(&assets[0].info))
                .ok_or_else(|| ContractError::InvalidAsset(assets[0].info.to_string()))?;
            assets.push(Asset {
                info: config.pair_info.asset_infos[1 ^ given_ind].clone(),
                amount: Uint128::zero(),
            });
        }
        2 => {}
        _ => {
            return Err(ContractError::InvalidNumberOfAssets(
                config.pair_info.asset_infos.len(),
            ))
        }
    }

    info.funds
        .assert_coins_properly_sent(&assets, &config.pair_info.asset_infos)?;

    let precisions = Precisions::new(deps.storage)?;
    let mut pools = query_pools(deps.querier, &env.contract.address, &config, &precisions)?;

    if pools[0].info.equal(&assets[1].info) {
        assets.swap(0, 1);
    }

    // precisions.get_precision() also validates that the asset belongs to the pool
    let deposits = [
        Decimal256::with_precision(assets[0].amount, precisions.get_precision(&assets[0].info)?)?,
        Decimal256::with_precision(assets[1].amount, precisions.get_precision(&assets[1].info)?)?,
    ];

    let total_share = query_native_supply(&deps.querier, &config.pair_info.liquidity_token)?
        .to_decimal256(LP_TOKEN_PRECISION)?;

    // Initial provide can not be one-sided
    if total_share.is_zero() && (deposits[0].is_zero() || deposits[1].is_zero()) {
        return Err(ContractError::InvalidZeroAmount {});
    }

    let mut messages = vec![];
    for (i, pool) in pools.iter_mut().enumerate() {
        match &pool.info {
            AssetInfo::NativeToken { .. } => {
                // If the asset is native token, the pool balance is already increased
                // To calculate the total amount of deposits properly, we should subtract the user deposit from the pool
                pool.amount = pool.amount.checked_sub(deposits[i])?;
            }
            AssetInfo::Token { .. } => unreachable!("Token assets are not supported"),
        }
    }

    let mut new_xp = pools
        .iter()
        .enumerate()
        .map(|(ind, pool)| pool.amount + deposits[ind])
        .collect_vec();
    new_xp[1] *= config.pool_state.price_state.price_scale;

    let amp_gamma = config.pool_state.get_amp_gamma(&env);
    let new_d = calc_d(&new_xp, &amp_gamma)?;

    let share = if total_share.is_zero() {
        let xcp = get_xcp(new_d, config.pool_state.price_state.price_scale);
        let mint_amount =
            xcp.saturating_sub(MINIMUM_LIQUIDITY_AMOUNT.to_decimal256(LP_TOKEN_PRECISION)?);

        // share cannot become zero after minimum liquidity subtraction
        if mint_amount.is_zero() {
            return Err(ContractError::MinimumLiquidityAmountError {});
        }

        messages.push(MsgMint {
            sender: env.contract.address.to_string(),
            amount: Some(
                coin(
                    MINIMUM_LIQUIDITY_AMOUNT.u128(),
                    config.pair_info.liquidity_token.to_string(),
                )
                .into(),
            ),
            mint_to_address: env.contract.address.to_string(),
        });

        config.pool_state.price_state.xcp_profit_real = Decimal256::one();
        config.pool_state.price_state.xcp_profit = Decimal256::one();

        mint_amount
    } else {
        let mut old_xp = pools.iter().map(|a| a.amount).collect_vec();
        old_xp[1] *= config.pool_state.price_state.price_scale;
        let old_d = calc_d(&old_xp, &amp_gamma)?;
        let share = (total_share * new_d / old_d).saturating_sub(total_share);

        let mut ideposits = deposits;
        ideposits[1] *= config.pool_state.price_state.price_scale;

        share * (Decimal256::one() - calc_provide_fee(&ideposits, &new_xp, &config.pool_params))
    };

    // calculate accrued share
    let share_ratio = share / (total_share + share);
    let balanced_share = vec![
        new_xp[0] * share_ratio,
        new_xp[1] * share_ratio / config.pool_state.price_state.price_scale,
    ];
    let assets_diff = vec![
        deposits[0].diff(balanced_share[0]),
        deposits[1].diff(balanced_share[1]),
    ];

    let mut slippage = Decimal256::zero();

    // If deposit doesn't diverge too much from the balanced share, we don't update the price
    if assets_diff[0] >= MIN_TRADE_SIZE && assets_diff[1] >= MIN_TRADE_SIZE {
        slippage = assert_slippage_tolerance(
            &deposits,
            share,
            &config.pool_state.price_state,
            slippage_tolerance,
        )?;

        let last_price = assets_diff[0] / assets_diff[1];
        config.pool_state.update_price(
            &config.pool_params,
            &env,
            total_share + share,
            &new_xp,
            last_price,
        )?;
    }

    let share_uint128 = share.to_uint(LP_TOKEN_PRECISION)?;

    // Mint LP tokens for the sender or for the receiver (if set)
    let receiver = addr_opt_validate(deps.api, &receiver)?.unwrap_or_else(|| info.sender.clone());
    messages.push(MsgMint {
        sender: env.contract.address.to_string(),
        amount: Some(
            coin(
                share_uint128.u128(),
                config.pair_info.liquidity_token.to_string(),
            )
            .into(),
        ),
        mint_to_address: receiver.to_string(),
    });

    if config.track_asset_balances {
        for (i, pool) in pools.iter().enumerate() {
            BALANCES.save(
                deps.storage,
                &pool.info,
                &pool
                    .amount
                    .checked_add(deposits[i])?
                    .to_uint(precisions.get_precision(&pool.info)?)?,
                env.block.height,
            )?;
        }
    }

    CONFIG.save(deps.storage, &config)?;

    let attrs = vec![
        attr("action", "provide_liquidity"),
        attr("sender", info.sender),
        attr("receiver", receiver),
        attr("assets", format!("{}, {}", &assets[0], &assets[1])),
        attr("share", share_uint128),
        attr("slippage", slippage.to_string()),
    ];

    Ok(Response::new().add_messages(messages).add_attributes(attrs))
}

/// Withdraw liquidity from the pool.
///
/// * **assets** defines number of coins a user wants to withdraw per each asset.
///
/// * **receiver** address that will receive assets back from the pair contract
pub fn withdraw_liquidity(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    assets: Vec<Asset>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let share_denom = config.pair_info.liquidity_token.as_str();

    // Ensure correct LP tokens are sent
    let amount = must_pay(&info, share_denom)?;

    let precisions = Precisions::new(deps.storage)?;
    let pools = query_pools(
        deps.querier,
        &config.pair_info.contract_addr,
        &config,
        &precisions,
    )?;

    let total_share = query_native_supply(&deps.querier, &config.pair_info.liquidity_token)?;
    let mut messages = vec![];

    let refund_assets = if assets.is_empty() {
        // Usual withdraw (balanced)
        get_share_in_assets(&pools, amount.saturating_sub(Uint128::one()), total_share)
    } else {
        return Err(StdError::generic_err("Imbalanced withdraw is currently disabled").into());
    };

    // decrease XCP
    let mut xs = pools.iter().map(|a| a.amount).collect_vec();

    xs[0] -= refund_assets[0].amount;
    xs[1] -= refund_assets[1].amount;
    xs[1] *= config.pool_state.price_state.price_scale;
    let amp_gamma = config.pool_state.get_amp_gamma(&env);
    let d = calc_d(&xs, &amp_gamma)?;
    config.pool_state.price_state.xcp_profit_real =
        get_xcp(d, config.pool_state.price_state.price_scale)
            / (total_share - amount).to_decimal256(LP_TOKEN_PRECISION)?;

    let refund_assets = refund_assets
        .into_iter()
        .map(|asset| {
            let prec = precisions.get_precision(&asset.info).unwrap();
            asset.into_asset(prec)
        })
        .collect::<StdResult<Vec<_>>>()?;

    messages.extend(
        refund_assets
            .iter()
            .cloned()
            .map(|asset| asset.into_msg(&info.sender))
            .collect::<StdResult<Vec<_>>>()?,
    );
    messages.push(
        MsgBurn {
            sender: env.contract.address.to_string(),
            amount: Some(coin(amount.u128(), share_denom).into()),
            burn_from_address: env.contract.address.to_string(), // pair contract itself already holding these tokens
        }
        .into(),
    );

    if config.track_asset_balances {
        for (i, pool) in pools.iter().enumerate() {
            BALANCES.save(
                deps.storage,
                &pool.info,
                &pool
                    .amount
                    .to_uint(precisions.get_precision(&pool.info)?)?
                    .checked_sub(refund_assets[i].amount)?,
                env.block.height,
            )?;
        }
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("action", "withdraw_liquidity"),
        attr("receiver", info.sender),
        attr("withdrawn_share", amount),
        attr("refund_assets", refund_assets.iter().join(", ")),
    ]))
}

/// Performs a swap operation with the specified parameters.
///
/// From external perspective this function behaves in the same way as in other Astroport pairs.
/// However, internally it forwards swap request to the Osmosis DEX module.
pub fn execute_swap(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    offer_asset: Asset,
    belief_price: Option<Decimal>,
    max_spread: Option<Decimal>,
    to: Option<String>,
) -> Result<Response, ContractError> {
    offer_asset.assert_sent_native_token_balance(&info)?;

    let config = CONFIG.load(deps.storage)?;
    if !config.pair_info.asset_infos.contains(&offer_asset.info) {
        return Err(ContractError::InvalidAsset(offer_asset.info.to_string()));
    }

    let (offer_ind, _) = config
        .pair_info
        .asset_infos
        .iter()
        .find_position(|asset| asset.equal(&offer_asset.info))
        .unwrap();
    let token_out_denom = config.pair_info.asset_infos[1 ^ offer_ind].to_string();

    SWAP_PARAMS.save(
        deps.storage,
        &SwapParams {
            belief_price,
            max_spread,
            sender: info.sender,
            to: addr_opt_validate(deps.api, &to)?,
        },
    )?;
    let dispatch_swap_msg = MsgSwapExactAmountIn {
        sender: env.contract.address.to_string(),
        routes: vec![SwapAmountInRoute {
            // If for some reason pool id was not set on instantiation any swap will fail which is totally safe.
            // Pool contract must know its pool id in Osmosis DEX module.
            pool_id: POOL_ID.load(deps.storage)?,
            // This is not needed as we currently support only pairs. However, we define out denom just for clarity.
            token_out_denom,
        }],
        token_in: Some(offer_asset.as_coin()?.into()),
        // We don't care about this field as all necessary parameters are passed through SWAP_PARAMS state
        token_out_min_amount: "1".to_string(),
    };
    Ok(Response::new()
        .add_attribute("action", "dispatch_swap")
        .add_message(dispatch_swap_msg))
}

/// Performs an swap operation with the specified parameters.
///
/// * **sender** is the sender of the swap operation.
///
/// * **offer_asset** proposed asset for swapping.
///
/// * **belief_price** is used to calculate the maximum swap spread.
///
/// * **max_spread** sets the maximum spread of the swap operation.
///
/// * **to** sets the recipient of the swap operation.
pub fn internal_swap(
    deps: DepsMut,
    env: Env,
    sender: Addr,
    offer_asset: Asset,
    belief_price: Option<Decimal>,
    max_spread: Option<Decimal>,
    to: Option<Addr>,
) -> Result<Response, ContractError> {
    let precisions = Precisions::new(deps.storage)?;
    let offer_asset_prec = precisions.get_precision(&offer_asset.info)?;
    let offer_asset_dec = offer_asset.to_decimal_asset(offer_asset_prec)?;
    let mut config = CONFIG.load(deps.storage)?;

    let mut pools = query_pools(deps.querier, &env.contract.address, &config, &precisions)?;

    let (offer_ind, _) = pools
        .iter()
        .find_position(|asset| asset.info == offer_asset_dec.info)
        .ok_or_else(|| ContractError::InvalidAsset(offer_asset_dec.info.to_string()))?;
    let ask_ind = 1 ^ offer_ind;
    let ask_asset_prec = precisions.get_precision(&pools[ask_ind].info)?;

    // Offer pool must have offer amount in it. We need to subtract it from the pool balance
    pools[offer_ind].amount -= offer_asset_dec.amount;

    before_swap_check(&pools, offer_asset_dec.amount)?;

    let mut xs = pools.iter().map(|asset| asset.amount).collect_vec();

    // Get fee info from the factory
    let fee_info = query_fee_info(
        &deps.querier,
        &config.factory_addr,
        config.pair_info.pair_type.clone(),
    )?;
    let mut maker_fee_share = Decimal256::zero();
    if fee_info.fee_address.is_some() {
        maker_fee_share = fee_info.maker_fee_rate.into();
    }

    let swap_result = compute_swap(
        &xs,
        offer_asset_dec.amount,
        ask_ind,
        &config,
        &env,
        maker_fee_share,
        Decimal256::zero(),
    )?;
    xs[offer_ind] += offer_asset_dec.amount;
    xs[ask_ind] -= swap_result.dy + swap_result.maker_fee;

    let return_amount = swap_result.dy.to_uint(ask_asset_prec)?;
    let spread_amount = swap_result.spread_fee.to_uint(ask_asset_prec)?;
    assert_max_spread(
        belief_price,
        max_spread,
        offer_asset.amount,
        return_amount,
        spread_amount,
    )?;

    let total_share = query_native_supply(&deps.querier, &config.pair_info.liquidity_token)?
        .to_decimal256(LP_TOKEN_PRECISION)?;

    // Skip very small trade sizes which could significantly mess up the price due to rounding errors,
    // especially if token precisions are 18.
    if (swap_result.dy + swap_result.maker_fee + swap_result.share_fee) >= MIN_TRADE_SIZE
        && offer_asset_dec.amount >= MIN_TRADE_SIZE
    {
        let last_price = swap_result.calc_last_price(offer_asset_dec.amount, offer_ind);

        // update_price() works only with internal representation
        xs[1] *= config.pool_state.price_state.price_scale;
        config
            .pool_state
            .update_price(&config.pool_params, &env, total_share, &xs, last_price)?;
    }

    let receiver = to.unwrap_or_else(|| sender.clone());

    let mut messages = vec![pools[ask_ind]
        .info
        .with_balance(return_amount)
        .into_msg(&receiver)?];

    let mut maker_fee = Uint128::zero();
    if let Some(fee_address) = fee_info.fee_address {
        maker_fee = swap_result.maker_fee.to_uint(ask_asset_prec)?;
        if !maker_fee.is_zero() {
            let fee = pools[ask_ind].info.with_balance(maker_fee);
            messages.push(fee.into_msg(fee_address)?);
        }
    }

    // Store observation from precommit data
    accumulate_swap_sizes(deps.storage, &env)?;

    // Store time series data in precommit observation.
    // Skipping small unsafe values which can seriously mess oracle price due to rounding errors.
    // This data will be reflected in observations in the next action.
    if offer_asset_dec.amount >= MIN_TRADE_SIZE && swap_result.dy >= MIN_TRADE_SIZE {
        let (base_amount, quote_amount) = if offer_ind == 0 {
            (offer_asset.amount, return_amount)
        } else {
            (return_amount, offer_asset.amount)
        };
        PrecommitObservation::save(deps.storage, &env, base_amount, quote_amount)?;
    }

    CONFIG.save(deps.storage, &config)?;

    if config.track_asset_balances {
        BALANCES.save(
            deps.storage,
            &pools[offer_ind].info,
            &(pools[offer_ind].amount + offer_asset_dec.amount).to_uint(offer_asset_prec)?,
            env.block.height,
        )?;
        BALANCES.save(
            deps.storage,
            &pools[ask_ind].info,
            &(pools[ask_ind].amount.to_uint(ask_asset_prec)? - return_amount - maker_fee),
            env.block.height,
        )?;
    }

    let response_data = to_json_binary(&SwapExactAmountInResponseData {
        token_out_amount: return_amount,
    })?;

    Ok(Response::new()
        .add_messages(messages)
        .add_attributes(vec![
            attr("action", "swap"),
            attr("sender", sender),
            attr("receiver", receiver),
            attr("offer_asset", offer_asset_dec.info.to_string()),
            attr("ask_asset", pools[ask_ind].info.to_string()),
            attr("offer_amount", offer_asset.amount),
            attr("return_amount", return_amount),
            attr("spread_amount", spread_amount),
            attr(
                "commission_amount",
                swap_result.total_fee.to_uint(ask_asset_prec)?,
            ),
            attr("maker_fee_amount", maker_fee),
        ])
        .set_data(response_data))
}

/// Updates the pool configuration with the specified parameters in the `params` variable.
///
/// * **params** new parameter values in [`Binary`] form.
pub fn update_config(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    params: Binary,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let factory_config = query_factory_config(&deps.querier, &config.factory_addr)?;

    let owner = config.owner.as_ref().unwrap_or(&factory_config.owner);
    if info.sender != *owner {
        return Err(ContractError::Unauthorized {});
    }

    let action = match from_json::<ConcentratedPoolUpdateParams>(&params)? {
        ConcentratedPoolUpdateParams::Update(update_params) => {
            config.pool_params.update_params(update_params)?;
            "update_params"
        }
        ConcentratedPoolUpdateParams::Promote(promote_params) => {
            config.pool_state.promote_params(&env, promote_params)?;
            "promote_params"
        }
        ConcentratedPoolUpdateParams::StopChangingAmpGamma {} => {
            config.pool_state.stop_promotion(&env);
            "stop_changing_amp_gamma"
        }
        ConcentratedPoolUpdateParams::EnableAssetBalancesTracking {} => {
            if config.track_asset_balances {
                return Err(ContractError::AssetBalancesTrackingIsAlreadyEnabled {});
            }
            config.track_asset_balances = true;

            let pools = config
                .pair_info
                .query_pools(&deps.querier, &config.pair_info.contract_addr)?;

            for pool in pools.iter() {
                BALANCES.save(deps.storage, &pool.info, &pool.amount, env.block.height)?;
            }

            "enable_asset_balances_tracking"
        }
        ConcentratedPoolUpdateParams::EnableFeeShare { .. }
        | ConcentratedPoolUpdateParams::DisableFeeShare => Err(StdError::generic_err(
            "Fee sharing is not supported in this version",
        ))?,
    };
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", action))
}
