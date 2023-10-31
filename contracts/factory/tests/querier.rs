use std::collections::HashMap;
use std::marker::PhantomData;

use astroport::asset::PairInfo;
use astroport::pair;
use cosmwasm_std::testing::{BankQuerier, MockApi, MockStorage};
use cosmwasm_std::{
    from_json, to_json_binary, ContractResult, Empty, OwnedDeps, Querier, QuerierResult,
    QueryRequest, SystemError, SystemResult, WasmQuery,
};
use osmosis_std::types::osmosis::cosmwasmpool::v1beta1::{
    ContractInfoByPoolIdRequest, ContractInfoByPoolIdResponse,
};

pub fn mock_dependencies_with_custom_querier<Q: Querier>(
    querier: Q,
) -> OwnedDeps<MockStorage, MockApi, Q, Empty> {
    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default(),
        querier,
        custom_query_type: PhantomData,
    }
}

pub struct MockedStargateQuerier<'a> {
    bank: BankQuerier,
    pool_id_to_contract: HashMap<u64, &'a str>,
    pair_infos: HashMap<&'a str, PairInfo>,
}

impl<'a> MockedStargateQuerier<'a> {
    pub fn new() -> Self {
        Self {
            bank: BankQuerier::new(&[]),
            pool_id_to_contract: Default::default(),
            pair_infos: Default::default(),
        }
    }

    pub fn add_contract(&mut self, contract: &'a str, pair_info: PairInfo, pool_id: u64) {
        self.pool_id_to_contract.insert(pool_id, contract);
        self.pair_infos.insert(contract, pair_info);
    }
}

impl<'a> MockedStargateQuerier<'a> {
    pub fn handle_query(&self, request: &QueryRequest<Empty>) -> QuerierResult {
        match request {
            QueryRequest::Bank(bank_query) => self.bank.query(bank_query),
            QueryRequest::Wasm(WasmQuery::Smart { contract_addr, msg }) => {
                match from_json(msg).unwrap() {
                    pair::QueryMsg::Pair {} => {}
                    _ => unimplemented!("Unsupported wasm smart query"),
                }
                let pair_info = self.pair_infos.get(contract_addr.as_str()).unwrap();
                SystemResult::Ok(ContractResult::Ok(to_json_binary(pair_info).unwrap()))
            }
            QueryRequest::Stargate { path, data }
                if path == "/osmosis.cosmwasmpool.v1beta1.Query/ContractInfoByPoolId" =>
            {
                let msg = ContractInfoByPoolIdRequest::try_from(data.clone()).unwrap();
                let contract = self.pool_id_to_contract.get(&msg.pool_id).unwrap();
                SystemResult::Ok(ContractResult::Ok(
                    to_json_binary(&ContractInfoByPoolIdResponse {
                        contract_address: contract.to_string(),
                        code_id: 0, // not used in tests
                    })
                    .unwrap(),
                ))
            }
            _ => SystemResult::Err(SystemError::InvalidRequest {
                error: "Unsupported query request".to_string(),
                request: to_json_binary(&request).unwrap(),
            }),
        }
    }
}

impl Querier for MockedStargateQuerier<'_> {
    fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
        let request: QueryRequest<Empty> = match from_json(bin_request) {
            Ok(v) => v,
            Err(e) => {
                return SystemResult::Err(SystemError::InvalidRequest {
                    error: format!("Parsing query request: {e}"),
                    request: bin_request.into(),
                })
            }
        };
        self.handle_query(&request)
    }
}
