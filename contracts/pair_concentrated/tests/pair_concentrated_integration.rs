#![cfg(not(tarpaulin_include))]

use std::str::FromStr;

use astroport::asset::{
    native_asset_info, Asset, AssetInfo, AssetInfoExt, MINIMUM_LIQUIDITY_AMOUNT,
};
use astroport::cosmwasm_ext::AbsDiff;
use astroport::observation::OracleObservation;
use astroport::pair::{ExecuteMsg, PoolResponse};
use astroport::pair_concentrated::{
    ConcentratedPoolParams, ConcentratedPoolUpdateParams, PromoteParams, UpdatePoolParams,
};
use astroport_pcl_common::consts::{AMP_MAX, AMP_MIN, MA_HALF_TIME_LIMITS};
use astroport_pcl_common::error::PclError;
use cosmwasm_std::{Addr, Decimal, Fraction, StdError, Uint128};
use cw_multi_test::{next_block, Executor};
use itertools::Itertools;

use astroport_on_osmosis::pair_pcl::{
    CalcInAmtGivenOutResponse, CalcOutAmtGivenInResponse, QueryMsg, SpotPriceResponse,
    TotalPoolLiquidityResponse,
};
use astroport_pcl_osmo::error::ContractError;
use common::helper::{dec_to_f64, f64_to_dec, AppExtension, Helper, TestCoin};

mod common;

fn common_pcl_params() -> ConcentratedPoolParams {
    ConcentratedPoolParams {
        amp: f64_to_dec(40f64),
        gamma: f64_to_dec(0.000145),
        mid_fee: f64_to_dec(0.0026),
        out_fee: f64_to_dec(0.0045),
        fee_gamma: f64_to_dec(0.00023),
        repeg_profit_threshold: f64_to_dec(0.000002),
        min_price_scale_delta: f64_to_dec(0.000146),
        price_scale: Decimal::one(),
        ma_half_time: 600,
        track_asset_balances: None,
        fee_share: None,
    }
}

#[test]
fn check_observe_queries() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    let mut helper = Helper::new(&owner, test_coins.clone(), common_pcl_params()).unwrap();

    let user = Addr::unchecked("user");
    let offer_asset = helper.assets[&test_coins[0]].with_balance(100_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user);

    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
        helper.assets[&test_coins[1]].with_balance(100_000_000000u128),
    ];
    helper.provide_liquidity(&owner, &assets).unwrap();

    let d = helper.query_d().unwrap();
    assert_eq!(dec_to_f64(d), 200000f64);

    assert_eq!(0, helper.coin_balance(&test_coins[1], &user));
    helper.swap(&user, &offer_asset, None).unwrap();
    assert_eq!(0, helper.coin_balance(&test_coins[0], &user));
    assert_eq!(99_737929, helper.coin_balance(&test_coins[1], &user));

    helper.app.next_block(1000);

    let user2 = Addr::unchecked("user2");
    let offer_asset = helper.assets[&test_coins[1]].with_balance(100_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user2);
    helper.swap(&user2, &offer_asset, None).unwrap();
    assert_eq!(0, helper.coin_balance(&test_coins[1], &user2));
    assert_eq!(99_741246, helper.coin_balance(&test_coins[0], &user2));

    let d = helper.query_d().unwrap();
    assert_eq!(dec_to_f64(d), 200000.260415);

    let res: OracleObservation = helper
        .app
        .wrap()
        .query_wasm_smart(
            helper.pair_addr.to_string(),
            &QueryMsg::Observe { seconds_ago: 0 },
        )
        .unwrap();

    assert_eq!(
        res,
        OracleObservation {
            timestamp: helper.app.block_info().time.seconds(),
            price: Decimal::from_str("1.002627596167552265").unwrap()
        }
    );
}

#[test]
fn check_wrong_initialization() {
    let owner = Addr::unchecked("owner");

    let params = common_pcl_params();

    let err = Helper::new(&owner, vec![TestCoin::native("uosmo")], params.clone()).unwrap_err();

    assert_eq!(
        err.root_cause().to_string(),
        "Generic error: asset_infos must contain two elements",
    );

    let mut wrong_params = params.clone();
    wrong_params.amp = Decimal::zero();

    let err = Helper::new(
        &owner,
        vec![TestCoin::native("uosmo"), TestCoin::native("uusd")],
        wrong_params,
    )
    .unwrap_err();

    assert_eq!(
        ContractError::PclError(PclError::IncorrectPoolParam(
            "amp".to_string(),
            AMP_MIN.to_string(),
            AMP_MAX.to_string()
        )),
        err.downcast().unwrap(),
    );

    let mut wrong_params = params.clone();
    wrong_params.ma_half_time = MA_HALF_TIME_LIMITS.end() + 1;

    let err = Helper::new(
        &owner,
        vec![TestCoin::native("uosmo"), TestCoin::native("uusd")],
        wrong_params,
    )
    .unwrap_err();

    assert_eq!(
        ContractError::PclError(PclError::IncorrectPoolParam(
            "ma_half_time".to_string(),
            MA_HALF_TIME_LIMITS.start().to_string(),
            MA_HALF_TIME_LIMITS.end().to_string()
        )),
        err.downcast().unwrap(),
    );

    let mut wrong_params = params.clone();
    wrong_params.price_scale = Decimal::zero();

    let err = Helper::new(
        &owner,
        vec![TestCoin::native("uosmo"), TestCoin::native("uusd")],
        wrong_params,
    )
    .unwrap_err();

    assert_eq!(
        err.root_cause().to_string(),
        "Generic error: Initial price scale can not be zero",
    );

    // check instantiation with valid params
    Helper::new(
        &owner,
        vec![TestCoin::native("uosmo"), TestCoin::native("uusd")],
        params,
    )
    .unwrap();
}

#[test]
fn check_create_pair_with_cw20() {
    let owner = Addr::unchecked("owner");

    let wrong_coins = vec![TestCoin::cw20("ASTRO"), TestCoin::native("uosmo")];

    let params = common_pcl_params();

    let err = Helper::new(&owner, wrong_coins, params).unwrap_err();
    assert_eq!(
        err.downcast::<astroport_factory::error::ContractError>()
            .unwrap(),
        astroport_factory::error::ContractError::NonNativeToken {}
    );
}

#[test]
fn provide_and_withdraw() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    let mut params = common_pcl_params();
    params.price_scale = Decimal::from_ratio(2u8, 1u8);

    let mut helper = Helper::new(&owner, test_coins.clone(), params).unwrap();

    // checking LP token virtual price on an empty pool
    let lp_price = helper.query_lp_price().unwrap();
    assert!(
        lp_price.is_zero(),
        "LP price must be zero before any provide"
    );

    let user1 = Addr::unchecked("user1");

    let random_coin = native_asset_info("random-coin".to_string()).with_balance(100u8);
    let wrong_assets = vec![
        helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
        random_coin.clone(),
    ];
    helper.give_me_money(&wrong_assets, &user1);
    let err = helper.provide_liquidity(&user1, &wrong_assets).unwrap_err();
    assert_eq!(
        "Generic error: Asset random-coin is not in the pool",
        err.root_cause().to_string()
    );

    // Provide with asset which does not belong to the pair
    let err = helper
        .provide_liquidity(
            &user1,
            &[
                random_coin.clone(),
                helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
            ],
        )
        .unwrap_err();
    assert_eq!(
        "Generic error: Asset random-coin is not in the pool",
        err.root_cause().to_string()
    );

    let err = helper
        .provide_liquidity(&user1, &[random_coin.clone()])
        .unwrap_err();
    assert_eq!(
        "The asset random-coin does not belong to the pair",
        err.root_cause().to_string()
    );

    let err = helper.provide_liquidity(&user1, &[]).unwrap_err();
    assert_eq!(
        "Generic error: Nothing to provide",
        err.root_cause().to_string()
    );

    // Try to provide 3 assets
    helper.give_me_money(&[helper.assets[&test_coins[1]].with_balance(1u8)], &user1);
    let err = helper
        .provide_liquidity(
            &user1,
            &[
                random_coin,
                helper.assets[&test_coins[0]].with_balance(1u8),
                helper.assets[&test_coins[1]].with_balance(1u8),
            ],
        )
        .unwrap_err();
    assert_eq!(
        ContractError::InvalidNumberOfAssets(2),
        err.downcast().unwrap()
    );

    helper.give_me_money(
        &[helper.assets[&test_coins[1]].with_balance(50_000_000000u128)],
        &user1,
    );
    // Try to provide with zero amount
    let err = helper
        .provide_liquidity(
            &user1,
            &[
                helper.assets[&test_coins[0]].with_balance(0u8),
                helper.assets[&test_coins[1]].with_balance(50_000_000000u128),
            ],
        )
        .unwrap_err();
    assert_eq!(ContractError::InvalidZeroAmount {}, err.downcast().unwrap());

    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
        helper.assets[&test_coins[1]].with_balance(50_000_000000u128),
    ];
    helper.give_me_money(
        &[helper.assets[&test_coins[1]].with_balance(50_000_000000u128)],
        &user1,
    );

    // Test very small initial provide
    let err = helper
        .provide_liquidity(
            &user1,
            &[
                helper.assets[&test_coins[0]].with_balance(1000u128),
                helper.assets[&test_coins[1]].with_balance(500u128),
            ],
        )
        .unwrap_err();
    assert_eq!(
        ContractError::MinimumLiquidityAmountError {},
        err.downcast().unwrap()
    );

    // This is normal provision
    let user2 = Addr::unchecked("user2");
    helper.give_me_money(&assets, &user2);
    helper.provide_liquidity(&user2, &assets).unwrap();

    assert_eq!(
        70710_677118,
        helper.native_balance(&helper.lp_token, &user2)
    );
    assert_eq!(0, helper.coin_balance(&test_coins[0], &user2));
    assert_eq!(0, helper.coin_balance(&test_coins[1], &user2));

    assert_eq!(
        helper
            .query_share(helper.native_balance(&helper.lp_token, &user2))
            .unwrap(),
        vec![
            helper.assets[&test_coins[0]].with_balance(99999998584u128),
            helper.assets[&test_coins[1]].with_balance(49999999292u128)
        ]
    );

    let user3 = Addr::unchecked("user3");
    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
        helper.assets[&test_coins[1]].with_balance(50_000_000000u128),
    ];
    helper.give_me_money(&assets, &user3);
    helper.provide_liquidity(&user3, &assets).unwrap();
    assert_eq!(
        70710_677118 + MINIMUM_LIQUIDITY_AMOUNT.u128(),
        helper.native_balance(&helper.lp_token, &user3)
    );

    // Changing order of assets does not matter
    let user4 = Addr::unchecked("user4");
    let assets = vec![
        helper.assets[&test_coins[1]].with_balance(50_000_000000u128),
        helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
    ];
    helper.give_me_money(&assets, &user4);
    helper.provide_liquidity(&user4, &assets).unwrap();
    assert_eq!(
        70710_677118 + MINIMUM_LIQUIDITY_AMOUNT.u128(),
        helper.native_balance(&helper.lp_token, &user4)
    );

    // After initial provide one-sided provide is allowed
    let user5 = Addr::unchecked("user5");
    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(0u128),
        helper.assets[&test_coins[1]].with_balance(100_000_000000u128),
    ];
    helper.give_me_money(&assets, &user5);
    helper.provide_liquidity(&user5, &assets).unwrap();
    // LP amount is less than for prev users as provide is imbalanced
    assert_eq!(
        62217_722016,
        helper.native_balance(&helper.lp_token, &user5)
    );

    // One of assets may be omitted
    let user6 = Addr::unchecked("user6");
    let assets = vec![helper.assets[&test_coins[0]].with_balance(140_000_000000u128)];
    helper.give_me_money(&assets, &user6);
    helper.provide_liquidity(&user6, &assets).unwrap();
    assert_eq!(
        57271_023590,
        helper.native_balance(&helper.lp_token, &user6)
    );

    // check that imbalanced withdraw is currently disabled
    let withdraw_assets = vec![
        helper.assets[&test_coins[0]].with_balance(10_000_000000u128),
        helper.assets[&test_coins[1]].with_balance(5_000_000000u128),
    ];
    let err = helper
        .withdraw_liquidity(&user2, 7071_067711, withdraw_assets)
        .unwrap_err();
    assert_eq!(
        err.root_cause().to_string(),
        "Generic error: Imbalanced withdraw is currently disabled"
    );

    // user2 withdraws 1/10 of his LP tokens
    helper
        .withdraw_liquidity(&user2, 7071_067711, vec![])
        .unwrap();

    assert_eq!(
        70710_677118 - 7071_067711,
        helper.native_balance(&helper.lp_token, &user2)
    );
    assert_eq!(9382_010960, helper.coin_balance(&test_coins[0], &user2));
    assert_eq!(5330_688045, helper.coin_balance(&test_coins[1], &user2));

    // user3 withdraws half
    helper
        .withdraw_liquidity(&user3, 35355_339059, vec![])
        .unwrap();

    assert_eq!(
        70710_677118 + MINIMUM_LIQUIDITY_AMOUNT.u128() - 35355_339059,
        helper.native_balance(&helper.lp_token, &user3)
    );
    assert_eq!(46910_055478, helper.coin_balance(&test_coins[0], &user3));
    assert_eq!(26653_440612, helper.coin_balance(&test_coins[1], &user3));
}

#[test]
fn check_imbalanced_provide() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    let mut params = common_pcl_params();
    params.price_scale = Decimal::from_ratio(2u8, 1u8);

    let mut helper = Helper::new(&owner, test_coins.clone(), params.clone()).unwrap();

    let user1 = Addr::unchecked("user1");
    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
        helper.assets[&test_coins[1]].with_balance(100_000_000000u128),
    ];
    // Making two provides just to check that both if-branches are covered (initial and usual provide)
    helper.give_me_money(&assets, &user1);
    helper.provide_liquidity(&user1, &assets).unwrap();

    helper.give_me_money(&assets, &user1);
    helper.provide_liquidity(&user1, &assets).unwrap();

    assert_eq!(
        200495_366531,
        helper.native_balance(&helper.lp_token, &user1)
    );
    assert_eq!(0, helper.coin_balance(&test_coins[0], &user1));
    assert_eq!(0, helper.coin_balance(&test_coins[1], &user1));

    // creating a new pool with inverted price scale
    params.price_scale = Decimal::from_ratio(1u8, 2u8);

    let mut helper = Helper::new(&owner, test_coins.clone(), params).unwrap();

    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
        helper.assets[&test_coins[1]].with_balance(100_000_000000u128),
    ];
    helper.give_me_money(&assets, &user1);
    helper.provide_liquidity(&user1, &assets).unwrap();

    helper.give_me_money(&assets, &user1);
    helper.provide_liquidity(&user1, &assets).unwrap();

    assert_eq!(
        200495_366531,
        helper.native_balance(&helper.lp_token, &user1)
    );
    assert_eq!(0, helper.coin_balance(&test_coins[0], &user1));
    assert_eq!(0, helper.coin_balance(&test_coins[1], &user1));
}

#[test]
fn provide_with_different_precision() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("foo"), TestCoin::native("uosmo")];

    let mut helper = Helper::new(&owner, test_coins.clone(), common_pcl_params()).unwrap();

    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(100_00000u128),
        helper.assets[&test_coins[1]].with_balance(100_000000u128),
    ];

    helper.provide_liquidity(&owner, &assets).unwrap();

    let tolerance = 9;

    for user_name in ["user1", "user2", "user3"] {
        let user = Addr::unchecked(user_name);

        helper.give_me_money(&assets, &user);

        helper.provide_liquidity(&user, &assets).unwrap();

        let lp_amount = helper.native_balance(&helper.lp_token, &user);
        assert!(
            100_000000 - lp_amount < tolerance,
            "LP token balance assert failed for {user}"
        );
        assert_eq!(0, helper.coin_balance(&test_coins[0], &user));
        assert_eq!(0, helper.coin_balance(&test_coins[1], &user));

        helper.withdraw_liquidity(&user, lp_amount, vec![]).unwrap();

        assert_eq!(0, helper.native_balance(&helper.lp_token, &user));
        assert!(
            100_00000 - helper.coin_balance(&test_coins[0], &user) < tolerance,
            "Withdrawn amount of coin0 assert failed for {user}"
        );
        assert!(
            100_000000 - helper.coin_balance(&test_coins[1], &user) < tolerance,
            "Withdrawn amount of coin1 assert failed for {user}"
        );
    }
}

#[test]
fn swap_different_precisions() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("foo"), TestCoin::native("uosmo")];

    let mut helper = Helper::new(&owner, test_coins.clone(), common_pcl_params()).unwrap();

    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(100_000_00000u128),
        helper.assets[&test_coins[1]].with_balance(100_000_000000u128),
    ];
    helper.provide_liquidity(&owner, &assets).unwrap();

    let user = Addr::unchecked("user");
    // 100 x FOO tokens
    let offer_asset = helper.assets[&test_coins[0]].with_balance(100_00000u128);

    // Checking direct swap simulation
    let sim_resp = helper.simulate_swap(&offer_asset, None).unwrap();
    // And reverse swap as well
    let reverse_sim_resp = helper
        .simulate_reverse_swap(
            &helper.assets[&test_coins[1]].with_balance(sim_resp.return_amount.u128()),
            None,
        )
        .unwrap();
    assert_eq!(reverse_sim_resp.offer_amount.u128(), 10019003);
    assert_eq!(reverse_sim_resp.commission_amount.u128(), 45084);
    assert_eq!(reverse_sim_resp.spread_amount.u128(), 125);

    helper.give_me_money(&[offer_asset.clone()], &user);
    helper.swap(&user, &offer_asset, None).unwrap();

    assert_eq!(0, helper.coin_balance(&test_coins[0], &user));
    // 99_737929 x OSMO tokens
    assert_eq!(99_737929, sim_resp.return_amount.u128());
    assert_eq!(
        sim_resp.return_amount.u128(),
        helper.coin_balance(&test_coins[1], &user)
    );
}

#[test]
fn check_reverse_swap() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    let mut helper = Helper::new(&owner, test_coins.clone(), common_pcl_params()).unwrap();

    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
        helper.assets[&test_coins[1]].with_balance(100_000_000000u128),
    ];
    helper.provide_liquidity(&owner, &assets).unwrap();

    let offer_asset = helper.assets[&test_coins[0]].with_balance(50_000_000000u128);

    let sim_resp = helper.simulate_swap(&offer_asset, None).unwrap();
    let reverse_sim_resp = helper
        .simulate_reverse_swap(
            &helper.assets[&test_coins[1]].with_balance(sim_resp.return_amount.u128()),
            None,
        )
        .unwrap();
    assert_eq!(reverse_sim_resp.offer_amount.u128(), 50000220879u128); // as it is hard to predict dynamic fees reverse swap is not exact
    assert_eq!(reverse_sim_resp.commission_amount.u128(), 151_913981);
    assert_eq!(reverse_sim_resp.spread_amount.u128(), 16241_558397);
}

#[test]
fn check_swaps_simple() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    let mut helper = Helper::new(&owner, test_coins.clone(), common_pcl_params()).unwrap();

    let user = Addr::unchecked("user");
    let offer_asset = helper.assets[&test_coins[0]].with_balance(100_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user);

    // Check swap does not work if pool is empty
    let err = helper.swap(&user, &offer_asset, None).unwrap_err();
    assert_eq!(
        err.root_cause().to_string(),
        "Generic error: One of the pools is empty"
    );

    // Try to swap a wrong asset
    let wrong_coin = native_asset_info("random-coin".to_string());
    let wrong_asset = wrong_coin.with_balance(100_000000u128);
    helper.give_me_money(&[wrong_asset.clone()], &user);
    let err = helper.swap(&user, &wrong_asset, None).unwrap_err();
    assert_eq!(
        ContractError::InvalidAsset(wrong_coin.to_string()),
        err.downcast().unwrap()
    );

    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
        helper.assets[&test_coins[1]].with_balance(100_000_000000u128),
    ];
    helper.provide_liquidity(&owner, &assets).unwrap();

    let d = helper.query_d().unwrap();
    assert_eq!(dec_to_f64(d), 200000f64);

    assert_eq!(0, helper.coin_balance(&test_coins[1], &user));
    helper.swap(&user, &offer_asset, None).unwrap();
    assert_eq!(0, helper.coin_balance(&test_coins[0], &user));
    assert_eq!(99_737929, helper.coin_balance(&test_coins[1], &user));

    let offer_asset = helper.assets[&test_coins[0]].with_balance(90_000_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user);
    let err = helper.swap(&user, &offer_asset, None).unwrap_err();
    assert_eq!(
        ContractError::PclError(PclError::MaxSpreadAssertion {}),
        err.downcast().unwrap()
    );

    let user2 = Addr::unchecked("user2");
    let offer_asset = helper.assets[&test_coins[1]].with_balance(100_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user2);
    helper.swap(&user2, &offer_asset, None).unwrap();
    assert_eq!(0, helper.coin_balance(&test_coins[1], &user2));
    assert_eq!(99_741246, helper.coin_balance(&test_coins[0], &user2));

    let d = helper.query_d().unwrap();
    assert_eq!(dec_to_f64(d), 200000.260415);

    let price1 = helper.observe_price(0).unwrap();
    helper.app.next_block(10);
    // Swapping the lowest amount possible which results in positive return amount
    helper
        .swap(
            &user,
            &helper.assets[&test_coins[1]].with_balance(2u128),
            None,
        )
        .unwrap();
    let price2 = helper.observe_price(0).unwrap();
    // With such a small swap size contract doesn't store observation
    assert_eq!(price1, price2);

    helper.app.next_block(10);
    // Swap the smallest possible amount which gets observation saved
    helper
        .swap(
            &user,
            &helper.assets[&test_coins[1]].with_balance(1005u128),
            None,
        )
        .unwrap();
    let price3 = helper.observe_price(0).unwrap();
    // Prove that price didn't jump that much
    let diff = price3.diff(price2);
    assert!(
        diff / price2 < f64_to_dec(0.005),
        "price jumped from {price2} to {price3} which is more than 0.5%"
    );
}

#[test]
fn check_swaps_with_price_update() {
    let owner = Addr::unchecked("owner");
    let half = Decimal::from_ratio(1u8, 2u8);

    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    let mut helper = Helper::new(&owner, test_coins.clone(), common_pcl_params()).unwrap();

    helper.app.next_block(1000);

    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
        helper.assets[&test_coins[1]].with_balance(100_000_000000u128),
    ];
    helper.provide_liquidity(&owner, &assets).unwrap();

    helper.app.next_block(1000);

    let user1 = Addr::unchecked("user1");
    let offer_asset = helper.assets[&test_coins[1]].with_balance(10_000_000000u128);
    let mut prev_vlp_price = helper.query_lp_price().unwrap();

    for i in 0..4 {
        helper.give_me_money(&[offer_asset.clone()], &user1);
        helper.swap(&user1, &offer_asset, Some(half)).unwrap();
        let new_vlp_price = helper.query_lp_price().unwrap();
        assert!(
            new_vlp_price >= prev_vlp_price,
            "{i}: new_vlp_price <= prev_vlp_price ({new_vlp_price} <= {prev_vlp_price})",
        );
        prev_vlp_price = new_vlp_price;
        helper.app.next_block(1000);
    }

    let offer_asset = helper.assets[&test_coins[0]].with_balance(10_000_000000u128);
    for _i in 0..4 {
        helper.give_me_money(&[offer_asset.clone()], &user1);
        helper.swap(&user1, &offer_asset, Some(half)).unwrap();
        helper.app.next_block(1000);
    }
}

#[test]
fn provides_and_swaps() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    let mut helper = Helper::new(&owner, test_coins.clone(), common_pcl_params()).unwrap();

    helper.app.next_block(1000);

    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
        helper.assets[&test_coins[1]].with_balance(100_000_000000u128),
    ];
    helper.provide_liquidity(&owner, &assets).unwrap();

    helper.app.next_block(1000);

    let user = Addr::unchecked("user");
    let offer_asset = helper.assets[&test_coins[0]].with_balance(100_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user);
    helper.swap(&user, &offer_asset, None).unwrap();

    let provider = Addr::unchecked("provider");
    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(1_000_000000u128),
        helper.assets[&test_coins[1]].with_balance(1_000_000000u128),
    ];
    helper.give_me_money(&assets, &provider);
    helper.provide_liquidity(&provider, &assets).unwrap();

    let offer_asset = helper.assets[&test_coins[1]].with_balance(100_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user);
    helper.swap(&user, &offer_asset, None).unwrap();

    helper
        .withdraw_liquidity(&provider, 999_999354, vec![])
        .unwrap();

    let offer_asset = helper.assets[&test_coins[0]].with_balance(100_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user);
    helper.swap(&user, &offer_asset, None).unwrap();
}

#[test]
fn check_amp_gamma_change() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    let mut params = common_pcl_params();
    params.gamma = f64_to_dec(0.0001);
    let mut helper = Helper::new(&owner, test_coins, params).unwrap();

    let random_user = Addr::unchecked("random");
    let action = ConcentratedPoolUpdateParams::Update(UpdatePoolParams {
        mid_fee: Some(f64_to_dec(0.002)),
        out_fee: None,
        fee_gamma: None,
        repeg_profit_threshold: None,
        min_price_scale_delta: None,
        ma_half_time: None,
    });

    let err = helper.update_config(&random_user, &action).unwrap_err();
    assert_eq!(ContractError::Unauthorized {}, err.downcast().unwrap());

    helper.update_config(&owner, &action).unwrap();

    helper.app.next_block(86400);

    let future_time = helper.app.block_info().time.seconds() + 100_000;
    let target_amp = 44f64;
    let target_gamma = 0.00009;
    let action = ConcentratedPoolUpdateParams::Promote(PromoteParams {
        next_amp: f64_to_dec(target_amp),
        next_gamma: f64_to_dec(target_gamma),
        future_time,
    });
    helper.update_config(&owner, &action).unwrap();

    let amp_gamma = helper.query_amp_gamma().unwrap();
    assert_eq!(dec_to_f64(amp_gamma.amp), 40f64);
    assert_eq!(dec_to_f64(amp_gamma.gamma), 0.0001);
    assert_eq!(amp_gamma.future_time, future_time);

    helper.app.next_block(50_000);

    let amp_gamma = helper.query_amp_gamma().unwrap();
    assert_eq!(dec_to_f64(amp_gamma.amp), 42f64);
    assert_eq!(dec_to_f64(amp_gamma.gamma), 0.000095);
    assert_eq!(amp_gamma.future_time, future_time);

    helper.app.next_block(50_000);

    let amp_gamma = helper.query_amp_gamma().unwrap();
    assert_eq!(dec_to_f64(amp_gamma.amp), target_amp);
    assert_eq!(dec_to_f64(amp_gamma.gamma), target_gamma);
    assert_eq!(amp_gamma.future_time, future_time);

    // change values back
    let future_time = helper.app.block_info().time.seconds() + 100_000;
    let action = ConcentratedPoolUpdateParams::Promote(PromoteParams {
        next_amp: f64_to_dec(40f64),
        next_gamma: f64_to_dec(0.000099),
        future_time,
    });
    helper.update_config(&owner, &action).unwrap();

    helper.app.next_block(50_000);

    let amp_gamma = helper.query_amp_gamma().unwrap();
    assert_eq!(dec_to_f64(amp_gamma.amp), 42f64);
    assert_eq!(dec_to_f64(amp_gamma.gamma), 0.0000945);
    assert_eq!(amp_gamma.future_time, future_time);

    // stop changing amp and gamma thus fixing current values
    let action = ConcentratedPoolUpdateParams::StopChangingAmpGamma {};
    helper.update_config(&owner, &action).unwrap();
    let amp_gamma = helper.query_amp_gamma().unwrap();
    let last_change_time = helper.app.block_info().time.seconds();
    assert_eq!(amp_gamma.future_time, last_change_time);

    helper.app.next_block(50_000);

    let amp_gamma = helper.query_amp_gamma().unwrap();
    assert_eq!(dec_to_f64(amp_gamma.amp), 42f64);
    assert_eq!(dec_to_f64(amp_gamma.gamma), 0.0000945);
    assert_eq!(amp_gamma.future_time, last_change_time);
}

#[test]
fn check_prices() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    let helper = Helper::new(&owner, test_coins, common_pcl_params()).unwrap();
    let err = helper.query_prices().unwrap_err();
    assert_eq!(StdError::generic_err("Querier contract error: Generic error: Not implemented.Use { \"observe\" : { \"seconds_ago\" : ... } } instead.")
    , err);
}

#[test]
fn update_owner() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    let mut helper = Helper::new(&owner, test_coins, common_pcl_params()).unwrap();

    let new_owner = String::from("new_owner");

    // New owner
    let msg = ExecuteMsg::ProposeNewOwner {
        owner: new_owner.clone(),
        expires_in: 100, // seconds
    };

    // Unauthorized check
    let err = helper
        .app
        .execute_contract(
            Addr::unchecked("not_owner"),
            helper.pair_addr.clone(),
            &msg,
            &[],
        )
        .unwrap_err();
    assert_eq!(err.root_cause().to_string(), "Generic error: Unauthorized");

    // Claim before proposal
    let err = helper
        .app
        .execute_contract(
            Addr::unchecked(new_owner.clone()),
            helper.pair_addr.clone(),
            &ExecuteMsg::ClaimOwnership {},
            &[],
        )
        .unwrap_err();
    assert_eq!(
        err.root_cause().to_string(),
        "Generic error: Ownership proposal not found"
    );

    // Propose new owner
    helper
        .app
        .execute_contract(
            Addr::unchecked(&helper.owner),
            helper.pair_addr.clone(),
            &msg,
            &[],
        )
        .unwrap();

    // Claim from invalid addr
    let err = helper
        .app
        .execute_contract(
            Addr::unchecked("invalid_addr"),
            helper.pair_addr.clone(),
            &ExecuteMsg::ClaimOwnership {},
            &[],
        )
        .unwrap_err();
    assert_eq!(err.root_cause().to_string(), "Generic error: Unauthorized");

    // Drop ownership proposal
    let err = helper
        .app
        .execute_contract(
            Addr::unchecked("invalid_addr"),
            helper.pair_addr.clone(),
            &ExecuteMsg::DropOwnershipProposal {},
            &[],
        )
        .unwrap_err();
    assert_eq!(err.root_cause().to_string(), "Generic error: Unauthorized");

    helper
        .app
        .execute_contract(
            helper.owner.clone(),
            helper.pair_addr.clone(),
            &ExecuteMsg::DropOwnershipProposal {},
            &[],
        )
        .unwrap();

    // Propose new owner
    helper
        .app
        .execute_contract(
            Addr::unchecked(&helper.owner),
            helper.pair_addr.clone(),
            &msg,
            &[],
        )
        .unwrap();

    // Claim ownership
    helper
        .app
        .execute_contract(
            Addr::unchecked(new_owner.clone()),
            helper.pair_addr.clone(),
            &ExecuteMsg::ClaimOwnership {},
            &[],
        )
        .unwrap();

    let config = helper.query_config().unwrap();
    assert_eq!(config.owner.unwrap().to_string(), new_owner)
}

#[test]
fn query_d_test() {
    let owner = Addr::unchecked("owner");
    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    // create pair with test_coins
    let helper = Helper::new(&owner, test_coins, common_pcl_params()).unwrap();

    // query current pool D value before providing any liquidity
    let err = helper.query_d().unwrap_err();
    assert_eq!(
        err.to_string(),
        "Generic error: Querier contract error: Generic error: Pools are empty"
    );
}

#[test]
fn asset_balances_tracking_without_in_params() {
    let owner = Addr::unchecked("owner");
    let user1 = Addr::unchecked("user1");
    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    // Instantiate pair without asset balances tracking
    let mut helper = Helper::new(&owner, test_coins.clone(), common_pcl_params()).unwrap();

    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(5_000000u128),
        helper.assets[&test_coins[1]].with_balance(5_000000u128),
    ];

    // Check that asset balances are not tracked
    // The query AssetBalanceAt returns None for this case
    let res = helper
        .query_asset_balance_at(&assets[0].info, helper.app.block_info().height)
        .unwrap();
    assert!(res.is_none());

    let res = helper
        .query_asset_balance_at(&assets[1].info, helper.app.block_info().height)
        .unwrap();
    assert!(res.is_none());

    // Enable asset balances tracking
    helper
        .update_config(
            &owner,
            &ConcentratedPoolUpdateParams::EnableAssetBalancesTracking {},
        )
        .unwrap();

    // Check that asset balances were not tracked before this was enabled
    // The query AssetBalanceAt returns None for this case
    let res = helper
        .query_asset_balance_at(&assets[0].info, helper.app.block_info().height)
        .unwrap();
    assert!(res.is_none());

    let res = helper
        .query_asset_balance_at(&assets[1].info, helper.app.block_info().height)
        .unwrap();
    assert!(res.is_none());

    // Check that asset balances had zero balances before next block upon tracking enabling
    helper.app.update_block(|b| b.height += 1);

    let res = helper
        .query_asset_balance_at(&assets[0].info, helper.app.block_info().height)
        .unwrap();
    assert!(res.unwrap().is_zero());

    let res = helper
        .query_asset_balance_at(&assets[1].info, helper.app.block_info().height)
        .unwrap();
    assert!(res.unwrap().is_zero());

    helper.give_me_money(&assets, &user1);
    helper.provide_liquidity(&user1, &assets).unwrap();

    // Check that asset balances changed after providing liqudity
    helper.app.update_block(|b| b.height += 1);
    let res = helper
        .query_asset_balance_at(&assets[0].info, helper.app.block_info().height)
        .unwrap();
    assert_eq!(res.unwrap(), Uint128::new(5_000000));

    let res = helper
        .query_asset_balance_at(&assets[1].info, helper.app.block_info().height)
        .unwrap();
    assert_eq!(res.unwrap(), Uint128::new(5_000000));
}

#[test]
fn asset_balances_tracking_with_in_params() {
    let owner = Addr::unchecked("owner");
    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    // Instantiate pair without asset balances tracking
    let mut params = common_pcl_params();
    params.track_asset_balances = Some(true);
    let mut helper = Helper::new(&owner, test_coins.clone(), params).unwrap();

    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(5_000000u128),
        helper.assets[&test_coins[1]].with_balance(5_000000u128),
    ];

    // Check that enabling asset balances tracking can not be done if it is already enabled
    let err = helper
        .update_config(
            &owner,
            &ConcentratedPoolUpdateParams::EnableAssetBalancesTracking {},
        )
        .unwrap_err();
    assert_eq!(
        err.downcast::<ContractError>().unwrap(),
        ContractError::AssetBalancesTrackingIsAlreadyEnabled {}
    );
    // Check that asset balances were not tracked before instantiation
    // The query AssetBalanceAt returns None for this case
    let res = helper
        .query_asset_balance_at(&assets[0].info, helper.app.block_info().height)
        .unwrap();
    assert!(res.is_none());

    let res = helper
        .query_asset_balance_at(&assets[1].info, helper.app.block_info().height)
        .unwrap();
    assert!(res.is_none());

    // Check that asset balances were not tracked before instantiation
    // The query AssetBalanceAt returns None for this case
    let res = helper
        .query_asset_balance_at(&assets[0].info, helper.app.block_info().height)
        .unwrap();
    assert!(res.is_none());

    let res = helper
        .query_asset_balance_at(&assets[1].info, helper.app.block_info().height)
        .unwrap();
    assert!(res.is_none());

    // Check that asset balances had zero balances before next block upon instantiation
    helper.app.update_block(|b| b.height += 1);

    let res = helper
        .query_asset_balance_at(&assets[0].info, helper.app.block_info().height)
        .unwrap();
    assert!(res.unwrap().is_zero());

    let res = helper
        .query_asset_balance_at(&assets[1].info, helper.app.block_info().height)
        .unwrap();
    assert!(res.unwrap().is_zero());

    // Provide liquidity
    helper
        .provide_liquidity(
            &owner,
            &[
                assets[0].info.with_balance(999_000000u128),
                assets[1].info.with_balance(1000_000000u128),
            ],
        )
        .unwrap();

    assert_eq!(
        helper.native_balance(&helper.lp_token, &owner),
        999_498998u128
    );

    // Check that asset balances changed after providing liquidity
    helper.app.update_block(|b| b.height += 1);
    let res = helper
        .query_asset_balance_at(&assets[0].info, helper.app.block_info().height)
        .unwrap();
    assert_eq!(res.unwrap(), Uint128::new(999_000000));

    let res = helper
        .query_asset_balance_at(&assets[1].info, helper.app.block_info().height)
        .unwrap();
    assert_eq!(res.unwrap(), Uint128::new(1000_000000));

    // Swap
    helper
        .swap(
            &owner,
            &Asset {
                info: AssetInfo::NativeToken {
                    denom: "uusd".to_owned(),
                },
                amount: Uint128::new(1_000000),
            },
            None,
        )
        .unwrap();

    // Check that asset balances changed after swapping
    helper.app.update_block(|b| b.height += 1);
    let res = helper
        .query_asset_balance_at(&assets[0].info, helper.app.block_info().height)
        .unwrap();
    assert_eq!(res.unwrap(), Uint128::new(998_001335));

    let res = helper
        .query_asset_balance_at(&assets[1].info, helper.app.block_info().height)
        .unwrap();
    assert_eq!(res.unwrap(), Uint128::new(1001_000000));

    // Withdraw liquidity
    helper
        .withdraw_liquidity(&owner, 500_000000, vec![])
        .unwrap();

    // Check that asset balances changed after withdrawing
    helper.app.update_block(|b| b.height += 1);
    let res = helper
        .query_asset_balance_at(&assets[0].info, helper.app.block_info().height)
        .unwrap();
    assert_eq!(res.unwrap(), Uint128::new(498_751043));

    let res = helper
        .query_asset_balance_at(&assets[1].info, helper.app.block_info().height)
        .unwrap();
    assert_eq!(res.unwrap(), Uint128::new(500_249625));
}

#[test]
fn provides_and_swaps_and_withdraw() {
    let owner = Addr::unchecked("owner");
    let half = Decimal::from_ratio(1u8, 2u8);
    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    let mut params = common_pcl_params();
    params.price_scale = Decimal::from_ratio(1u8, 2u8);
    let mut helper = Helper::new(&owner, test_coins.clone(), params).unwrap();

    helper.app.next_block(1000);

    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
        helper.assets[&test_coins[1]].with_balance(200_000_000000u128),
    ];
    helper.provide_liquidity(&owner, &assets).unwrap();

    // swap uosmo
    let user = Addr::unchecked("user");
    let offer_asset = helper.assets[&test_coins[0]].with_balance(1000_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user);
    helper.swap(&user, &offer_asset, Some(half)).unwrap();

    helper.app.next_block(1000);

    // swap usdc
    let offer_asset = helper.assets[&test_coins[1]].with_balance(1000_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user);
    helper.swap(&user, &offer_asset, Some(half)).unwrap();

    let offer_asset = helper.assets[&test_coins[1]].with_balance(100_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user);
    helper.swap(&user, &offer_asset, Some(half)).unwrap();

    // swap uosmo
    let offer_asset = helper.assets[&test_coins[0]].with_balance(100_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user);
    helper.swap(&user, &offer_asset, Some(half)).unwrap();
    let res: PoolResponse = helper
        .app
        .wrap()
        .query_wasm_smart(helper.pair_addr.to_string(), &QueryMsg::Pool {})
        .unwrap();

    assert_eq!(res.total_share.u128(), 141_421_356_237u128);
    let owner_balance = helper.native_balance(&helper.lp_token, &owner);

    helper
        .withdraw_liquidity(&owner, owner_balance, vec![])
        .unwrap();
    let res: PoolResponse = helper
        .app
        .wrap()
        .query_wasm_smart(helper.pair_addr.to_string(), &QueryMsg::Pool {})
        .unwrap();

    assert_eq!(res.total_share.u128(), 1000u128);
}

#[test]
fn provide_withdraw_provide() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("uusd"), TestCoin::native("uosmo")];

    let params = ConcentratedPoolParams {
        amp: f64_to_dec(10f64),
        price_scale: Decimal::from_ratio(10u8, 1u8),
        ..common_pcl_params()
    };

    let mut helper = Helper::new(&owner, test_coins.clone(), params).unwrap();

    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(10_938039u128),
        helper.assets[&test_coins[1]].with_balance(1_093804u128),
    ];
    helper.provide_liquidity(&owner, &assets).unwrap();
    helper.app.next_block(90);
    helper.provide_liquidity(&owner, &assets).unwrap();

    helper.app.next_block(90);
    let uusd = helper.assets[&test_coins[0]].with_balance(5_000000u128);
    helper.swap(&owner, &uusd, Some(f64_to_dec(0.5))).unwrap();

    helper.app.next_block(600);
    // Withdraw all
    let lp_amount = helper.native_balance(&helper.lp_token, &owner);
    helper
        .withdraw_liquidity(&owner, lp_amount, vec![])
        .unwrap();

    // Provide again
    helper
        .provide_liquidity_with_slip_tolerance(&owner, &assets, Some(f64_to_dec(0.5)))
        .unwrap();
}

#[test]
fn provide_withdraw_slippage() {
    let owner = Addr::unchecked("owner");
    let test_coins = vec![TestCoin::native("uusd"), TestCoin::native("uosmo")];
    let mut params = common_pcl_params();
    params.price_scale = Decimal::from_ratio(10u8, 1u8);

    let mut helper = Helper::new(&owner, test_coins.clone(), params).unwrap();

    // Fully balanced provide
    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(10_000000u128),
        helper.assets[&test_coins[1]].with_balance(1_000000u128),
    ];
    helper
        .provide_liquidity_with_slip_tolerance(&owner, &assets, Some(f64_to_dec(0.02)))
        .unwrap();

    // Imbalanced provide. Slippage is more than 2% while we enforce 2% max slippage
    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(5_000000u128),
        helper.assets[&test_coins[1]].with_balance(1_000000u128),
    ];
    let err = helper
        .provide_liquidity_with_slip_tolerance(&owner, &assets, Some(f64_to_dec(0.02)))
        .unwrap_err();
    assert_eq!(
        ContractError::PclError(PclError::MaxSpreadAssertion {}),
        err.downcast().unwrap(),
    );
    // With 3% slippage it should work
    helper
        .provide_liquidity_with_slip_tolerance(&owner, &assets, Some(f64_to_dec(0.03)))
        .unwrap();

    // Provide with a huge imbalance. Slippage is ~42.2%
    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(1000_000000u128),
        helper.assets[&test_coins[1]].with_balance(1000_000000u128),
    ];
    let err = helper
        .provide_liquidity_with_slip_tolerance(&owner, &assets, Some(f64_to_dec(0.02)))
        .unwrap_err();
    assert_eq!(
        ContractError::PclError(PclError::MaxSpreadAssertion {}),
        err.downcast().unwrap(),
    );
    helper
        .provide_liquidity_with_slip_tolerance(&owner, &assets, Some(f64_to_dec(0.5)))
        .unwrap();
}

#[test]
fn test_frontrun_before_initial_provide() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("uusd"), TestCoin::native("uosmo")];

    let params = ConcentratedPoolParams {
        amp: f64_to_dec(10f64),
        price_scale: Decimal::from_ratio(10u8, 1u8),
        ..common_pcl_params()
    };

    let mut helper = Helper::new(&owner, test_coins.clone(), params).unwrap();

    // Random person tries to frontrun initial provide and imbalance pool upfront
    helper
        .app
        .send_tokens(
            owner.clone(),
            helper.pair_addr.clone(),
            &[helper.assets[&test_coins[0]]
                .with_balance(10_000_000000u128)
                .as_coin()
                .unwrap()],
        )
        .unwrap();

    // Fully balanced provide
    let assets = vec![
        helper.assets[&test_coins[0]].with_balance(10_000000u128),
        helper.assets[&test_coins[1]].with_balance(1_000000u128),
    ];
    helper.provide_liquidity(&owner, &assets).unwrap();
    // Now pool became imbalanced with value (10010, 1)  (or in internal representation (10010, 10))
    // while price scale stays 10

    let arber = Addr::unchecked("arber");
    let offer_asset_luna = helper.assets[&test_coins[1]].with_balance(1_000000u128);
    // Arber spinning pool back to balanced state
    loop {
        helper.app.next_block(10);
        helper.give_me_money(&[offer_asset_luna.clone()], &arber);
        // swapping until price satisfies an arber
        if helper
            .swap_full_params(
                &arber,
                &offer_asset_luna,
                Some(f64_to_dec(0.02)),
                Some(f64_to_dec(0.1)), // imagine market price is 10 -> i.e. inverted price is 1/10
            )
            .is_err()
        {
            break;
        }
    }

    // price scale changed, however it isn't equal to 10 because of repegging
    // But next swaps will align price back to the market value
    let config = helper.query_config().unwrap();
    let price_scale = config.pool_state.price_state.price_scale;
    assert!(
        dec_to_f64(price_scale) - 77.255853 < 1e-5,
        "price_scale: {price_scale} is far from expected price",
    );

    // Arber collected significant profit (denominated in uusd)
    // Essentially 10_000 - fees (which settled in the pool)
    let arber_balance = helper.coin_balance(&test_coins[0], &arber);
    assert_eq!(arber_balance, 9667_528248);

    // Pool's TVL increased from (10, 1) i.e. 20 to (320, 32) i.e. 640 considering market price is 10.0
    let pools = config
        .pair_info
        .query_pools(&helper.app.wrap(), &helper.pair_addr)
        .unwrap();
    assert_eq!(pools[0].amount.u128(), 320_624088);
    assert_eq!(pools[1].amount.u128(), 32_000000);
}

#[test]
fn test_osmosis_specific_queries() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    let mut helper = Helper::new(&owner, test_coins.clone(), common_pcl_params()).unwrap();

    let provide_assets = [
        helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
        helper.assets[&test_coins[1]].with_balance(100_000_000000u128),
    ];
    helper.provide_liquidity(&owner, &provide_assets).unwrap();

    let offer_asset = helper.assets[&test_coins[0]].with_balance(100_000000u128);
    let user = Addr::unchecked("user");
    for _ in 0..10 {
        helper.give_me_money(&[offer_asset.clone()], &user);
        helper.swap(&user, &offer_asset, None).unwrap();
    }

    let osm_liq = helper
        .app
        .wrap()
        .query_wasm_smart::<TotalPoolLiquidityResponse>(
            &helper.pair_addr,
            &QueryMsg::GetTotalPoolLiquidity {},
        )
        .unwrap()
        .total_pool_liquidity;
    let astro_liq = helper
        .app
        .wrap()
        .query_wasm_smart::<PoolResponse>(&helper.pair_addr, &QueryMsg::Pool {})
        .unwrap()
        .assets
        .into_iter()
        .map(|a| a.as_coin().unwrap())
        .collect_vec();
    assert_eq!(osm_liq, astro_liq);

    let osm_resp = helper
        .app
        .wrap()
        .query_wasm_smart::<CalcOutAmtGivenInResponse>(
            &helper.pair_addr,
            &QueryMsg::CalcOutAmtGivenIn {
                token_in: offer_asset.as_coin().unwrap(),
                token_out_denom: "doesnt_matter".to_string(),
                swap_fee: Default::default(),
            },
        )
        .unwrap();
    let astro_resp = helper.simulate_swap(&offer_asset, None).unwrap();
    assert_eq!(osm_resp.token_out.amount, astro_resp.return_amount);

    let osm_resp = helper
        .app
        .wrap()
        .query_wasm_smart::<CalcInAmtGivenOutResponse>(
            &helper.pair_addr,
            &QueryMsg::CalcInAmtGivenOut {
                token_out: offer_asset.as_coin().unwrap(),
                token_in_denom: offer_asset.as_coin().unwrap().denom,
                swap_fee: Default::default(),
            },
        )
        .unwrap();
    let astro_resp = helper.simulate_reverse_swap(&offer_asset, None).unwrap();
    assert_eq!(osm_resp.token_in.amount, astro_resp.offer_amount);

    let osm_resp = helper
        .app
        .wrap()
        .query_wasm_smart::<SpotPriceResponse>(
            &helper.pair_addr,
            &QueryMsg::SpotPrice {
                quote_asset_denom: helper.assets[&test_coins[1]].to_string(),
                base_asset_denom: helper.assets[&test_coins[0]].to_string(),
            },
        )
        .unwrap();
    let astro_resp = helper.observe_price(0).unwrap();
    assert_eq!(osm_resp.spot_price, astro_resp);

    // query inverted price
    let osm_resp = helper
        .app
        .wrap()
        .query_wasm_smart::<SpotPriceResponse>(
            &helper.pair_addr,
            &QueryMsg::SpotPrice {
                quote_asset_denom: helper.assets[&test_coins[0]].to_string(),
                base_asset_denom: helper.assets[&test_coins[1]].to_string(),
            },
        )
        .unwrap();
    assert_eq!(osm_resp.spot_price, astro_resp.inv().unwrap());

    let err = helper
        .app
        .wrap()
        .query_wasm_smart::<SpotPriceResponse>(
            &helper.pair_addr,
            &QueryMsg::SpotPrice {
                quote_asset_denom: "random".to_string(),
                base_asset_denom: helper.assets[&test_coins[0]].to_string(),
            },
        )
        .unwrap_err();
    assert_eq!(
        err,
        StdError::generic_err(format!(
            "Querier contract error: Generic error: Invalid pool denoms random {base}. Must be {base} {quote}",
            quote = helper.assets[&test_coins[1]],
            base = helper.assets[&test_coins[0]].to_string()
        ))
    );
}

#[test]
fn check_reverse_swaps() {
    let owner = Addr::unchecked("owner");

    let test_coins = vec![TestCoin::native("uosmo"), TestCoin::native("uusd")];

    let params = ConcentratedPoolParams {
        track_asset_balances: Some(true),
        ..common_pcl_params()
    };
    let mut helper = Helper::new(&owner, test_coins.clone(), params).unwrap();

    let provide_assets = [
        helper.assets[&test_coins[0]].with_balance(100_000_000000u128),
        helper.assets[&test_coins[1]].with_balance(100_000_000000u128),
    ];
    helper.provide_liquidity(&owner, &provide_assets).unwrap();

    let user = Addr::unchecked("user");
    let offer_asset = helper.assets[&test_coins[0]].with_balance(100_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user);

    // exchange rate is not 1:1 thus pool is not able to perform such reverse swap
    let ask_asset = helper.assets[&test_coins[1]].with_balance(100_000000u128);
    let err = helper
        .reverse_swap(&user, &ask_asset, &offer_asset)
        .unwrap_err();
    assert_eq!(err.root_cause().to_string(), "Generic error: Not enough tokens to perform swap. Need 100453296 but token_in_max_amount is 100000000");

    // check the user still holds their uosmo token and didn't receive uusd
    assert_eq!(helper.coin_balance(&test_coins[0], &user), 100_000000);
    assert_eq!(helper.coin_balance(&test_coins[1], &user), 0);

    // Ask slightly less uusd
    let ask_asset = helper.assets[&test_coins[1]].with_balance(95_000000u128);
    helper
        .reverse_swap(&user, &ask_asset, &offer_asset)
        .unwrap();

    // check balances. User received slightly more uusd than expected cuz PCL has dynamic fee
    // and it can't forecast exact fee rate in reverse swaps
    assert_eq!(helper.coin_balance(&test_coins[0], &user), 4_569430);
    assert_eq!(helper.coin_balance(&test_coins[1], &user), 95_180600);

    helper.app.update_block(next_block);

    // Check that asset balance is being tracked
    let res = helper
        .query_asset_balance_at(
            &helper.assets[&test_coins[0]],
            helper.app.block_info().height,
        )
        .unwrap();
    assert_eq!(res.unwrap().u128(), 100095_430570);

    // make reverse swap in opposite direction
    let user2 = Addr::unchecked("user2");
    let offer_asset = helper.assets[&test_coins[1]].with_balance(100_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user2);
    let ask_asset = helper.assets[&test_coins[0]].with_balance(95_000000u128);
    helper
        .reverse_swap(&user2, &ask_asset, &offer_asset)
        .unwrap();
    assert_eq!(helper.coin_balance(&test_coins[0], &user), 4_569430);
    assert_eq!(helper.coin_balance(&test_coins[1], &user), 95_180600);

    // try to abuse reverse swap and use equal offer and ask assets
    let user3 = Addr::unchecked("user3");
    let offer_asset = helper.assets[&test_coins[1]].with_balance(100_000000u128);
    helper.give_me_money(&[offer_asset.clone()], &user3);
    let ask_asset = helper.assets[&test_coins[1]].with_balance(95_000000u128);
    let err = helper
        .reverse_swap(&user3, &ask_asset, &offer_asset)
        .unwrap_err();
    assert_eq!(
        err.root_cause().to_string(),
        format!(
            "Generic error: Invalid swap: {0} to {0}",
            offer_asset.info.to_string()
        )
    );
}
