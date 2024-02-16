#![cfg(not(tarpaulin_include))]

use anyhow::Result as AnyResult;
use astroport::factory::{PairConfig, PairType};
use cosmwasm_std::Addr;
use cw_multi_test::{App, AppResponse, ContractWrapper, Executor};

pub struct FactoryHelper {
    pub owner: Addr,
    pub factory: Addr,
}

impl FactoryHelper {
    pub fn init(router: &mut App, owner: &Addr) -> Self {
        let pair_contract = Box::new(
            ContractWrapper::new_with_empty(
                astroport_pcl_osmo::contract::execute,
                astroport_pcl_osmo::contract::instantiate,
                astroport_pcl_osmo::queries::query,
            )
            .with_reply_empty(astroport_pcl_osmo::contract::reply),
        );

        let pair_code_id = router.store_code(pair_contract);

        let factory_contract = Box::new(
            ContractWrapper::new_with_empty(
                astroport_factory_osmosis::contract::execute,
                astroport_factory_osmosis::contract::instantiate,
                astroport_factory_osmosis::contract::query,
            )
            .with_reply_empty(astroport_factory_osmosis::contract::reply),
        );

        let factory_code_id = router.store_code(factory_contract);

        let msg = astroport::factory::InstantiateMsg {
            pair_configs: vec![PairConfig {
                code_id: pair_code_id,
                pair_type: PairType::Custom("concentrated".to_string()),
                total_fee_bps: 0,
                maker_fee_bps: 5000,
                is_disabled: false,
                is_generator_disabled: false,
                permissioned: false,
            }],
            token_code_id: 0,
            fee_address: None,
            generator_address: None,
            owner: owner.to_string(),
            whitelist_code_id: 0,
            coin_registry_address: "coin_registry".to_string(),
        };

        let factory = router
            .instantiate_contract(
                factory_code_id,
                owner.clone(),
                &msg,
                &[],
                String::from("ASTRO"),
                None,
            )
            .unwrap();

        Self {
            owner: owner.clone(),
            factory,
        }
    }

    pub fn update_config(
        &mut self,
        router: &mut App,
        sender: &Addr,
        token_code_id: Option<u64>,
        fee_address: Option<String>,
        generator_address: Option<String>,
        whitelist_code_id: Option<u64>,
        coin_registry_address: Option<String>,
    ) -> AnyResult<AppResponse> {
        let msg = astroport::factory::ExecuteMsg::UpdateConfig {
            token_code_id,
            fee_address,
            generator_address,
            whitelist_code_id,
            coin_registry_address,
        };

        router.execute_contract(sender.clone(), self.factory.clone(), &msg, &[])
    }
}
