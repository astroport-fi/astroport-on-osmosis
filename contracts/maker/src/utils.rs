use std::collections::HashMap;
use std::str::FromStr;

use cosmwasm_std::{
    ensure, Coin, Decimal, Fraction, QuerierWrapper, StdError, StdResult, Storage, Uint128,
};
use itertools::Itertools;
use osmosis_std::types::osmosis::poolmanager::v1beta1::{PoolmanagerQuerier, SwapAmountInRoute};

use astroport_on_osmosis::maker::{COOLDOWN_LIMITS, MAX_SWAPS_DEPTH};

use crate::error::ContractError;
use crate::state::{RouteStep, ROUTES};

/// Validate cooldown value is within the allowed range
pub fn validate_cooldown(maybe_cooldown: Option<u64>) -> Result<(), ContractError> {
    if let Some(collect_cooldown) = maybe_cooldown {
        if !COOLDOWN_LIMITS.contains(&collect_cooldown) {
            return Err(ContractError::IncorrectCooldown {
                min: *COOLDOWN_LIMITS.start(),
                max: *COOLDOWN_LIMITS.end(),
            });
        }
    }

    Ok(())
}

/// Query how much amount of denom_out we get for denom_in.
/// Copied from Mars: https://github.com/mars-protocol/contracts/blob/28edbfb37768cc6c73b854ce5d95b2655951af58/contracts/swapper/osmosis/src/route.rs#L193
///
/// Example calculation:
/// If we want to swap atom to usdc and configured routes are [pool_1 (atom/osmo), pool_69 (osmo/usdc)] (no direct pool of atom/usdc):
/// 1) query pool_1 to get price for atom/osmo
/// 2) query pool_69 to get price for osmo/usdc
/// 3) atom/usdc = (price for atom/osmo) * (price for osmo/usdc)
/// 4) usdc_out_amount = (atom amount) * (price for atom/usdc)
pub fn query_out_amount(
    querier: QuerierWrapper,
    coin_in: &Coin,
    steps: &[SwapAmountInRoute],
) -> Result<Uint128, ContractError> {
    let mut price = Decimal::one();
    let mut denom_in = coin_in.denom.clone();
    for step in steps {
        let step_price = query_spot_price(querier, step.pool_id, &denom_in, &step.token_out_denom)?;
        price = price.checked_mul(step_price)?;
        denom_in.clone_from(&step.token_out_denom);
    }

    let out_amount = coin_in
        .amount
        .checked_multiply_ratio(price.numerator(), price.denominator())?;
    Ok(out_amount)
}

/// Query spot price of a coin, denominated in quote_denom.
pub fn query_spot_price(
    querier: QuerierWrapper,
    pool_id: u64,
    base_denom: &str,
    quote_denom: &str,
) -> StdResult<Decimal> {
    let spot_price_res = PoolmanagerQuerier::new(&querier).spot_price(
        pool_id,
        base_denom.to_string(),
        quote_denom.to_string(),
    )?;
    let price = Decimal::from_str(&spot_price_res.spot_price)?;
    if price.is_zero() {
        Err(StdError::generic_err(format!(
            "Zero spot price. pool_id {pool_id} base_denom {base_denom} quote_denom {quote_denom}",
        )))
    } else {
        Ok(price)
    }
}

#[derive(Default)]
pub struct RoutesBuilder {
    pub routes_cache: HashMap<String, RouteStep>,
}

pub struct BuiltRoutes {
    pub routes: Vec<SwapAmountInRoute>,
    pub route_taken: String,
}

impl RoutesBuilder {
    pub fn build_routes(
        &mut self,
        storage: &dyn Storage,
        denom_in: &str,
        astro_denom: &str,
    ) -> Result<BuiltRoutes, ContractError> {
        let mut prev_denom = denom_in.to_string();
        let mut routes = vec![];

        for _ in 0..MAX_SWAPS_DEPTH {
            if prev_denom == astro_denom {
                break;
            }

            let step = if let Some(found) = self.routes_cache.get(&prev_denom).cloned() {
                found
            } else {
                let step =
                    ROUTES
                        .may_load(storage, &prev_denom)?
                        .ok_or(ContractError::RouteNotFound {
                            denom: prev_denom.to_string(),
                        })?;
                self.routes_cache
                    .insert(prev_denom.to_string(), step.clone());

                step
            };

            routes.push(SwapAmountInRoute {
                pool_id: step.pool_id,
                token_out_denom: step.denom_out.clone(),
            });

            prev_denom = step.denom_out;
        }

        let route_denoms = routes
            .iter()
            .map(|r| r.token_out_denom.clone())
            .collect_vec();
        let route_taken = [vec![denom_in.to_string()], route_denoms]
            .concat()
            .join(" -> ");

        ensure!(
            prev_denom == astro_denom,
            ContractError::FailedToBuildRoute {
                denom: denom_in.to_string(),
                route_taken,
            }
        );

        Ok(BuiltRoutes {
            routes,
            route_taken,
        })
    }
}
