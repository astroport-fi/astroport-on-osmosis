use cosmwasm_std::{coin, Addr};
use itertools::Itertools;

use astroport_maker_osmosis::error::ContractError;
use astroport_on_osmosis::maker::{PoolRoute, SwapRouteResponse, MAX_SWAPS_DEPTH};

use crate::common::helper::{Helper, ASTRO_DENOM};

mod common;
#[test]
fn check_set_routes() {
    let owner = Addr::unchecked("owner");
    let mut helper = Helper::new(&owner).unwrap();

    let (_, astro_pool_id) = helper
        .create_and_seed_pair([
            coin(1_000_000_000000, "uusd"),
            coin(1_000_000_000000, ASTRO_DENOM),
        ])
        .unwrap();

    let (_, pool_1) = helper
        .create_and_seed_pair([
            coin(1_000_000_000000, "ucoin"),
            coin(1_000_000_000000, "uusd"),
        ])
        .unwrap();

    // Set wrong pool id
    let err = helper
        .set_pool_routes(vec![PoolRoute {
            denom_in: "ucoin".to_string(),
            denom_out: "uusd".to_string(),
            pool_id: astro_pool_id,
        }])
        .unwrap_err();
    assert_eq!(
        err.downcast::<ContractError>().unwrap(),
        ContractError::InvalidPoolDenom {
            pool_id: astro_pool_id,
            denom: "ucoin".to_string()
        }
    );
    let err = helper
        .set_pool_routes(vec![PoolRoute {
            denom_in: "ucoin".to_string(),
            denom_out: "rand".to_string(),
            pool_id: pool_1,
        }])
        .unwrap_err();
    assert_eq!(
        err.downcast::<ContractError>().unwrap(),
        ContractError::InvalidPoolDenom {
            pool_id: pool_1,
            denom: "rand".to_string()
        }
    );

    // ucoin -> uusd -> astro
    helper
        .set_pool_routes(vec![
            PoolRoute {
                denom_in: "ucoin".to_string(),
                denom_out: "uusd".to_string(),
                pool_id: pool_1,
            },
            PoolRoute {
                denom_in: "uusd".to_string(),
                denom_out: ASTRO_DENOM.to_string(),
                pool_id: astro_pool_id,
            },
        ])
        .unwrap();

    let route = helper.query_route("ucoin", ASTRO_DENOM);
    assert_eq!(
        route,
        vec![
            SwapRouteResponse {
                pool_id: pool_1,
                token_out_denom: "uusd".to_string(),
            },
            SwapRouteResponse {
                token_out_denom: ASTRO_DENOM.to_string(),
                pool_id: astro_pool_id,
            }
        ]
    );

    let (_, pool_2) = helper
        .create_and_seed_pair([
            coin(1_000_000_000000, "utest"),
            coin(1_000_000_000000, "uusd"),
        ])
        .unwrap();

    //          utest
    //            |
    // ucoin -> uusd -> astro
    helper
        .set_pool_routes(vec![PoolRoute {
            denom_in: "utest".to_string(),
            denom_out: "uusd".to_string(),
            pool_id: pool_2,
        }])
        .unwrap();

    let route = helper.query_route("utest", ASTRO_DENOM);
    assert_eq!(
        route,
        vec![
            SwapRouteResponse {
                pool_id: pool_2,
                token_out_denom: "uusd".to_string(),
            },
            SwapRouteResponse {
                token_out_denom: ASTRO_DENOM.to_string(),
                pool_id: astro_pool_id,
            }
        ]
    );

    let (_, pool_3) = helper
        .create_and_seed_pair([
            coin(1_000_000_000000, "utest"),
            coin(1_000_000_000000, "ucoin"),
        ])
        .unwrap();

    // Update route
    //  utest
    //    |
    // ucoin -> uusd -> astro
    helper
        .set_pool_routes(vec![PoolRoute {
            denom_in: "utest".to_string(),
            denom_out: "ucoin".to_string(),
            pool_id: pool_3,
        }])
        .unwrap();

    let route = helper.query_route("utest", ASTRO_DENOM);
    assert_eq!(
        route,
        vec![
            SwapRouteResponse {
                pool_id: pool_3,
                token_out_denom: "ucoin".to_string(),
            },
            SwapRouteResponse {
                pool_id: pool_1,
                token_out_denom: "uusd".to_string(),
            },
            SwapRouteResponse {
                token_out_denom: ASTRO_DENOM.to_string(),
                pool_id: astro_pool_id,
            }
        ]
    );

    let (_, pool_4) = helper
        .create_and_seed_pair([
            coin(1_000_000_000000, "utest"),
            coin(1_000_000_000000, "uatomn"),
        ])
        .unwrap();

    // Trying to set route which doesn't lead to ASTRO
    //  utest -> uatomn
    //    x
    // ucoin -> uusd -> astro
    let err = helper
        .set_pool_routes(vec![PoolRoute {
            denom_in: "utest".to_string(),
            denom_out: "uatomn".to_string(),
            pool_id: pool_4,
        }])
        .unwrap_err();
    assert_eq!(
        err.downcast::<ContractError>().unwrap(),
        ContractError::RouteNotFound {
            denom: "uatomn".to_string(),
        }
    );

    // Checking long swap path
    let mut routes = (0..=MAX_SWAPS_DEPTH)
        .into_iter()
        .tuple_windows()
        .map(|(i, j)| {
            let coin_a = format!("coin{i}");
            let coin_b = format!("coin{j}");
            let (_, pool_id) = helper
                .create_and_seed_pair([
                    coin(1_000_000_000000, &coin_a),
                    coin(1_000_000_000000, &coin_b),
                ])
                .unwrap();
            PoolRoute {
                denom_in: coin_a,
                denom_out: coin_b,
                pool_id,
            }
        })
        .collect_vec();

    let last_coin = format!("coin{MAX_SWAPS_DEPTH}");

    let (_, pool_id) = helper
        .create_and_seed_pair([
            coin(1_000_000_000000, &last_coin),
            coin(1_000_000_000000, ASTRO_DENOM),
        ])
        .unwrap();

    routes.push(PoolRoute {
        denom_in: last_coin,
        denom_out: ASTRO_DENOM.to_string(),
        pool_id,
    });

    let err = helper.set_pool_routes(routes).unwrap_err();
    assert_eq!(
        err.downcast::<ContractError>().unwrap(),
        ContractError::FailedToBuildRoute {
            denom: "coin0".to_string(),
            route_taken: "coin0 -> coin1 -> coin2 -> coin3 -> coin4 -> coin5".to_string()
        }
    );
}
