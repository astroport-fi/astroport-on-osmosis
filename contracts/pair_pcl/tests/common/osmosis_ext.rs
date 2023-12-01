use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Debug;

use anyhow::Result as AnyResult;
use cosmwasm_schema::schemars::JsonSchema;
use cosmwasm_schema::serde::de::DeserializeOwned;
use cosmwasm_std::{
    coin, coins, from_json, to_json_binary, Addr, Api, BankMsg, Binary, BlockInfo, CustomQuery,
    Querier, QueryRequest, Storage, SubMsgResponse, WasmMsg, WasmQuery,
};
use cw_multi_test::{AppResponse, BankSudo, CosmosRouter, Stargate, WasmSudo};
use osmosis_std::types::osmosis::cosmwasmpool::v1beta1::{
    ContractInfoByPoolIdRequest, ContractInfoByPoolIdResponse, MsgCreateCosmWasmPool,
    MsgCreateCosmWasmPoolResponse,
};
use osmosis_std::types::osmosis::poolmanager;
use osmosis_std::types::osmosis::poolmanager::v1beta1::{
    MsgSwapExactAmountIn, MsgSwapExactAmountOut,
};
use osmosis_std::types::osmosis::tokenfactory::v1beta1::{
    MsgBurn, MsgCreateDenom, MsgCreateDenomResponse, MsgMint,
};

use astroport_on_osmosis::pair_pcl;
use astroport_on_osmosis::pair_pcl::{GetSwapFeeResponse, QueryMsg};

#[derive(Default)]
pub struct OsmosisStargate {
    pub cw_pools: RefCell<HashMap<u64, String>>,
}

impl Stargate for OsmosisStargate {
    fn execute<ExecC, QueryC>(
        &self,
        api: &dyn Api,
        storage: &mut dyn Storage,
        router: &dyn CosmosRouter<ExecC = ExecC, QueryC = QueryC>,
        block: &BlockInfo,
        sender: Addr,
        type_url: String,
        value: Binary,
    ) -> AnyResult<AppResponse>
    where
        ExecC: Debug + Clone + PartialEq + JsonSchema + DeserializeOwned + 'static,
        QueryC: CustomQuery + DeserializeOwned + 'static,
    {
        match type_url.as_str() {
            MsgCreateCosmWasmPool::TYPE_URL => {
                let cw_msg: MsgCreateCosmWasmPool = value.try_into()?;
                let init_wasm = WasmMsg::Instantiate {
                    admin: None,
                    code_id: cw_msg.code_id,
                    msg: cw_msg.instantiate_msg.into(),
                    funds: vec![],
                    label: "CW pool: Astroport PCL".to_string(),
                };
                let resp = router.execute(api, storage, block, sender, init_wasm.into())?;
                let contract_addr = resp
                    .events
                    .iter()
                    .find_map(|e| {
                        if e.ty == "instantiate" {
                            Some(
                                e.attributes
                                    .iter()
                                    .find(|a| a.key == "_contract_address")
                                    .unwrap()
                                    .value
                                    .clone(),
                            )
                        } else {
                            None
                        }
                    })
                    .unwrap();

                let mut cw_pools = self.cw_pools.borrow_mut();
                let next_pool_id = cw_pools.len() as u64 + 1;
                cw_pools.insert(next_pool_id, contract_addr);

                let submsg_response = SubMsgResponse {
                    events: vec![],
                    data: Some(
                        MsgCreateCosmWasmPoolResponse {
                            pool_id: next_pool_id,
                        }
                        .into(),
                    ),
                };
                Ok(submsg_response.into())
            }
            MsgCreateDenom::TYPE_URL => {
                let tf_msg: MsgCreateDenom = value.try_into()?;
                let submsg_response = SubMsgResponse {
                    events: vec![],
                    data: Some(
                        MsgCreateDenomResponse {
                            new_token_denom: format!(
                                "factory/{}/{}",
                                tf_msg.sender, tf_msg.subdenom
                            ),
                        }
                        .into(),
                    ),
                };
                Ok(submsg_response.into())
            }
            MsgMint::TYPE_URL => {
                let tf_msg: MsgMint = value.try_into()?;
                let mint_coins = tf_msg
                    .amount
                    .expect("Empty amount in tokenfactory MsgMint!");
                let bank_sudo = BankSudo::Mint {
                    to_address: tf_msg.mint_to_address,
                    amount: coins(mint_coins.amount.parse()?, mint_coins.denom),
                };
                router.sudo(api, storage, block, bank_sudo.into())
            }
            MsgBurn::TYPE_URL => {
                let tf_msg: MsgBurn = value.try_into()?;
                let burn_coins = tf_msg
                    .amount
                    .expect("Empty amount in tokenfactory MsgBurn!");
                let burn_msg = BankMsg::Burn {
                    amount: coins(burn_coins.amount.parse()?, burn_coins.denom),
                };
                router.execute(
                    api,
                    storage,
                    block,
                    Addr::unchecked(tf_msg.sender),
                    burn_msg.into(),
                )
            }
            MsgSwapExactAmountIn::TYPE_URL => {
                let pm_msg: MsgSwapExactAmountIn = value.try_into()?;
                let token_in = pm_msg.token_in.expect("token_in must be set!");

                let contract_addr =
                    Addr::unchecked(&self.cw_pools.borrow()[&pm_msg.routes[0].pool_id]);

                // Osmosis always performs this query before calling a contract.
                let res = router
                    .query(
                        api,
                        storage,
                        block,
                        QueryRequest::Wasm(WasmQuery::Smart {
                            contract_addr: contract_addr.to_string(),
                            msg: to_json_binary(&QueryMsg::GetSwapFee {}).unwrap(),
                        }),
                    )
                    .unwrap();

                let inner_contract_msg = pair_pcl::SudoMessage::SwapExactAmountIn {
                    sender: pm_msg.sender.to_string(),
                    token_in: coin(token_in.amount.parse()?, token_in.denom),
                    token_out_denom: pm_msg.routes[0].token_out_denom.clone(),
                    token_out_min_amount: pm_msg.token_out_min_amount.parse()?,
                    swap_fee: from_json::<GetSwapFeeResponse>(&res)?.swap_fee,
                };

                let wasm_sudo_msg = WasmSudo::new(&contract_addr, &inner_contract_msg)?;
                router.sudo(api, storage, block, wasm_sudo_msg.into())
            }
            MsgSwapExactAmountOut::TYPE_URL => {
                let pm_msg: MsgSwapExactAmountOut = value.try_into()?;
                let token_out = pm_msg.token_out.expect("token_out must be set!");

                let contract_addr =
                    Addr::unchecked(&self.cw_pools.borrow()[&pm_msg.routes[0].pool_id]);

                // Osmosis always performs this query before calling a contract.
                let res = router
                    .query(
                        api,
                        storage,
                        block,
                        QueryRequest::Wasm(WasmQuery::Smart {
                            contract_addr: contract_addr.to_string(),
                            msg: to_json_binary(&QueryMsg::GetSwapFee {}).unwrap(),
                        }),
                    )
                    .unwrap();

                let inner_contract_msg = pair_pcl::SudoMessage::SwapExactAmountOut {
                    sender: pm_msg.sender.clone(),
                    token_in_denom: pm_msg.routes[0].token_in_denom.clone(),
                    token_in_max_amount: pm_msg.token_in_max_amount.parse()?,
                    token_out: coin(token_out.amount.parse()?, token_out.denom),
                    swap_fee: from_json::<GetSwapFeeResponse>(&res)?.swap_fee,
                };

                router.execute(
                    api,
                    storage,
                    block,
                    Addr::unchecked(pm_msg.sender),
                    BankMsg::Send {
                        to_address: contract_addr.to_string(),
                        amount: coins(
                            pm_msg.token_in_max_amount.parse()?,
                            pm_msg.routes[0].token_in_denom.clone(),
                        ),
                    }
                    .into(),
                )?;

                let wasm_sudo_msg = WasmSudo::new(&contract_addr, &inner_contract_msg)?;
                router.sudo(api, storage, block, wasm_sudo_msg.into())
            }
            _ => Err(anyhow::anyhow!(
                "Unexpected exec msg {type_url} from {sender:?}",
            )),
        }
    }

    fn query(
        &self,
        _api: &dyn Api,
        _storage: &dyn Storage,
        _querier: &dyn Querier,
        _block: &BlockInfo,
        path: String,
        data: Binary,
    ) -> AnyResult<Binary> {
        match path.as_str() {
            "/osmosis.cosmwasmpool.v1beta1.Query/ContractInfoByPoolId" => {
                let inner: ContractInfoByPoolIdRequest = data.try_into()?;
                let contract_address = self.cw_pools.borrow()[&inner.pool_id].clone();
                Ok(to_json_binary(&ContractInfoByPoolIdResponse {
                    contract_address,
                    code_id: 0,
                })?)
            }
            "/osmosis.poolmanager.v1beta1.Query/Params" => {
                Ok(to_json_binary(&poolmanager::v1beta1::ParamsResponse {
                    params: Some(poolmanager::v1beta1::Params {
                        pool_creation_fee: vec![coin(1000_000000, "uosmo").into()],
                        taker_fee_params: None,
                        authorized_quote_denoms: vec![],
                    }),
                })?)
            }
            _ => Err(anyhow::anyhow!("Unexpected stargate query request {path}",)),
        }
    }
}
