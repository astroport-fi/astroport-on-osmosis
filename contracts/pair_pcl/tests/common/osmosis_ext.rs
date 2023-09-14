use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Debug;

use anyhow::Result as AnyResult;
use astroport_on_osmosis::pair_pcl;
use cosmwasm_schema::schemars::JsonSchema;
use cosmwasm_schema::serde::de::DeserializeOwned;
use cosmwasm_std::{
    coin, coins, to_binary, Addr, Api, Binary, BlockInfo, CustomQuery, Empty, Querier, Storage,
    SubMsgResponse, WasmMsg,
};
use cw_multi_test::{
    AppResponse, BankSudo, CosmosRouter, Module, Stargate, StargateMsg, StargateQuery, WasmSudo,
};
use osmosis_std::types::osmosis::cosmwasmpool::v1beta1::{
    ContractInfoByPoolIdRequest, ContractInfoByPoolIdResponse, MsgCreateCosmWasmPool,
    MsgCreateCosmWasmPoolResponse,
};
use osmosis_std::types::osmosis::poolmanager::v1beta1::MsgSwapExactAmountIn;
use osmosis_std::types::osmosis::tokenfactory::v1beta1::{
    MsgBurn, MsgCreateDenom, MsgCreateDenomResponse, MsgMint,
};

pub struct OsmosisStargate {
    pub cw_pools: RefCell<HashMap<u64, String>>,
}

impl Module for OsmosisStargate {
    type ExecT = StargateMsg;
    type QueryT = StargateQuery;
    type SudoT = Empty;

    fn execute<ExecC, QueryC>(
        &self,
        api: &dyn Api,
        storage: &mut dyn Storage,
        router: &dyn CosmosRouter<ExecC = ExecC, QueryC = QueryC>,
        block: &BlockInfo,
        sender: Addr,
        msg: Self::ExecT,
    ) -> AnyResult<AppResponse>
    where
        ExecC: Debug + Clone + PartialEq + JsonSchema + DeserializeOwned + 'static,
        QueryC: CustomQuery + DeserializeOwned + 'static,
    {
        match msg.type_url.as_str() {
            "/osmosis.cosmwasmpool.v1beta1.MsgCreateCosmWasmPool" => {
                let cw_msg: MsgCreateCosmWasmPool = msg.value.try_into()?;
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
            "/osmosis.tokenfactory.v1beta1.MsgCreateDenom" => {
                let tf_msg: MsgCreateDenom = msg.value.try_into()?;
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
            "/osmosis.tokenfactory.v1beta1.MsgMint" => {
                let tf_msg: MsgMint = msg.value.try_into()?;
                let mint_coins = tf_msg
                    .amount
                    .expect("Empty amount in tokenfactory MsgMint!");
                let bank_sudo = BankSudo::Mint {
                    to_address: tf_msg.mint_to_address,
                    amount: coins(mint_coins.amount.parse()?, mint_coins.denom),
                };
                router.sudo(api, storage, block, bank_sudo.into())
            }
            "/osmosis.tokenfactory.v1beta1.MsgBurn" => {
                let tf_msg: MsgBurn = msg.value.try_into()?;
                let burn_coins = tf_msg
                    .amount
                    .expect("Empty amount in tokenfactory MsgBurn!");
                let bank_sudo = BankSudo::Burn {
                    from_address: tf_msg.sender,
                    amount: coins(burn_coins.amount.parse()?, burn_coins.denom),
                };
                router.sudo(api, storage, block, bank_sudo.into())
            }
            "/osmosis.poolmanager.v1beta1.MsgSwapExactAmountIn" => {
                let pm_msg: MsgSwapExactAmountIn = msg.value.try_into()?;
                let token_in = pm_msg.token_in.expect("token_in must be set!");
                let inner_contract_msg = pair_pcl::SudoMessage::SwapExactAmountIn {
                    sender: pm_msg.sender.to_string(),
                    token_in: coin(token_in.amount.parse()?, token_in.denom),
                    token_out_denom: pm_msg.routes[0].token_out_denom.clone(),
                    token_out_min_amount: pm_msg.token_out_min_amount.parse()?,
                    swap_fee: Default::default(),
                };
                let wasm_sudo_msg = WasmSudo::new(
                    &Addr::unchecked(&self.cw_pools.borrow()[&pm_msg.routes[0].pool_id]),
                    &inner_contract_msg,
                )?;
                router.sudo(api, storage, block, wasm_sudo_msg.into())
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected exec msg {msg:?} from {sender:?}",
                ))
            }
        }
    }

    fn sudo<ExecC, QueryC>(
        &self,
        _api: &dyn Api,
        _storage: &mut dyn Storage,
        _router: &dyn CosmosRouter<ExecC = ExecC, QueryC = QueryC>,
        _block: &BlockInfo,
        _msg: Self::SudoT,
    ) -> AnyResult<AppResponse>
    where
        ExecC: Debug + Clone + PartialEq + JsonSchema + DeserializeOwned + 'static,
        QueryC: CustomQuery + DeserializeOwned + 'static,
    {
        unimplemented!("sudo for Osmosis Stargate mock module is not implemented")
    }

    fn query(
        &self,
        _api: &dyn Api,
        _storage: &dyn Storage,
        _querier: &dyn Querier,
        _block: &BlockInfo,
        request: Self::QueryT,
    ) -> AnyResult<Binary> {
        match request.path.as_str() {
            "/osmosis.cosmwasmpool.v1beta1.Query/ContractInfoByPoolId" => {
                let inner: ContractInfoByPoolIdRequest = request.data.try_into()?;
                let contract_address = self.cw_pools.borrow()[&inner.pool_id].clone();
                Ok(to_binary(&ContractInfoByPoolIdResponse {
                    contract_address,
                    code_id: 0,
                })?)
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected stargate query request {request:?}",
                ))
            }
        }
    }
}

impl Stargate for OsmosisStargate {}
