use astroport::asset::{native_asset_info, AssetInfo, AssetInfoExt};
use astroport::pair;
use cosmwasm_std::{coin, to_json_binary, Coin, Decimal};
use osmosis_std::types::osmosis::cosmwasmpool::v1beta1::{
    ContractInfoByPoolIdRequest, ContractInfoByPoolIdResponse, MsgCreateCosmWasmPool,
    MsgCreateCosmWasmPoolResponse,
};
use osmosis_test_tube::{Account, OsmosisTestApp, Runner};

use astroport_osmo_e2e_tests::helper::{default_pcl_params, TestAppWrapper};

fn gas_fee() -> Coin {
    coin(2_000_000_000000, "uosmo")
}

#[test]
fn dex_swap_test() {
    let app = OsmosisTestApp::new();
    let helper = TestAppWrapper::bootstrap(&app).unwrap();

    let foo_denom = helper.register_and_mint("foo", 1_000_000_000000, 6, None);
    let bar_denom = helper.register_and_mint("bar", 1_000_000_000000, 6, None);
    let foo = native_asset_info(foo_denom.clone());
    let bar = native_asset_info(bar_denom.clone());

    let (pair_addr, _) = helper
        .create_pair(
            &[foo.clone(), bar.clone()],
            default_pcl_params(Decimal::from_ratio(1u8, 2u8)),
        )
        .unwrap();
    let pool_id = helper.get_pool_id_by_contract(&pair_addr);

    helper
        .provide(
            &helper.signer,
            &pair_addr,
            &[
                foo.with_balance(50_000_000000u128),
                bar.with_balance(100_000_000000u128),
            ],
            None,
        )
        .unwrap();

    // Swap via DEX: FOO -> BAR
    let asset = foo.with_balance(1_000000u128);
    let user = helper
        .app
        .init_account(&[asset.as_coin().unwrap(), gas_fee()])
        .unwrap();

    helper.swap_on_dex(&user, pool_id, &asset).unwrap();

    let foo_bal = helper.coin_balance(&user.address(), &foo_denom);
    let bar_bal = helper.coin_balance(&user.address(), &bar_denom);
    assert_eq!(foo_bal, 0);
    assert_eq!(bar_bal, 1_994798);

    // Swap via DEX: BAR -> FOO
    let asset = bar.with_balance(bar_bal);
    helper.swap_on_dex(&user, pool_id, &asset).unwrap();

    let foo_bal = helper.coin_balance(&user.address(), &foo_denom);
    let bar_bal = helper.coin_balance(&user.address(), &bar_denom);
    assert_eq!(bar_bal, 0);
    assert_eq!(foo_bal, 994806);

    // Direct swap via pair contract FOO -> BAR (which essentially proxies it to DEX module)
    let asset = foo.with_balance(foo_bal);
    helper
        .swap_on_pair(&user, &pair_addr, &asset, None)
        .unwrap();

    let foo_bal = helper.coin_balance(&user.address(), &foo_denom);
    let bar_bal = helper.coin_balance(&user.address(), &bar_denom);
    assert_eq!(foo_bal, 0);
    assert_eq!(bar_bal, 1984437);

    // Reverse swap via DEX: FOO -> BAR
    let asset = foo.with_balance(1_000000u128);
    let user2 = helper
        .app
        .init_account(&[asset.as_coin().unwrap(), gas_fee()])
        .unwrap();
    let ask_asset = bar.with_balance(1_900000u128);
    helper
        .reverse_swap_on_dex(&user2, pool_id, &foo_denom, asset.amount.u128(), &ask_asset)
        .unwrap();
    let foo_bal = helper.coin_balance(&user2.address(), &foo_denom);
    let bar_bal = helper.coin_balance(&user2.address(), &bar_denom);
    assert_eq!(foo_bal, 45703); // excess tokens sent back to the user2
    assert_eq!(bar_bal, 1_903626); // PCL pool gives slightly more tokens than expected (due to dynamic fees)
}

#[test]
fn init_outside_of_factory() {
    let app = OsmosisTestApp::new();
    let helper = TestAppWrapper::bootstrap(&app).unwrap();

    let foo_denom = helper.register_and_mint("foo", 1_000_000_000000, 6, None);
    let bar_denom = helper.register_and_mint("bar", 1_000_000_000000, 6, None);
    let foo = native_asset_info(foo_denom.clone());
    let bar = native_asset_info(bar_denom.clone());

    let pool_id = app
        .execute::<_, MsgCreateCosmWasmPoolResponse>(
            MsgCreateCosmWasmPool {
                code_id: helper.code_ids["pair-concentrated"],
                instantiate_msg: to_json_binary(&pair::InstantiateMsg {
                    asset_infos: vec![foo.clone(), bar.clone()],
                    init_params: Some(
                        to_json_binary(&default_pcl_params(Decimal::from_ratio(1u8, 2u8))).unwrap(),
                    ),
                    factory_addr: "".to_string(),
                    token_code_id: 0,
                })
                .unwrap()
                .to_vec(),
                sender: helper.signer.address(),
            },
            MsgCreateCosmWasmPool::TYPE_URL,
            &helper.signer,
        )
        .unwrap()
        .data
        .pool_id;
    let resp = app
        .query::<_, ContractInfoByPoolIdResponse>(
            "/osmosis.cosmwasmpool.v1beta1.Query/ContractInfoByPoolId",
            &ContractInfoByPoolIdRequest { pool_id },
        )
        .unwrap();
    let pair_addr = resp.contract_address.as_str();

    let err = helper
        .provide(
            &helper.signer,
            &pair_addr,
            &[
                foo.with_balance(50_000_000000u128),
                bar.with_balance(100_000_000000u128),
            ],
            None,
        )
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "execute error: failed to execute message; message index: 0: Pair is not registered in the factory. Only swap and withdraw are allowed: execute wasm contract failed"
    );
}

#[test]
fn swap_with_fake_token() {
    let app = OsmosisTestApp::new();
    let helper = TestAppWrapper::bootstrap(&app).unwrap();

    let foo_denom = helper.register_and_mint("foo", 1_000_000_000000, 6, None);
    let bar_denom = helper.register_and_mint("bar", 1_000_000_000000, 6, None);
    let foo = native_asset_info(foo_denom.clone());
    let bar = native_asset_info(bar_denom.clone());

    let (pair_addr, _) = helper
        .create_pair(
            &[foo.clone(), bar.clone()],
            default_pcl_params(Decimal::from_ratio(1u8, 2u8)),
        )
        .unwrap();
    let pool_id = helper.get_pool_id_by_contract(&pair_addr);

    helper
        .provide(
            &helper.signer,
            &pair_addr,
            &[
                foo.with_balance(50_000_000000u128),
                bar.with_balance(100_000_000000u128),
            ],
            None,
        )
        .unwrap();

    // Try to swap fake tokens via DEX module
    let asset = AssetInfo::native("random").with_balance(1_000000u128);
    let user = helper
        .app
        .init_account(&[asset.as_coin().unwrap(), gas_fee()])
        .unwrap();

    let err = helper.swap_on_dex(&user, pool_id, &asset).unwrap_err();
    assert_eq!(
        err.to_string(),
        "execute error: failed to execute message; message index: 0: The asset random does not belong to the pair: execute wasm contract failed"
    );

    // Try reverse swap fake tokens via DEX module
    let ask_asset = bar.with_balance(1_000000u128);
    let err = helper
        .reverse_swap_on_dex(&user, pool_id, "random", 1_000000u128, &ask_asset)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "execute error: failed to execute message; message index: 0: The asset random does not belong to the pair: execute wasm contract failed"
    );
}
