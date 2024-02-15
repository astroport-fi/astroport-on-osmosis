use astroport::asset::validate_native_denom;
use astroport::common::{claim_ownership, drop_ownership_proposal, propose_new_owner};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, ensure, Coin, Decimal, DepsMut, Env, MessageInfo, ReplyOn, Response, StdError, SubMsg,
};
use itertools::Itertools;
use osmosis_std::types::cosmos::base::v1beta1::Coin as OsmoCoin;
use osmosis_std::types::osmosis::poolmanager::v1beta1::{MsgSwapExactAmountIn, PoolmanagerQuerier};

use astroport_on_osmosis::maker::{ExecuteMsg, PoolRoute, MAX_ALLOWED_SPREAD};

use crate::error::ContractError;
use crate::reply::POST_COLLECT_REPLY_ID;
use crate::state::{RouteStep, CONFIG, LAST_COLLECT_TS, OWNERSHIP_PROPOSAL, ROUTES};
use crate::utils::{query_out_amount, validate_cooldown, RoutesBuilder};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Collect { assets } => collect(deps, env, assets),
        ExecuteMsg::UpdateConfig {
            astro_denom,
            fee_receiver,
            max_spread,
            collect_cooldown,
        } => update_config(
            deps,
            info,
            astro_denom,
            fee_receiver,
            max_spread,
            collect_cooldown,
        ),
        ExecuteMsg::SetPoolRoutes(routes) => set_pool_routes(deps, info, routes),
        ExecuteMsg::ProposeNewOwner { owner, expires_in } => {
            let config = CONFIG.load(deps.storage)?;
            propose_new_owner(
                deps,
                info,
                env,
                owner,
                expires_in,
                config.owner,
                OWNERSHIP_PROPOSAL,
            )
            .map_err(Into::into)
        }
        ExecuteMsg::DropOwnershipProposal {} => {
            let config = CONFIG.load(deps.storage)?;
            drop_ownership_proposal(deps, info, config.owner, OWNERSHIP_PROPOSAL)
                .map_err(Into::into)
        }
        ExecuteMsg::ClaimOwnership {} => {
            claim_ownership(deps, info, env, OWNERSHIP_PROPOSAL, |deps, new_owner| {
                CONFIG
                    .update::<_, StdError>(deps.storage, |mut v| {
                        v.owner = new_owner;
                        Ok(v)
                    })
                    .map(|_| ())
            })
            .map_err(Into::into)
        }
    }
}

pub fn collect(deps: DepsMut, env: Env, assets: Vec<Coin>) -> Result<Response, ContractError> {
    ensure!(!assets.is_empty(), ContractError::EmptyAssets {});

    let config = CONFIG.load(deps.storage)?;

    // Allowing collect only once per cooldown period
    LAST_COLLECT_TS.update(deps.storage, |last_ts| match config.collect_cooldown {
        Some(cd_period) if env.block.time.seconds() < last_ts + cd_period => {
            Err(ContractError::Cooldown {
                next_collect_ts: last_ts + cd_period,
            })
        }
        _ => Ok(env.block.time.seconds()),
    })?;

    let mut messages = vec![];
    let mut attrs = vec![attr("action", "collect")];

    let mut routes_builder = RoutesBuilder::default();
    for asset in assets {
        let balance = deps
            .querier
            .query_balance(&env.contract.address, &asset.denom)
            .map(|coin| Coin {
                amount: asset.amount.min(coin.amount),
                ..asset
            })?;

        // Skip silently if the balance is zero.
        // This allows our bot to operate normally without manual adjustments.
        if balance.amount.is_zero() {
            continue;
        }

        attrs.push(attr("collected_asset", &balance.to_string()));

        let built_routes =
            routes_builder.build_routes(deps.storage, &balance.denom, &config.astro_denom)?;

        attrs.push(attr("route_taken", built_routes.route_taken));

        let out_amount = query_out_amount(
            deps.querier,
            env.block.time.seconds(),
            &balance,
            &built_routes.routes,
        )?;
        let min_out_amount = (Decimal::one() - config.max_spread) * out_amount;

        let swap_msg = MsgSwapExactAmountIn {
            sender: env.contract.address.to_string(),
            routes: built_routes.routes,
            token_in: Some(OsmoCoin {
                denom: balance.denom.clone(),
                amount: balance.amount.to_string(),
            }),
            token_out_min_amount: min_out_amount.to_string(),
        };
        messages.push(SubMsg::new(swap_msg));
    }

    messages
        .last_mut()
        .and_then(|submsg| {
            submsg.id = POST_COLLECT_REPLY_ID;
            submsg.reply_on = ReplyOn::Success;
            Some(())
        })
        .ok_or(ContractError::NothingToCollect {})?;

    Ok(Response::new()
        .add_submessages(messages)
        .add_attributes(attrs))
}

pub fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    astro_denom: Option<String>,
    fee_receiver: Option<String>,
    max_spread: Option<Decimal>,
    collect_cooldown: Option<u64>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    ensure!(info.sender == config.owner, ContractError::Unauthorized {});

    let mut attrs = vec![];

    if let Some(astro_denom) = astro_denom {
        validate_native_denom(&astro_denom)?;
        attrs.push(attr("new_astro_denom", &astro_denom));
        config.astro_denom = astro_denom;
    }

    if let Some(fee_receiver) = fee_receiver {
        config.satellite = deps.api.addr_validate(&fee_receiver)?;
        attrs.push(attr("new_fee_receiver", &fee_receiver));
    }

    if let Some(max_spread) = max_spread {
        ensure!(
            max_spread <= MAX_ALLOWED_SPREAD,
            ContractError::MaxSpreadTooHigh {}
        );
        attrs.push(attr("new_max_spread", max_spread.to_string()));
        config.max_spread = max_spread;
    }

    if let Some(collect_cooldown_val) = collect_cooldown {
        validate_cooldown(collect_cooldown)?;
        attrs.push(attr(
            "new_collect_cooldown",
            collect_cooldown_val.to_string(),
        ));
        config.collect_cooldown = Some(collect_cooldown_val);
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attributes(attrs))
}

pub fn set_pool_routes(
    deps: DepsMut,
    info: MessageInfo,
    routes: Vec<PoolRoute>,
) -> Result<Response, ContractError> {
    ensure!(!routes.is_empty(), ContractError::EmptyRoutes {});
    ensure!(
        routes.iter().map(|r| &r.denom_in).all_unique(),
        ContractError::DuplicatedRoutes {}
    );

    let config = CONFIG.load(deps.storage)?;
    ensure!(info.sender == config.owner, ContractError::Unauthorized {});

    let mut attrs = vec![attr("action", "set_pool_routes")];

    let mut routes_builder = RoutesBuilder::default();

    for route in &routes {
        // Sanity checks via osmosis pool manager
        let pm_quierier = PoolmanagerQuerier::new(&deps.querier);
        let pool_denoms = pm_quierier
            .total_pool_liquidity(route.pool_id)?
            .liquidity
            .into_iter()
            .map(|coin| coin.denom)
            .collect_vec();

        ensure!(
            pool_denoms.contains(&route.denom_in),
            ContractError::InvalidPoolDenom {
                pool_id: route.pool_id,
                denom: route.denom_in.to_owned()
            }
        );
        ensure!(
            pool_denoms.contains(&route.denom_out),
            ContractError::InvalidPoolDenom {
                pool_id: route.pool_id,
                denom: route.denom_out.to_owned()
            }
        );

        if ROUTES.has(deps.storage, &route.denom_in) {
            attrs.push(attr("updated_route", &route.denom_in));
        }

        let route_step = RouteStep {
            denom_out: route.denom_out.to_owned(),
            pool_id: route.pool_id,
        };

        // If route exists then this iteration updates the route.
        ROUTES.save(deps.storage, &route.denom_in, &route_step)?;

        routes_builder
            .routes_cache
            .insert(route.denom_in.clone(), route_step);
    }

    // Check all updated routes end up in ASTRO. It also checks for possible loops.
    routes.iter().try_for_each(|route| {
        routes_builder
            .build_routes(deps.storage, &route.denom_in, &config.astro_denom)
            .map(|_| ())
    })?;

    Ok(Response::new().add_attributes(attrs))
}
