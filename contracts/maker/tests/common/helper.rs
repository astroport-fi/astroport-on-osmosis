#![allow(dead_code)]

use std::error::Error;
use std::fmt::Display;
use std::str::FromStr;

use anyhow::Result as AnyResult;
use astroport::asset::{Asset, AssetInfo, PairInfo};
use astroport::factory::{PairConfig, PairType};
use astroport::pair_concentrated::ConcentratedPoolParams;
use astroport::{factory, pair};
use cosmwasm_std::testing::MockApi;
use cosmwasm_std::{
    coins, to_json_binary, Addr, Api, Binary, Coin, Decimal, Deps, DepsMut, Empty, Env, GovMsg,
    IbcMsg, IbcQuery, MemoryStorage, MessageInfo, Response, StdResult, Storage,
};
use cw_multi_test::{
    AddressGenerator, App, AppResponse, BankKeeper, BankSudo, BasicAppBuilder, Contract,
    ContractWrapper, DistributionKeeper, Executor, FailingModule, StakeKeeper, WasmKeeper,
};
use cw_storage_plus::Item;
use derivative::Derivative;
use itertools::Itertools;

use astroport_on_osmosis::maker;
use astroport_on_osmosis::maker::{CoinWithLimit, PoolRoute, SwapRouteResponse};
use astroport_on_osmosis::pair_pcl::ExecuteMsg;
use astroport_pcl_osmo::contract::{execute, instantiate, reply};
use astroport_pcl_osmo::queries::query;
use astroport_pcl_osmo::state::POOL_ID;
use astroport_pcl_osmo::sudo::sudo;

use crate::common::osmosis_ext::OsmosisStargate;

pub type OsmoApp = App<
    BankKeeper,
    MockApi,
    MemoryStorage,
    FailingModule<Empty, Empty, Empty>,
    WasmKeeper<Empty, Empty>,
    StakeKeeper,
    DistributionKeeper,
    FailingModule<IbcMsg, IbcQuery, Empty>,
    FailingModule<GovMsg, Empty, Empty>,
    OsmosisStargate,
>;

fn pair_contract() -> Box<dyn Contract<Empty>> {
    Box::new(
        ContractWrapper::new_with_empty(execute, instantiate, query)
            .with_reply_empty(reply)
            .with_sudo_empty(sudo),
    )
}

fn coin_registry_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new_with_empty(
        astroport_native_coin_registry::contract::execute,
        astroport_native_coin_registry::contract::instantiate,
        astroport_native_coin_registry::contract::query,
    ))
}
fn factory_contract() -> Box<dyn Contract<Empty>> {
    Box::new(
        ContractWrapper::new_with_empty(
            astroport_factory::contract::execute,
            astroport_factory::contract::instantiate,
            astroport_factory::contract::query,
        )
        .with_reply_empty(astroport_factory::contract::reply),
    )
}

fn maker_contract() -> Box<dyn Contract<Empty>> {
    Box::new(
        ContractWrapper::new_with_empty(
            astroport_maker_osmosis::execute::execute,
            astroport_maker_osmosis::instantiate::instantiate,
            astroport_maker_osmosis::query::query,
        )
        .with_reply_empty(astroport_maker_osmosis::reply::reply),
    )
}

fn mock_satellite_contract() -> Box<dyn Contract<Empty>> {
    let instantiate = |_: DepsMut, _: Env, _: MessageInfo, _: Empty| -> StdResult<Response> {
        Ok(Default::default())
    };
    let execute = |_: DepsMut,
                   _: Env,
                   _: MessageInfo,
                   _: astro_satellite_package::ExecuteMsg|
     -> StdResult<Response> { Ok(Default::default()) };
    let empty_query = |_: Deps, _: Env, _: Empty| -> StdResult<Binary> { unimplemented!() };

    Box::new(ContractWrapper::new_with_empty(
        execute,
        instantiate,
        empty_query,
    ))
}

pub fn osmo_create_pair_fee() -> Vec<Coin> {
    coins(1000_000000, "uosmo")
}

fn common_pcl_params(price_scale: Decimal) -> ConcentratedPoolParams {
    ConcentratedPoolParams {
        amp: f64_to_dec(10f64),
        gamma: f64_to_dec(0.000145),
        mid_fee: f64_to_dec(0.0026),
        out_fee: f64_to_dec(0.0045),
        fee_gamma: f64_to_dec(0.00023),
        repeg_profit_threshold: f64_to_dec(0.000002),
        min_price_scale_delta: f64_to_dec(0.000146),
        price_scale,
        ma_half_time: 600,
        track_asset_balances: None,
        fee_share: None,
    }
}

const FACTORY_ADDRESS: &str = include_str!("../../../pair_concentrated/src/factory_address");

#[derive(Default)]
struct HackyAddressGenerator<'a> {
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> HackyAddressGenerator<'a> {
    pub const FACTORY_MARKER: Item<'a, ()> = Item::new("factory_marker");
}

impl<'a> AddressGenerator for HackyAddressGenerator<'a> {
    fn contract_address(
        &self,
        _api: &dyn Api,
        storage: &mut dyn Storage,
        _code_id: u64,
        instance_id: u64,
    ) -> AnyResult<Addr> {
        if Self::FACTORY_MARKER.may_load(storage).unwrap().is_some() {
            Self::FACTORY_MARKER.remove(storage);
            Ok(Addr::unchecked(FACTORY_ADDRESS))
        } else {
            Ok(Addr::unchecked(format!("contract{instance_id}")))
        }
    }
}

pub const ASTRO_DENOM: &str = "astro";

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Helper {
    #[derivative(Debug = "ignore")]
    pub app: OsmoApp,
    pub owner: Addr,
    pub coin_registry: Addr,
    pub factory: Addr,
    pub maker: Addr,
    pub satellite: Addr,
}

impl Helper {
    pub fn new(owner: &Addr) -> AnyResult<Self> {
        let wasm_keeper =
            WasmKeeper::new().with_address_generator(HackyAddressGenerator::default());
        let mut app = BasicAppBuilder::new()
            .with_stargate(OsmosisStargate::default())
            .with_wasm(wasm_keeper)
            .build(|router, _, storage| {
                router
                    .bank
                    .init_balance(storage, owner, coins(1_000_000_000_000, "uosmo"))
                    .unwrap()
            });

        let pair_code_id = app.store_code(pair_contract());
        let factory_code_id = app.store_code(factory_contract());
        let satellite_code_id = app.store_code(mock_satellite_contract());

        let satellite = app.instantiate_contract(
            satellite_code_id,
            owner.clone(),
            &Empty {},
            &[],
            "Satellite",
            None,
        )?;

        let maker_code_id = app.store_code(maker_contract());
        let maker = app.instantiate_contract(
            maker_code_id,
            owner.clone(),
            &maker::InstantiateMsg {
                owner: owner.to_string(),
                astro_denom: ASTRO_DENOM.to_string(),
                satellite: satellite.to_string(),
                max_spread: Decimal::percent(10),
                collect_cooldown: None,
            },
            &[],
            "Maker",
            None,
        )?;

        let coin_registry_id = app.store_code(coin_registry_contract());

        let coin_registry = app
            .instantiate_contract(
                coin_registry_id,
                owner.clone(),
                &astroport::native_coin_registry::InstantiateMsg {
                    owner: owner.to_string(),
                },
                &[],
                "Coin registry",
                None,
            )
            .unwrap();

        let init_msg = factory::InstantiateMsg {
            fee_address: Some(maker.to_string()),
            pair_configs: vec![PairConfig {
                code_id: pair_code_id,
                maker_fee_bps: 2500,
                total_fee_bps: 0u16, // Concentrated pair does not use this field,
                pair_type: PairType::Custom("concentrated".to_string()),
                is_disabled: false,
                is_generator_disabled: false,
            }],
            token_code_id: 0,
            generator_address: None,
            owner: owner.to_string(),
            whitelist_code_id: 0,
            coin_registry_address: coin_registry.to_string(),
        };

        // Set marker in storage that the next contract is factory. We need this to have exact FACTORY_ADDRESS constant
        // which is hardcoded in the PCL code.
        app.init_modules(|_, _, storage| HackyAddressGenerator::FACTORY_MARKER.save(storage, &()))
            .unwrap();
        let factory = app.instantiate_contract(
            factory_code_id,
            owner.clone(),
            &init_msg,
            &[],
            "Factory",
            None,
        )?;

        Ok(Self {
            app,
            owner: owner.clone(),
            coin_registry,
            factory,
            maker,
            satellite,
        })
    }

    pub fn create_and_seed_pair(
        &mut self,
        initial_liquidity: [Coin; 2],
    ) -> AnyResult<(PairInfo, u64)> {
        let native_coins = initial_liquidity
            .iter()
            .cloned()
            .map(|x| (x.denom.clone(), 6))
            .collect::<Vec<_>>();
        let asset_infos = native_coins
            .iter()
            .map(|(denom, _)| AssetInfo::native(denom))
            .collect_vec();

        self.app
            .execute_contract(
                self.owner.clone(),
                self.coin_registry.clone(),
                &astroport::native_coin_registry::ExecuteMsg::Add { native_coins },
                &[],
            )
            .unwrap();

        let price_scale =
            Decimal::from_ratio(initial_liquidity[0].amount, initial_liquidity[1].amount);
        let owner = self.owner.clone();

        let pair_info = self
            .app
            .execute_contract(
                owner.clone(),
                self.factory.clone(),
                &factory::ExecuteMsg::CreatePair {
                    pair_type: PairType::Custom("concentrated".to_string()),
                    asset_infos: asset_infos.clone(),
                    init_params: Some(to_json_binary(&common_pcl_params(price_scale)).unwrap()),
                },
                &osmo_create_pair_fee(),
            )
            .map(|_| self.query_pair_info(&asset_infos))?;

        let provide_assets = [
            Asset::native(&initial_liquidity[0].denom, initial_liquidity[0].amount),
            Asset::native(&initial_liquidity[1].denom, initial_liquidity[1].amount),
        ];

        self.give_me_money(&provide_assets, &owner);
        self.provide(&pair_info.contract_addr, &owner, &provide_assets)
            .unwrap();

        let pool_id = POOL_ID
            .query(&self.app.wrap(), pair_info.contract_addr.clone())
            .unwrap();

        Ok((pair_info, pool_id))
    }

    pub fn set_pool_routes(&mut self, pool_routes: Vec<PoolRoute>) -> AnyResult<AppResponse> {
        self.app.execute_contract(
            self.owner.clone(),
            self.maker.clone(),
            &maker::ExecuteMsg::SetPoolRoutes(pool_routes),
            &[],
        )
    }

    pub fn collect(&mut self, assets: Vec<CoinWithLimit>) -> AnyResult<AppResponse> {
        self.app.execute_contract(
            self.owner.clone(),
            self.maker.clone(),
            &maker::ExecuteMsg::Collect { assets },
            &[],
        )
    }

    pub fn query_route(&self, denom_in: &str, denom_out: &str) -> Vec<SwapRouteResponse> {
        self.app
            .wrap()
            .query_wasm_smart(
                &self.maker,
                &maker::QueryMsg::Route {
                    denom_in: denom_in.to_string(),
                    denom_out: denom_out.to_string(),
                },
            )
            .unwrap()
    }

    pub fn query_pair_info(&self, asset_infos: &[AssetInfo]) -> PairInfo {
        self.app
            .wrap()
            .query_wasm_smart(
                &self.factory,
                &factory::QueryMsg::Pair {
                    asset_infos: asset_infos.to_vec(),
                },
            )
            .unwrap()
    }

    pub fn provide(
        &mut self,
        pair: &Addr,
        sender: &Addr,
        assets: &[Asset],
    ) -> AnyResult<AppResponse> {
        let funds = assets
            .iter()
            .map(|x| x.as_coin().unwrap())
            .collect::<Vec<_>>();

        let msg = ExecuteMsg::ProvideLiquidity {
            assets: assets.to_vec(),
            slippage_tolerance: Some(f64_to_dec(0.5)),
            auto_stake: None,
            receiver: None,
        };

        self.app
            .execute_contract(sender.clone(), pair.clone(), &msg, &funds)
    }

    pub fn swap(
        &mut self,
        pair: &Addr,
        sender: &Addr,
        offer_asset: &Asset,
        max_spread: Option<Decimal>,
    ) -> AnyResult<AppResponse> {
        match &offer_asset.info {
            AssetInfo::NativeToken { .. } => self.app.execute_contract(
                sender.clone(),
                pair.clone(),
                &pair::ExecuteMsg::Swap {
                    offer_asset: offer_asset.clone(),
                    ask_asset_info: None,
                    belief_price: None,
                    max_spread,
                    to: None,
                },
                &[offer_asset.as_coin().unwrap()],
            ),
            AssetInfo::Token { .. } => unimplemented!("cw20 not implemented"),
        }
    }

    pub fn native_balance(&self, denom: &str, user: &Addr) -> u128 {
        self.app
            .wrap()
            .query_balance(user, denom)
            .unwrap()
            .amount
            .u128()
    }

    pub fn give_me_money(&mut self, assets: &[Asset], recipient: &Addr) {
        let funds = assets
            .iter()
            .map(|x| x.as_coin().unwrap())
            .collect::<Vec<_>>();

        self.app
            .sudo(
                BankSudo::Mint {
                    to_address: recipient.to_string(),
                    amount: funds,
                }
                .into(),
            )
            .unwrap();
    }
}

pub trait AppExtension {
    fn next_block(&mut self, time: u64);
}

impl AppExtension for OsmoApp {
    fn next_block(&mut self, time: u64) {
        self.update_block(|block| {
            block.time = block.time.plus_seconds(time);
            block.height += 1
        });
    }
}

pub fn f64_to_dec<T>(val: f64) -> T
where
    T: FromStr,
    T::Err: Error,
{
    T::from_str(&val.to_string()).unwrap()
}

pub fn dec_to_f64(val: impl Display) -> f64 {
    f64::from_str(&val.to_string()).unwrap()
}
