use astroport::asset::{Asset, AssetInfo};
use cosmwasm_std::{coin, Decimal};
use osmosis_test_tube::OsmosisTestApp;

use astroport_osmo_e2e_tests::helper::{default_pcl_params, TestAppWrapper};

#[test]
fn collect_fees_test() {
    let app = OsmosisTestApp::new();
    let helper = TestAppWrapper::bootstrap(&app).unwrap();

    // Create and seed ASTRO pool
    let uusd_denom = helper.register_and_mint("uusd", 200_000_000_000000, 6, None);
    let (pair_addr, _) = helper
        .create_pair(
            &[
                AssetInfo::native(&helper.astro_denom),
                AssetInfo::native(&uusd_denom),
            ],
            default_pcl_params(Decimal::one()),
        )
        .unwrap();
    helper
        .provide(
            &helper.signer,
            &pair_addr,
            &[
                Asset::native(&helper.astro_denom, 1_000_000_000000u64),
                Asset::native(&uusd_denom, 1_000_000_000000u64),
            ],
            None,
        )
        .unwrap();
    let astro_pool = helper.get_pool_id_by_contract(&pair_addr);

    let ucoin_denom = helper.register_and_mint("ucoin", 200_000_000_000000, 6, None);
    let (pair_addr, _) = helper
        .create_pair(
            &[
                AssetInfo::native(&ucoin_denom),
                AssetInfo::native(&uusd_denom),
            ],
            default_pcl_params(Decimal::one()),
        )
        .unwrap();
    helper
        .provide(
            &helper.signer,
            &pair_addr,
            &[
                Asset::native(&ucoin_denom, 1_000_000_000000u64),
                Asset::native(&uusd_denom, 1_000_000_000000u64),
            ],
            None,
        )
        .unwrap();
    let pool_1 = helper.get_pool_id_by_contract(&pair_addr);

    // Set routes
    helper
        .wasm
        .execute(
            &helper.maker,
            &astroport_on_osmosis::maker::ExecuteMsg::SetPoolRoutes(vec![
                astroport_on_osmosis::maker::PoolRoute {
                    denom_in: ucoin_denom.clone(),
                    denom_out: uusd_denom.clone(),
                    pool_id: pool_1,
                },
                astroport_on_osmosis::maker::PoolRoute {
                    denom_in: uusd_denom.clone(),
                    denom_out: helper.astro_denom.to_owned(),
                    pool_id: astro_pool,
                },
            ]),
            &[],
            &helper.signer,
        )
        .unwrap();

    // Mock receiving fees
    helper.mint(coin(1_000000, &ucoin_denom), Some(helper.maker.clone()));

    // Collect fees
    let err = helper
        .wasm
        .execute(
            &helper.maker,
            &astroport_on_osmosis::maker::ExecuteMsg::Collect {
                assets: vec![astroport_on_osmosis::maker::CoinWithLimit {
                    denom: ucoin_denom,
                    amount: None,
                }],
            },
            &[],
            &helper.signer,
        )
        .unwrap_err();
    // Assert we receive IBC error which means all other collect steps passed.
    // test-tube doesn't support IBC thus it is correct to assert this error.
    assert!(
        err.to_string()
            .contains("port ID (transfer) channel ID (channel-1): channel not found"),
        "unexpected error: {err}"
    );
}
