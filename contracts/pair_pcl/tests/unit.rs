use astroport::asset::{native_asset_info, token_asset_info, PairInfo};
use astroport::factory::PairType;
use astroport::pair::InstantiateMsg;
use astroport::pair_concentrated::ConcentratedPoolParams;
use astroport_pcl_common::state::{Config, PoolState};
use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{to_binary, Addr, Reply, StdError, SubMsgResponse, SubMsgResult};
use osmosis_std::types::osmosis::tokenfactory::v1beta1::MsgCreateDenomResponse;

use astroport_on_osmosis::pair_pcl::ExecuteMsg;
use astroport_pcl_osmo::contract::{execute, instantiate, reply};
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
        fee_share: None,
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

#[test]
fn try_init_with_cw20() {
    let init_msg = InstantiateMsg {
        asset_infos: vec![
            token_asset_info(Addr::unchecked("ASTRO")),
            native_asset_info("uosmo".to_string()),
        ],
        token_code_id: 0,
        factory_addr: FACTORY_ADDRESS.to_string(),
        init_params: Some(
            to_binary(&ConcentratedPoolParams {
                amp: Default::default(),
                gamma: Default::default(),
                mid_fee: Default::default(),
                out_fee: Default::default(),
                fee_gamma: Default::default(),
                repeg_profit_threshold: Default::default(),
                min_price_scale_delta: Default::default(),
                price_scale: Default::default(),
                ma_half_time: 0,
                track_asset_balances: None,
                fee_share: None,
            })
            .unwrap(),
        ),
    };

    let mut deps = mock_dependencies();
    let err = instantiate(
        deps.as_mut(),
        mock_env(),
        mock_info("sender", &[]),
        init_msg,
    )
    .unwrap_err();

    assert_eq!(
        err,
        ContractError::Std(StdError::generic_err("CW20 tokens are not supported"))
    );
}
