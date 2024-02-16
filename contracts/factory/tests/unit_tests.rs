use astroport::asset::{native_asset_info, token_asset_info, PairInfo};
use astroport::factory::{
    ConfigResponse, ExecuteMsg, FeeInfoResponse, InstantiateMsg, PairConfig, PairType,
    PairsResponse, QueryMsg,
};
use astroport::{factory, pair};
use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, MOCK_CONTRACT_ADDR};
use cosmwasm_std::{
    attr, coins, from_json, to_json_binary, Addr, Empty, Reply, ReplyOn, StdError, SubMsg,
    SubMsgResponse, SubMsgResult,
};
use cw_utils::PaymentError::NoFunds;
use osmosis_std::types::osmosis::cosmwasmpool::v1beta1::{
    MsgCreateCosmWasmPool, MsgCreateCosmWasmPoolResponse,
};

use astroport_factory_osmosis::contract::{execute, instantiate, migrate, query, reply};
use astroport_factory_osmosis::error::ContractError;
use astroport_factory_osmosis::error::ContractError::PaymentError;

use crate::querier::{mock_dependencies_with_custom_querier, MockedStargateQuerier};

mod querier;

#[test]
fn test_init() {
    let mut deps = mock_dependencies();
    let info = mock_info("tester", &[]);

    let err = instantiate(
        deps.as_mut(),
        mock_env(),
        info.clone(),
        InstantiateMsg {
            pair_configs: vec![
                PairConfig {
                    code_id: 100,
                    pair_type: PairType::Xyk {},
                    total_fee_bps: 10000,
                    maker_fee_bps: 5000,
                    is_disabled: false,
                    is_generator_disabled: false,
                    permissioned: false,
                },
                // Two pair configs with equal types are not allowed
                PairConfig {
                    code_id: 200,
                    pair_type: PairType::Xyk {},
                    total_fee_bps: 10000,
                    maker_fee_bps: 5000,
                    is_disabled: false,
                    is_generator_disabled: false,
                    permissioned: false,
                },
            ],
            token_code_id: 0,
            fee_address: None,
            generator_address: None,
            owner: "owner".to_string(),
            whitelist_code_id: 0,
            coin_registry_address: "coin_registry".to_string(),
        },
    )
    .unwrap_err();
    assert_eq!(err, ContractError::PairConfigDuplicate {});

    let err = instantiate(
        deps.as_mut(),
        mock_env(),
        info.clone(),
        InstantiateMsg {
            pair_configs: vec![PairConfig {
                code_id: 100,
                pair_type: PairType::Xyk {},
                total_fee_bps: 12000, // <--- invalid fee setting
                maker_fee_bps: 5000,
                is_disabled: false,
                is_generator_disabled: false,
                permissioned: false,
            }],
            token_code_id: 0,
            fee_address: None,
            generator_address: None,
            owner: "owner".to_string(),
            whitelist_code_id: 0,
            coin_registry_address: "coin_registry".to_string(),
        },
    )
    .unwrap_err();
    assert_eq!(err, ContractError::PairConfigInvalidFeeBps {});

    let msg = InstantiateMsg {
        pair_configs: vec![PairConfig {
            code_id: 100,
            pair_type: PairType::Xyk {},
            total_fee_bps: 10000,
            maker_fee_bps: 5000,
            is_disabled: false,
            is_generator_disabled: false,
            permissioned: false,
        }],
        token_code_id: 0,
        fee_address: None,
        generator_address: None,
        owner: "owner".to_string(),
        whitelist_code_id: 0,
        coin_registry_address: "coin_registry".to_string(),
    };
    instantiate(deps.as_mut(), mock_env(), info, msg.clone()).unwrap();

    let query_res = query(deps.as_ref(), mock_env(), factory::QueryMsg::Config {}).unwrap();
    let config_res: ConfigResponse = from_json(query_res).unwrap();
    assert_eq!(0, config_res.token_code_id);
    assert_eq!(msg.pair_configs, config_res.pair_configs);
    assert_eq!("owner", config_res.owner.as_str());
}

#[test]
fn update_config() {
    let mut deps = mock_dependencies();
    let owner = "owner0000";

    let pair_configs = vec![PairConfig {
        code_id: 123u64,
        pair_type: PairType::Xyk {},
        total_fee_bps: 3,
        maker_fee_bps: 166,
        is_disabled: false,
        is_generator_disabled: false,
        permissioned: false,
    }];

    let msg = InstantiateMsg {
        pair_configs,
        token_code_id: 123u64,
        fee_address: None,
        owner: owner.to_string(),
        generator_address: Some("generator".to_owned()),
        whitelist_code_id: 234u64,
        coin_registry_address: "coin_registry".to_string(),
    };

    let env = mock_env();
    let info = mock_info(owner, &[]);

    instantiate(deps.as_mut(), env, info, msg).unwrap();

    // Update config
    let env = mock_env();
    let info = mock_info(owner, &[]);
    let msg = ExecuteMsg::UpdateConfig {
        token_code_id: None,
        fee_address: Some(String::from("new_fee_addr")),
        generator_address: Some(String::from("new_generator_addr")),
        whitelist_code_id: None,
        coin_registry_address: Some("registry".to_owned()),
    };

    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert_eq!(0, res.messages.len());

    let query_res = query(deps.as_ref(), env, QueryMsg::Config {}).unwrap();
    let config_res: ConfigResponse = from_json(query_res).unwrap();
    assert_eq!(owner, config_res.owner);
    assert_eq!(
        String::from("new_fee_addr"),
        config_res.fee_address.unwrap()
    );
    assert_eq!(
        String::from("new_generator_addr"),
        config_res.generator_address.unwrap()
    );

    // Unauthorized err
    let env = mock_env();
    let info = mock_info("addr0000", &[]);
    let msg = ExecuteMsg::UpdateConfig {
        token_code_id: None,
        fee_address: None,
        generator_address: None,
        whitelist_code_id: None,
        coin_registry_address: None,
    };

    let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(err, ContractError::Unauthorized {});
}

#[test]
fn update_owner() {
    let mut deps = mock_dependencies();
    let owner = "owner0000";

    let msg = InstantiateMsg {
        pair_configs: vec![],
        token_code_id: 123u64,
        fee_address: None,
        owner: owner.to_string(),
        generator_address: Some(String::from("generator")),
        whitelist_code_id: 234u64,
        coin_registry_address: "coin_registry".to_string(),
    };

    let env = mock_env();
    let info = mock_info(owner, &[]);

    // We can just call .unwrap() to assert this was a success
    instantiate(deps.as_mut(), env, info, msg).unwrap();

    let new_owner = String::from("new_owner");

    // New owner
    let env = mock_env();
    let msg = ExecuteMsg::ProposeNewOwner {
        owner: new_owner.clone(),
        expires_in: 100, // seconds
    };

    let info = mock_info(new_owner.as_str(), &[]);

    // Unauthorized check
    let err = execute(deps.as_mut(), env.clone(), info, msg.clone()).unwrap_err();
    assert_eq!(err.to_string(), "Generic error: Unauthorized");

    // Claim before proposal
    let info = mock_info(new_owner.as_str(), &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimOwnership {},
    )
    .unwrap_err();

    // Propose new owner
    let info = mock_info(owner, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert_eq!(0, res.messages.len());

    // Unauthorized ownership claim
    let info = mock_info("invalid_addr", &[]);
    let err = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimOwnership {},
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "Generic error: Unauthorized");

    // Claim ownership
    let info = mock_info(new_owner.as_str(), &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::ClaimOwnership {},
    )
    .unwrap();
    assert_eq!(0, res.messages.len());

    // Let's query the state
    let config: ConfigResponse =
        from_json(query(deps.as_ref(), env, QueryMsg::Config {}).unwrap()).unwrap();
    assert_eq!(new_owner, config.owner);
}

#[test]
fn update_pair_config() {
    let mut deps = mock_dependencies_with_custom_querier(MockedStargateQuerier::new());
    let owner = "owner0000";
    let pair_configs = vec![PairConfig {
        code_id: 123u64,
        pair_type: PairType::Xyk {},
        total_fee_bps: 100,
        maker_fee_bps: 10,
        is_disabled: false,
        is_generator_disabled: false,
        permissioned: false,
    }];

    let msg = InstantiateMsg {
        pair_configs: pair_configs.clone(),
        token_code_id: 123u64,
        fee_address: None,
        owner: owner.to_string(),
        generator_address: Some(String::from("generator")),
        whitelist_code_id: 234u64,
        coin_registry_address: "coin_registry".to_string(),
    };

    let env = mock_env();
    let info = mock_info("addr0000", &[]);

    // We can just call .unwrap() to assert this was a success
    instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

    // It worked, let's query the state
    let query_res = query(deps.as_ref(), env, QueryMsg::Config {}).unwrap();
    let config_res: ConfigResponse = from_json(query_res).unwrap();
    assert_eq!(pair_configs, config_res.pair_configs);

    // Update config
    let pair_config = PairConfig {
        code_id: 800,
        pair_type: PairType::Xyk {},
        total_fee_bps: 1,
        maker_fee_bps: 2,
        is_disabled: false,
        is_generator_disabled: false,
        permissioned: false,
    };

    // Unauthorized err
    let env = mock_env();
    let info = mock_info("wrong-addr0000", &[]);
    let msg = ExecuteMsg::UpdatePairConfig {
        config: pair_config.clone(),
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap_err();
    assert_eq!(res, ContractError::Unauthorized {});

    // Check validation of total and maker fee bps
    let env = mock_env();
    let info = mock_info(owner, &[]);
    let msg = ExecuteMsg::UpdatePairConfig {
        config: PairConfig {
            code_id: 123u64,
            pair_type: PairType::Xyk {},
            total_fee_bps: 3,
            maker_fee_bps: 10_001,
            is_disabled: false,
            is_generator_disabled: false,
            permissioned: false,
        },
    };

    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap_err();
    assert_eq!(res, ContractError::PairConfigInvalidFeeBps {});

    let info = mock_info(owner, &[]);
    let msg = ExecuteMsg::UpdatePairConfig {
        config: pair_config.clone(),
    };

    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert_eq!(0, res.messages.len());

    // It worked, let's query the state
    let query_res = query(deps.as_ref(), env.clone(), QueryMsg::Config {}).unwrap();
    let config_res: ConfigResponse = from_json(query_res).unwrap();
    assert_eq!(vec![pair_config.clone()], config_res.pair_configs);

    // Add second config
    let pair_config_custom = PairConfig {
        code_id: 100,
        pair_type: PairType::Custom("test".to_string()),
        total_fee_bps: 10,
        maker_fee_bps: 20,
        is_disabled: false,
        is_generator_disabled: false,
        permissioned: false,
    };

    let info = mock_info(owner, &[]);
    let msg = ExecuteMsg::UpdatePairConfig {
        config: pair_config_custom.clone(),
    };

    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    let query_res = query(deps.as_ref(), env.clone(), QueryMsg::Config {}).unwrap();
    let config_res: ConfigResponse = from_json(query_res).unwrap();
    assert_eq!(
        vec![pair_config_custom, pair_config],
        config_res.pair_configs
    );

    // disable pair and try to create a new pair
    execute(
        deps.as_mut(),
        env.clone(),
        mock_info(owner, &[]),
        ExecuteMsg::UpdatePairConfig {
            config: PairConfig {
                code_id: 0,
                pair_type: PairType::Custom("test".to_string()),
                total_fee_bps: 500,
                maker_fee_bps: 5000,
                is_disabled: true,
                is_generator_disabled: false,
                permissioned: false,
            },
        },
    )
    .unwrap();

    let asset_infos = vec![
        native_asset_info("asset0000".to_string()),
        native_asset_info("asset0001".to_string()),
    ];

    let err = execute(
        deps.as_mut(),
        env,
        mock_info("user", &coins(1000_000000, "uosmo")),
        ExecuteMsg::CreatePair {
            pair_type: PairType::Custom("test".to_string()),
            asset_infos,
            init_params: None,
        },
    )
    .unwrap_err();

    assert_eq!(err, ContractError::PairConfigDisabled {});
}

#[test]
fn create_pair() {
    let mut deps = mock_dependencies_with_custom_querier(MockedStargateQuerier::new());

    let pair_config = PairConfig {
        code_id: 321,
        pair_type: PairType::Xyk {},
        total_fee_bps: 100,
        maker_fee_bps: 10,
        is_disabled: false,
        is_generator_disabled: false,
        permissioned: false,
    };

    let factory_init_msg = InstantiateMsg {
        pair_configs: vec![pair_config.clone()],
        token_code_id: 123,
        fee_address: None,
        owner: "owner0000".to_string(),
        generator_address: Some(String::from("generator")),
        whitelist_code_id: 234,
        coin_registry_address: "coin_registry".to_string(),
    };

    let env = mock_env();
    let info = mock_info("addr0000", &[]);

    instantiate(deps.as_mut(), env, info, factory_init_msg).unwrap();

    let asset_infos = vec![
        native_asset_info("asset0000".to_string()),
        native_asset_info("asset0001".to_string()),
    ];

    let env = mock_env();
    let info = mock_info("addr0000", &[]);

    // must send 100 OSMO
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CreatePair {
            pair_type: PairType::Xyk {},
            asset_infos: asset_infos.clone(),
            init_params: None,
        },
    )
    .unwrap_err();
    assert_eq!(res, PaymentError(NoFunds {}));

    let res = execute(
        deps.as_mut(),
        env.clone(),
        mock_info("addr0000", &coins(50_000000, "uosmo")),
        ExecuteMsg::CreatePair {
            pair_type: PairType::Stable {},
            asset_infos: asset_infos.clone(),
            init_params: None,
        },
    )
    .unwrap_err();
    assert_eq!(
        res,
        ContractError::Std(StdError::generic_err(
            "Not enough funds to create a pool. Check pool_creation_fee in poolmanager params."
        ))
    );

    let res = execute(
        deps.as_mut(),
        env.clone(),
        mock_info("addr0000", &coins(1000_000000, "uosmo")),
        ExecuteMsg::CreatePair {
            pair_type: PairType::Stable {},
            asset_infos: asset_infos.clone(),
            init_params: None,
        },
    )
    .unwrap_err();
    assert_eq!(res, ContractError::PairConfigNotFound {});

    let res = execute(
        deps.as_mut(),
        env,
        mock_info("addr0000", &coins(1000_000000, "uosmo")),
        ExecuteMsg::CreatePair {
            pair_type: PairType::Xyk {},
            asset_infos: asset_infos.clone(),
            init_params: None,
        },
    )
    .unwrap();

    assert_eq!(
        res.attributes,
        vec![
            attr("action", "create_pair"),
            attr("pair", "asset0000-asset0001")
        ]
    );
    assert_eq!(
        res.messages,
        vec![SubMsg {
            msg: MsgCreateCosmWasmPool {
                code_id: pair_config.code_id,
                instantiate_msg: to_json_binary(&pair::InstantiateMsg {
                    asset_infos,
                    token_code_id: 0,
                    factory_addr: MOCK_CONTRACT_ADDR.to_string(),
                    init_params: None,
                })
                .unwrap()
                .to_vec(),
                sender: MOCK_CONTRACT_ADDR.to_string(),
            }
            .into(),
            id: 1,
            gas_limit: None,
            reply_on: ReplyOn::Success
        }]
    );
}

#[test]
fn register() {
    let mut deps = mock_dependencies_with_custom_querier(MockedStargateQuerier::new());
    let owner = "owner0000";

    let msg = InstantiateMsg {
        pair_configs: vec![PairConfig {
            code_id: 123u64,
            pair_type: PairType::Xyk {},
            total_fee_bps: 100,
            maker_fee_bps: 10,
            is_disabled: false,
            is_generator_disabled: false,
            permissioned: false,
        }],
        token_code_id: 123u64,
        fee_address: Some("maker".to_owned()),
        generator_address: Some("generator".to_owned()),
        owner: owner.to_string(),
        whitelist_code_id: 234u64,
        coin_registry_address: "coin_registry".to_string(),
    };

    let env = mock_env();
    let info = mock_info("addr0000", &[]);
    instantiate(deps.as_mut(), env, info, msg).unwrap();

    // Try to create a pair with equal assets
    let msg = ExecuteMsg::CreatePair {
        pair_type: PairType::Xyk {},
        asset_infos: vec![
            native_asset_info("asset".to_string()),
            native_asset_info("asset".to_string()),
        ],
        init_params: None,
    };
    let err = execute(
        deps.as_mut(),
        mock_env(),
        mock_info("addr0000", &coins(1000_000000, "uosmo")),
        msg,
    )
    .unwrap_err();
    assert_eq!(err, ContractError::DoublingAssets {});

    // Try to create a pair with cw20 asset
    let msg = ExecuteMsg::CreatePair {
        pair_type: PairType::Xyk {},
        asset_infos: vec![
            native_asset_info("asset".to_string()),
            token_asset_info(Addr::unchecked("cw20token")),
        ],
        init_params: None,
    };
    let err = execute(
        deps.as_mut(),
        mock_env(),
        mock_info("addr0000", &coins(1000_000000, "uosmo")),
        msg,
    )
    .unwrap_err();
    assert_eq!(err, ContractError::NonNativeToken {});

    let asset_infos = vec![
        native_asset_info("asset0000".to_string()),
        native_asset_info("asset0001".to_string()),
    ];

    let msg = ExecuteMsg::CreatePair {
        pair_type: PairType::Xyk {},
        asset_infos: asset_infos.clone(),
        init_params: None,
    };

    let env = mock_env();
    let info = mock_info("addr0000", &coins(1000_000000, "uosmo"));
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    let pair_info = PairInfo {
        asset_infos: asset_infos.clone(),
        contract_addr: Addr::unchecked("pair0000"),
        liquidity_token: Addr::unchecked("liquidity0000"),
        pair_type: PairType::Xyk {},
    };
    deps.querier.add_contract("pair0000", pair_info, 1);

    let reply_msg = Reply {
        id: 1,
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![],
            data: Some(MsgCreateCosmWasmPoolResponse { pool_id: 1 }.into()),
        }),
    };

    reply(deps.as_mut(), mock_env(), reply_msg.clone()).unwrap();

    let query_res = query(
        deps.as_ref(),
        env,
        QueryMsg::Pair {
            asset_infos: asset_infos.clone(),
        },
    )
    .unwrap();

    let pair_res: PairInfo = from_json(query_res).unwrap();
    assert_eq!(
        pair_res,
        PairInfo {
            liquidity_token: Addr::unchecked("liquidity0000"),
            contract_addr: Addr::unchecked("pair0000"),
            asset_infos: asset_infos.clone(),
            pair_type: PairType::Xyk {},
        }
    );

    // Check pair was registered
    let res = reply(deps.as_mut(), mock_env(), reply_msg).unwrap_err();
    assert_eq!(res, ContractError::PairWasRegistered {});

    // Store one more item to test query pairs
    let asset_infos_2 = vec![
        native_asset_info("asset0000".to_string()),
        native_asset_info("asset0002".to_string()),
    ];

    let msg = ExecuteMsg::CreatePair {
        pair_type: PairType::Xyk {},
        asset_infos: asset_infos_2.clone(),
        init_params: None,
    };

    let env = mock_env();
    let info = mock_info("addr0000", &coins(1000_000000, "uosmo"));
    execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

    let pair_info = PairInfo {
        asset_infos: asset_infos_2.clone(),
        contract_addr: Addr::unchecked("pair0001"),
        liquidity_token: Addr::unchecked("liquidity0001"),
        pair_type: PairType::Xyk {},
    };
    deps.querier.add_contract("pair0001", pair_info, 2);

    let reply_msg = Reply {
        id: 1,
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![],
            data: Some(MsgCreateCosmWasmPoolResponse { pool_id: 2 }.into()),
        }),
    };

    reply(deps.as_mut(), mock_env(), reply_msg).unwrap();

    // Try to create one more pair with the same assets
    let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert_eq!(err, ContractError::PairWasCreated {});

    let query_msg = QueryMsg::Pairs {
        start_after: None,
        limit: None,
    };

    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let pairs_res: PairsResponse = from_json(res).unwrap();
    assert_eq!(
        pairs_res.pairs,
        vec![
            PairInfo {
                liquidity_token: Addr::unchecked("liquidity0000"),
                contract_addr: Addr::unchecked("pair0000"),
                asset_infos: asset_infos.clone(),
                pair_type: PairType::Xyk {},
            },
            PairInfo {
                liquidity_token: Addr::unchecked("liquidity0001"),
                contract_addr: Addr::unchecked("pair0001"),
                asset_infos: asset_infos_2.clone(),
                pair_type: PairType::Xyk {},
            }
        ]
    );

    let query_msg = QueryMsg::Pairs {
        start_after: None,
        limit: Some(1),
    };

    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let pairs_res: PairsResponse = from_json(res).unwrap();
    assert_eq!(
        pairs_res.pairs,
        vec![PairInfo {
            liquidity_token: Addr::unchecked("liquidity0000"),
            contract_addr: Addr::unchecked("pair0000"),
            asset_infos: asset_infos.clone(),
            pair_type: PairType::Xyk {},
        }]
    );

    let query_msg = QueryMsg::Pairs {
        start_after: Some(asset_infos.clone()),
        limit: None,
    };

    let res = query(deps.as_ref(), env, query_msg).unwrap();
    let pairs_res: PairsResponse = from_json(res).unwrap();
    assert_eq!(
        pairs_res.pairs,
        vec![PairInfo {
            liquidity_token: Addr::unchecked("liquidity0001"),
            contract_addr: Addr::unchecked("pair0001"),
            asset_infos: asset_infos_2.clone(),
            pair_type: PairType::Xyk {},
        }]
    );

    // Deregister from wrong acc
    let env = mock_env();
    let info = mock_info("wrong_addr0000", &coins(1000_000000, "uosmo"));
    let res = execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::Deregister {
            asset_infos: asset_infos_2.clone(),
        },
    )
    .unwrap_err();

    assert_eq!(res, ContractError::Unauthorized {});

    // Proper deregister
    let env = mock_env();
    let info = mock_info(owner, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::Deregister {
            asset_infos: asset_infos_2,
        },
    )
    .unwrap();

    assert_eq!(res.attributes[0], attr("action", "deregister"));

    let query_msg = QueryMsg::Pairs {
        start_after: None,
        limit: None,
    };

    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let pairs_res: PairsResponse = from_json(res).unwrap();
    assert_eq!(
        pairs_res.pairs,
        vec![PairInfo {
            liquidity_token: Addr::unchecked("liquidity0000"),
            contract_addr: Addr::unchecked("pair0000"),
            asset_infos,
            pair_type: PairType::Xyk {},
        },]
    );

    let res = query(
        deps.as_ref(),
        env.clone(),
        QueryMsg::FeeInfo {
            pair_type: PairType::Xyk {},
        },
    )
    .unwrap();
    assert_eq!(
        from_json::<FeeInfoResponse>(&res).unwrap(),
        FeeInfoResponse {
            fee_address: Some(Addr::unchecked("maker")),
            total_fee_bps: 100,
            maker_fee_bps: 10,
        }
    );

    let res = query(deps.as_ref(), env, QueryMsg::BlacklistedPairTypes {}).unwrap();
    assert_eq!(from_json::<[(); 0]>(&res).unwrap(), []);
}

const SET_POOL_ID_FAILED_REPLY_ID: u64 = 2;

#[test]
fn test_failed_replies() {
    let mut deps = mock_dependencies();
    let res = reply(
        deps.as_mut(),
        mock_env(),
        Reply {
            id: SET_POOL_ID_FAILED_REPLY_ID, // this reply is only processed on set_pool_id callback failure
            result: SubMsgResult::Ok(SubMsgResponse {
                events: vec![],
                data: None,
            }),
        },
    )
    .unwrap();
    assert_eq!(
        res.attributes,
        [
            attr("action", "set_pool_id_reply"),
            attr("state", "failed"),
            attr("solution", "pass"),
        ]
    );

    let err = reply(
        deps.as_mut(),
        mock_env(),
        Reply {
            id: 1000, // unknown reply id
            result: SubMsgResult::Ok(SubMsgResponse {
                events: vec![],
                data: None,
            }),
        },
    )
    .unwrap_err();
    assert_eq!(err, ContractError::FailedToParseReply {});

    let err = migrate(deps.as_mut(), mock_env(), Empty {}).unwrap_err();
    assert_eq!(
        err,
        StdError::generic_err("Migration is not implemented yet")
    );
}
