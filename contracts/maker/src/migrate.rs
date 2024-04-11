#![cfg(not(tarpaulin_include))]

use astroport::asset::AssetInfo;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{Addr, DepsMut, Env, MessageInfo, Response};
use cw_storage_plus::Map;

use astroport_on_osmosis::maker::InstantiateMsg;

use crate::error::ContractError;
use crate::instantiate::{instantiate, CONTRACT_NAME, CONTRACT_VERSION};

const EXPECTED_CONTRACT_NAME: &str = "astroport-maker";
const EXPECTED_CONTRACT_VERSION: &str = "1.4.0";

/// This migration is used to convert the dummy maker contract on Osmosis into real working Maker.
///
/// Mainnet contract which is only subject of this migration: https://celatone.osmosis.zone/osmosis-1/contracts/osmo1kl96qztvtrz9h8873jlcl9fz0fmgtdgfdw65shm9frr9y7xvc83qstjd8h
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, env: Env, msg: InstantiateMsg) -> Result<Response, ContractError> {
    cw2::assert_contract_version(
        deps.storage,
        EXPECTED_CONTRACT_NAME,
        EXPECTED_CONTRACT_VERSION,
    )?;

    // Clear old state
    Map::<String, AssetInfo>::new("bridges").clear(deps.storage);

    let cw_admin = deps
        .querier
        .query_wasm_contract_info(&env.contract.address)?
        .admin
        .unwrap();
    // Even though info object is ignored in instantiate, we provide it for clarity
    let info = MessageInfo {
        sender: Addr::unchecked(cw_admin),
        funds: vec![],
    };
    // Instantiate state.
    // Config and cw2 info will be overwritten.
    let contract_version = cw2::get_contract_version(deps.storage)?;

    instantiate(deps, env, info, msg).map(|resp| {
        resp.add_attributes([
            ("previous_contract_name", contract_version.contract.as_str()),
            (
                "previous_contract_version",
                contract_version.version.as_str(),
            ),
            ("new_contract_name", CONTRACT_NAME),
            ("new_contract_version", CONTRACT_VERSION),
        ])
    })
}
