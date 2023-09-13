use astroport::asset::{native_asset_info, AssetInfoExt};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_binary, Decimal, DepsMut, Env, Response};

use astroport_on_osmosis::pair_pcl::{SudoMessage, SwapExactAmountInResponseData};

use crate::contract::internal_swap;
use crate::error::ContractError;
use crate::state::SWAP_PARAMS;

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

            let response_data = to_binary(&SwapExactAmountInResponseData {
                // TODO: extract amount from the response. Osmosis: "It’s needed in case of multi pool swap routing"
                // https://github.com/osmosis-labs/osmosis/blob/294302637a47ffec5cafc0c1953e88a54390b20e/x/poolmanager/router.go#L102C3-L113
                token_out_amount: 1u8.into(),
            })?;
            internal_swap(deps, env, sender, offer_asset, belief_price, max_spread, to).map(|res| {
                res.add_attribute("method", "swap_exact_amount_in")
                    .set_data(response_data)
            })
        }
        SudoMessage::SwapExactAmountOut { .. } => {
            todo!("Unsafe function! Osmosis doesn't pull out expected coins from sender balance!")
            // TODO: implement according to internal Osmosis logic described here https://github.com/osmosis-labs/osmosis/blob/294302637a47ffec5cafc0c1953e88a54390b20e/x/cosmwasmpool/pool_module.go#L272-L324
            /*let sender = deps.api.addr_validate(&sender)?;
            let ask_asset = native_asset_info(token_out.denom).with_balance(token_out.amount);

            let (_, offer_asset) =
                query_reverse_simulation(deps.as_ref(), env.clone(), ask_asset.clone())?;

            ensure!(
                offer_asset.amount <= token_in_max_amount,
                StdError::generic_err(
                    format!("Not enough tokens to perform swap. Need {} but token_in_max_amount is {token_in_max_amount}", ask_asset.to_string())
                )
            );

            // Since PCL has dynamic fees reverse simulation is not able to predict fees upfront and applies max possible fee.
            // Pretending there was direct swap to get 100% accurate result.
            let belief_price = Some(Decimal::from_ratio(token_in_max_amount, token_out.amount));
            let max_spread = Some(Decimal::zero());
            let to = None;

            internal_swap(
                deps,
                env,
                sender,
                offer_asset.clone(),
                belief_price,
                max_spread,
                to,
            )
            .map(|res| {
                res.add_attribute("method", "swap_exact_amount_out")
                    .set_data(to_binary(&SwapExactAmountOutResponseData {
                        token_in_amount: offer_asset.amount,
                    })?)
            })*/
        }
        SudoMessage::SetActive { .. } => todo!("Do we need this?"),
    }
}
