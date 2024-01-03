use astroport::asset::{native_asset_info, AssetInfo, AssetInfoExt};
use astroport::cosmwasm_ext::{DecimalToInteger, IntegerToDecimal};
use astroport::observation::PrecommitObservation;
use astroport::pair::MIN_TRADE_SIZE;
use astroport::querier::query_fee_info;
use astroport_pcl_common::state::Precisions;
use astroport_pcl_common::utils::{compute_offer_amount, compute_swap};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, ensure, to_json_binary, Coin, Decimal, Decimal256, DepsMut, Env, Response, StdError,
    Uint128,
};

use astroport_on_osmosis::pair_pcl::{SudoMessage, SwapExactAmountOutResponseData};

use crate::contract::{internal_swap, LP_TOKEN_PRECISION};
use crate::error::ContractError;
use crate::state::{BALANCES, CONFIG, SWAP_PARAMS};
use crate::utils::{accumulate_swap_sizes, query_native_supply};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(deps: DepsMut, env: Env, msg: SudoMessage) -> Result<Response, ContractError> {
    match msg {
        SudoMessage::SwapExactAmountIn {
            sender,
            token_in,
            token_out_min_amount,
            ..
        } => {
            let mut sender = deps.api.addr_validate(&sender)?;
            let offer_asset = native_asset_info(token_in.denom).with_balance(token_in.amount);

            let mut belief_price = Some(Decimal::from_ratio(token_in.amount, token_out_min_amount));
            // Osmosis applies slippage on their frontend side hence we won't disrupt
            // this logic with our additional default 0.02% slippage tolerance.
            let mut max_spread = Some(Decimal::zero());
            let mut to = None;
            // If swap was dispatched from Astroport pair it must have SWAP_PARAMS in the storage
            if let Some(swap_params) = SWAP_PARAMS.may_load(deps.storage)? {
                belief_price = swap_params.belief_price;
                max_spread = swap_params.max_spread;
                sender = swap_params.sender;
                to = swap_params.to;

                // Remove params so they won't be used if SwapExactAmountIn is called directly from the DEX module
                SWAP_PARAMS.remove(deps.storage);
            }

            internal_swap(deps, env, sender, offer_asset, belief_price, max_spread, to)
                .map(|res| res.add_attribute("method", "swap_exact_amount_in"))
        }
        SudoMessage::SwapExactAmountOut {
            sender,
            token_in_denom,
            token_in_max_amount,
            token_out,
            ..
        } => swap_exact_amount_out(
            deps,
            env,
            sender,
            token_in_denom,
            token_in_max_amount,
            token_out,
        ),
        SudoMessage::SetActive { .. } => unimplemented!("SetActive is not implemented"),
    }
}

/// Osmosis cosmwasmpool module guarantees that token_in_max_amount is always sent to the contract
/// https://github.com/osmosis-labs/osmosis/blob/294302637a47ffec5cafc0c1953e88a54390b20e/x/cosmwasmpool/pool_module.go#L288-L293
fn swap_exact_amount_out(
    deps: DepsMut,
    env: Env,
    sender: String,
    token_in_denom: String,
    token_in_max_amount: Uint128,
    token_out: Coin,
) -> Result<Response, ContractError> {
    if token_in_denom == token_out.denom {
        return Err(StdError::generic_err(format!(
            "Invalid swap: {token_in_denom} to {token_in_denom}"
        ))
        .into());
    }

    let mut config = CONFIG.load(deps.storage)?;
    let precisions = Precisions::new(deps.storage)?;
    let ask_asset = native_asset_info(token_out.denom).with_balance(token_out.amount);
    let ask_asset_prec = precisions.get_precision(&ask_asset.info)?;
    let ask_amount_dec = token_out.amount.to_decimal256(ask_asset_prec)?;

    let mut pools = config
        .pair_info
        .query_pools(&deps.querier, &env.contract.address)?;

    let ask_ind = pools
        .iter()
        .position(|asset| asset.info == ask_asset.info)
        .ok_or_else(|| ContractError::InvalidAsset(ask_asset.info.to_string()))?;
    let offer_ind = pools
        .iter()
        .position(|asset| asset.info == AssetInfo::native(&token_in_denom))
        .ok_or(ContractError::InvalidAsset(token_in_denom))?;
    let offer_asset_prec = precisions.get_precision(&pools[offer_ind].info)?;

    // Offer pool must have token_in_max_amount in it. We need to subtract it from the pool balance
    pools[offer_ind].amount -= token_in_max_amount;

    let mut xs = pools
        .iter()
        .map(|asset| {
            asset
                .amount
                .to_decimal256(precisions.get_precision(&asset.info)?)
                .map_err(Into::into)
        })
        .collect::<Result<Vec<_>, ContractError>>()?;

    let (offer_amount, ..) = compute_offer_amount(&xs, ask_amount_dec, ask_ind, &config, &env)?;

    let offer_amount = offer_amount.to_uint(offer_asset_prec)?;
    ensure!(
        offer_amount <= token_in_max_amount,
        StdError::generic_err(
            format!("Not enough tokens to perform swap. Need {offer_amount} but token_in_max_amount is {token_in_max_amount}")
        )
    );

    let offer_asset = pools[offer_ind].info.with_balance(offer_amount);

    // Since PCL has dynamic fees reverse simulation is not able to predict fees upfront and applies max possible fee.
    // Pretending there was direct swap to get 100% accurate result.
    let offer_asset_prec = precisions.get_precision(&offer_asset.info)?;
    let offer_asset_dec = offer_asset.to_decimal_asset(offer_asset_prec)?;

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

    let total_share = query_native_supply(&deps.querier, &config.pair_info.liquidity_token)?
        .to_decimal256(LP_TOKEN_PRECISION)?;

    let last_price = swap_result.calc_last_price(offer_asset_dec.amount, offer_ind);

    // update_price() works only with internal representation
    xs[1] *= config.pool_state.price_state.price_scale;
    config
        .pool_state
        .update_price(&config.pool_params, &env, total_share, &xs, last_price)?;

    let mut messages = vec![pools[ask_ind]
        .info
        .with_balance(return_amount)
        .into_msg(&sender)?];

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
            &(pools[offer_ind].amount + offer_asset_dec.amount.to_uint(offer_asset_prec)?),
            env.block.height,
        )?;
        BALANCES.save(
            deps.storage,
            &pools[ask_ind].info,
            &(pools[ask_ind].amount - return_amount - maker_fee),
            env.block.height,
        )?;
    }

    let response_data = to_json_binary(&SwapExactAmountOutResponseData {
        token_in_amount: offer_asset.amount,
    })?;

    Ok(Response::new()
        .add_messages(messages)
        .add_attributes([
            attr("method", "swap_exact_amount_out"),
            attr("sender", sender),
            attr("offer_asset", offer_asset.info.to_string()),
            attr("ask_asset", ask_asset.info.to_string()),
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
