use astroport::asset::validate_native_denom;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{ensure, DepsMut, Env, MessageInfo, Response};
use cw2::set_contract_version;

use astroport_on_osmosis::maker::{Config, InstantiateMsg, MAX_ALLOWED_SPREAD};

use crate::error::ContractError;
use crate::state::{CONFIG, LAST_COLLECT_TS};
use crate::utils::validate_cooldown;

/// Contract name for cw2 info
const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version for cw2 info
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    validate_native_denom(&msg.astro_denom)?;
    ensure!(
        msg.max_spread <= MAX_ALLOWED_SPREAD,
        ContractError::MaxSpreadTooHigh {}
    );
    validate_cooldown(msg.collect_cooldown)?;
    LAST_COLLECT_TS.save(deps.storage, &env.block.time.seconds())?;

    CONFIG.save(
        deps.storage,
        &Config {
            owner: deps.api.addr_validate(&msg.owner)?,
            astro_denom: msg.astro_denom,
            satellite: deps.api.addr_validate(&msg.satellite)?,
            max_spread: msg.max_spread,
            collect_cooldown: None,
        },
    )?;

    Ok(Response::new())
}
