use astroport::asset::PairInfo;
use astroport::factory::PairType;
use astroport_pcl_common::state::{Config, PoolState};
use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{Addr, Reply, StdError, SubMsgResponse, SubMsgResult};
use osmosis_std::types::osmosis::tokenfactory::v1beta1::MsgCreateDenomResponse;

use astroport_on_osmosis::pair_pcl::ExecuteMsg;
use astroport_pcl_osmo::contract::{execute, reply};
use astroport_pcl_osmo::error::ContractError;
use astroport_pcl_osmo::state::CONFIG;

const FACTORY_ADDRESS: &str = include_str!("../src/factory_address");

#[test]
fn test_replies() {
    let mut deps = mock_dependencies();

    let config = Config {
        pair_info: PairInfo {
            asset_infos: vec![],
            contract_addr: Addr::unchecked(""),
            liquidity_token: Addr::unchecked("already_set"),
            pair_type: PairType::Xyk {},
        },
        factory_addr: Addr::unchecked(""),
        pool_params: Default::default(),
        pool_state: PoolState {
            initial: Default::default(),
            future: Default::default(),
            future_time: 0,
            initial_time: 0,
            price_state: Default::default(),
        },
        owner: None,
        track_asset_balances: false,
    };
    CONFIG.save(deps.as_mut().storage, &config).unwrap();

    let reply_msg = Reply {
        id: 1,
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![],
            data: Some(
                MsgCreateDenomResponse {
                    new_token_denom: "other".to_string(),
                }
                .into(),
            ),
        }),
    };
    let err = reply(deps.as_mut(), mock_env(), reply_msg).unwrap_err();
    assert_eq!(
        err,
        ContractError::Std(StdError::generic_err(
            "Liquidity token denom is already set"
        ))
    );

    let reply_msg = Reply {
        id: 3000,
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![],
            data: None,
        }),
    };
    let err = reply(deps.as_mut(), mock_env(), reply_msg.clone()).unwrap_err();
    assert_eq!(
        err,
        ContractError::Std(StdError::generic_err(format!(
            "Unknown reply id: {}",
            reply_msg.id
        )))
    );
}

#[test]
fn test_set_pool_id() {
    let mut deps = mock_dependencies();

    // Only factory can set pool id
    let msg = ExecuteMsg::SetPoolId { pool_id: 1 };
    let err = execute(
        deps.as_mut(),
        mock_env(),
        mock_info("random", &[]),
        msg.clone(),
    )
    .unwrap_err();
    assert_eq!(err, ContractError::Unauthorized {});

    execute(
        deps.as_mut(),
        mock_env(),
        mock_info(FACTORY_ADDRESS, &[]),
        msg.clone(),
    )
    .unwrap();
    // Pool id is set

    let err = execute(
        deps.as_mut(),
        mock_env(),
        mock_info(FACTORY_ADDRESS, &[]),
        msg,
    )
    .unwrap_err();
    assert_eq!(err, ContractError::PoolIdAlreadySet {});
}
