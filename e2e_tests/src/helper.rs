use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::str::FromStr;

use serde::Serialize;

use anyhow::Result as AnyResult;
use astroport::asset::{Asset, AssetInfo, PairInfo};
use astroport::factory::{PairConfig, PairType};
use astroport::pair_concentrated::ConcentratedPoolParams;
use astroport::{factory, pair};
use astroport_on_osmosis::pair_pcl::ExecuteMsg;
use cosmwasm_std::{coin, coins, to_json_binary, Coin, Decimal};
use osmosis_std::types::cosmos::bank::v1beta1::QueryBalanceRequest;
use osmosis_std::types::cosmwasm::wasm::v1::MsgExecuteContractResponse;
use osmosis_std::types::osmosis::cosmwasmpool::v1beta1::{
    CosmWasmPool, PoolsRequest, PoolsResponse,
};
use osmosis_std::types::osmosis::poolmanager::v1beta1::{
    MsgSwapExactAmountIn, MsgSwapExactAmountInResponse, MsgSwapExactAmountOut,
    MsgSwapExactAmountOutResponse, SwapAmountInRoute, SwapAmountOutRoute,
};
use osmosis_std::types::osmosis::tokenfactory::v1beta1::{MsgCreateDenom, MsgMint};
use osmosis_test_tube::osmosis_std::types::osmosis::cosmwasmpool::v1beta1::UploadCosmWasmPoolCodeAndWhiteListProposal;
use osmosis_test_tube::{
    Account, Bank, GovWithAppAccess, Module, OsmosisTestApp, PoolManager, Runner,
    RunnerExecuteResult, SigningAccount, TokenFactory, Wasm,
};

fn locate_workspace_root() -> String {
    let result = Command::new("cargo")
        .args(["locate-project", "--workspace", "--message-format=plain"])
        .output()
        .expect("failed to locate workspace root");

    String::from_utf8(result.stdout)
        .unwrap()
        .trim_end()
        .strip_suffix("Cargo.toml")
        .unwrap()
        .to_string()
}

pub fn f64_to_dec<T>(val: f64) -> T
where
    T: FromStr,
    T::Err: Error,
{
    T::from_str(&val.to_string()).unwrap()
}

const FAKE_MAKER: &str = "osmo1ek9r5ulgr0cmwdwchhd87d2x4lajaucwv8p5xn";

const BUILD_CONTRACTS: &[&str] = &[
    // "astroport-pcl-osmo", // we build this contract separately to hardcode factory address
    "astroport-factory-osmosis",
    "coin-registry",
];

fn compile_wasm(project_dir: &str, contract: &str) {
    eprintln!("Building contract {contract}...");
    let output = Command::new("cargo")
        .args([
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            "--lib",
            "--locked",
            "--package",
            contract,
        ])
        .current_dir(project_dir)
        .output()
        .unwrap_or_else(|_| panic!("failed to build contract {contract}"));
    assert!(
        output.status.success(),
        "failed to build contracts: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub struct TestAppWrapper<'a> {
    pub signer: SigningAccount,
    pub wasm: Wasm<'a, OsmosisTestApp>,
    pub tf: TokenFactory<'a, OsmosisTestApp>,
    pub bank: Bank<'a, OsmosisTestApp>,
    pub pool_manager: PoolManager<'a, OsmosisTestApp>,
    pub app: &'a OsmosisTestApp,
    pub code_ids: HashMap<&'a str, u64>,
    pub coin_registry: String,
    pub factory: String,
}

impl<'a> TestAppWrapper<'a> {
    pub fn bootstrap(app: &'a OsmosisTestApp) -> AnyResult<Self> {
        // Build contracts
        let project_dir = locate_workspace_root();

        for contract in BUILD_CONTRACTS {
            compile_wasm(&project_dir, contract);
        }

        let target_dir = Path::new(&project_dir).join("target/wasm32-unknown-unknown/release");
        let native_registry_wasm = target_dir.join("coin_registry.wasm");
        let factory_wasm = target_dir.join("astroport_factory_osmosis.wasm");

        let mut helper = Self {
            signer: app.init_account(&[coin(1_500_000e6 as u128, "uosmo")])?,
            wasm: Wasm::new(app),
            tf: TokenFactory::new(app),
            bank: Bank::new(app),
            pool_manager: PoolManager::new(app),
            app,
            code_ids: HashMap::new(),
            coin_registry: "".to_string(),
            factory: "".to_string(),
        };

        println!("Storing coin registry contract...");
        let native_registry_code_id = helper.store_code(native_registry_wasm).unwrap();
        helper
            .code_ids
            .insert("coin-registry", native_registry_code_id);

        println!("Storing factory contract...");
        let factory_code_id = helper.store_code(factory_wasm).unwrap();
        helper.code_ids.insert("factory", factory_code_id);

        let coin_registry_address = helper
            .init_contract(
                "coin-registry",
                &astroport::native_coin_registry::InstantiateMsg {
                    owner: helper.signer.address(),
                },
                &[],
            )
            .unwrap();
        helper.coin_registry = coin_registry_address.clone();

        // setting 3 a little hacky but I don't know other way
        helper.code_ids.insert("pair-concentrated", 3);

        let factory_init_msg = factory::InstantiateMsg {
            pair_configs: vec![PairConfig {
                code_id: helper.code_ids["pair-concentrated"],
                pair_type: PairType::Custom("concentrated".to_string()),
                total_fee_bps: 0,
                maker_fee_bps: 0,
                is_disabled: false,
                is_generator_disabled: false,
            }],
            fee_address: Some(FAKE_MAKER.to_string()),
            generator_address: None,
            owner: helper.signer.address(),
            coin_registry_address,
            token_code_id: 0,
            whitelist_code_id: 0,
        };
        helper.factory = helper
            .init_contract("factory", &factory_init_msg, &[])
            .unwrap();

        // Pin factory address in the PCL wasm binary
        fs::write("../contracts/pair_pcl/src/factory_address", &helper.factory).unwrap();
        compile_wasm(&project_dir, "astroport-pcl-osmo");
        println!("Storing cl pool contract...");
        let cl_pool_wasm = target_dir.join("astroport_pcl_osmo.wasm");
        let gov = GovWithAppAccess::new(app);

        gov.propose_and_execute(
            UploadCosmWasmPoolCodeAndWhiteListProposal::TYPE_URL.to_string(),
            UploadCosmWasmPoolCodeAndWhiteListProposal {
                title: String::from("store test cosmwasm pool code"),
                description: String::from("test"),
                wasm_byte_code: std::fs::read(cl_pool_wasm).unwrap(),
            },
            helper.signer.address(),
            false,
            &helper.signer,
        )?;

        Ok(helper)
    }

    pub fn register_and_mint(
        &self,
        subdenom: &str,
        amount: u128,
        precision: u8,
        to: Option<String>,
    ) -> String {
        let denom = self.create_denom(subdenom);
        self.mint(coin(amount, &denom), to);

        self.wasm
            .execute(
                self.coin_registry.as_str(),
                &astroport::native_coin_registry::ExecuteMsg::Add {
                    native_coins: vec![(denom.clone(), precision)],
                },
                &[],
                &self.signer,
            )
            .unwrap();

        denom
    }

    pub fn create_denom(&self, subdenom: &str) -> String {
        self.tf
            .create_denom(
                MsgCreateDenom {
                    sender: self.signer.address(),
                    subdenom: subdenom.to_string(),
                },
                &self.signer,
            )
            .unwrap()
            .data
            .new_token_denom
    }

    pub fn mint(&self, coin: Coin, to: Option<String>) {
        let receiver = to.unwrap_or(self.signer.address());

        self.tf
            .mint(
                MsgMint {
                    sender: self.signer.address(),
                    amount: Some(coin.into()),
                    mint_to_address: receiver,
                },
                &self.signer,
            )
            .unwrap();
    }

    pub fn create_pair(
        &self,
        asset_infos: &[AssetInfo],
        init_params: ConcentratedPoolParams,
    ) -> AnyResult<(String, String)> {
        self.wasm.execute(
            &self.factory,
            &factory::ExecuteMsg::CreatePair {
                pair_type: PairType::Custom("concentrated".to_string()),
                asset_infos: asset_infos.to_vec(),
                init_params: Some(to_json_binary(&init_params).unwrap()),
            },
            &coins(1000_000000, "uosmo"),
            &self.signer,
        )?;

        let pair_info: PairInfo = self.wasm.query(
            &self.factory,
            &factory::QueryMsg::Pair {
                asset_infos: asset_infos.to_vec(),
            },
        )?;

        Ok((
            pair_info.contract_addr.to_string(),
            pair_info.liquidity_token.to_string(),
        ))
    }

    pub fn provide(
        &self,
        sender: &SigningAccount,
        pair_addr: &str,
        assets: &[Asset],
        slippage_tolerance: Option<f64>,
    ) -> RunnerExecuteResult<MsgExecuteContractResponse> {
        let mut sorted_coins = assets
            .iter()
            .map(|asset| asset.as_coin().unwrap())
            .collect::<Vec<_>>();
        sorted_coins.sort_by(|a, b| a.denom.cmp(&b.denom));
        self.wasm.execute(
            pair_addr,
            &ExecuteMsg::ProvideLiquidity {
                assets: assets.to_vec(),
                slippage_tolerance: slippage_tolerance.map(f64_to_dec),
                auto_stake: None,
                receiver: None,
            },
            &sorted_coins,
            sender,
        )
    }

    pub fn withdraw(
        &self,
        sender: &SigningAccount,
        pair_addr: &str,
        lp_tokens: Coin,
    ) -> RunnerExecuteResult<MsgExecuteContractResponse> {
        self.wasm.execute(
            pair_addr,
            &ExecuteMsg::WithdrawLiquidity { assets: vec![] },
            &[lp_tokens],
            sender,
        )
    }

    pub fn coin_balance(&self, owner: &str, denom: &str) -> u128 {
        self.bank
            .query_balance(&QueryBalanceRequest {
                address: owner.to_string(),
                denom: denom.to_string(),
            })
            .unwrap()
            .balance
            .unwrap()
            .amount
            .parse()
            .unwrap()
    }

    pub fn pair_info(&self, pair_addr: &str) -> AnyResult<PairInfo> {
        self.wasm
            .query(pair_addr, &pair::QueryMsg::Pair {})
            .map_err(Into::into)
    }

    pub fn get_pool_id_by_contract(&self, pair_addr: &str) -> u64 {
        let query_msg = PoolsRequest { pagination: None };
        let pool_infos: Vec<CosmWasmPool> = self
            .app
            .query::<_, PoolsResponse>("/osmosis.cosmwasmpool.v1beta1.Query/Pools", &query_msg)
            // .query::<_, PoolsResponse>(PoolsRequest::TYPE_URL, &query_msg)
            .unwrap()
            .pools
            .into_iter()
            .map(|data| data.try_into().unwrap())
            .collect();
        pool_infos
            .iter()
            .find_map(|pool| {
                if pool.contract_address == pair_addr {
                    Some(pool.pool_id)
                } else {
                    None
                }
            })
            .unwrap()
    }

    pub fn swap_on_dex(
        &self,
        sender: &SigningAccount,
        pool_id: u64,
        asset: &Asset,
    ) -> RunnerExecuteResult<MsgSwapExactAmountInResponse> {
        self.pool_manager.swap_exact_amount_in(
            MsgSwapExactAmountIn {
                sender: sender.address(),
                routes: vec![SwapAmountInRoute {
                    pool_id,
                    // I assume it doesn't matter in our context as pair has only 2 assets
                    token_out_denom: "uosmo".to_string(),
                }],
                token_in: Some(asset.as_coin().unwrap().into()),
                token_out_min_amount: "1".to_string(),
            },
            sender,
        )
    }

    pub fn reverse_swap_on_dex(
        &self,
        sender: &SigningAccount,
        pool_id: u64,
        token_in_denom: &str,
        token_in_max_amount: u128,
        exact_asset_out: &Asset,
    ) -> RunnerExecuteResult<MsgSwapExactAmountOutResponse> {
        self.pool_manager.swap_exact_amount_out(
            MsgSwapExactAmountOut {
                sender: sender.address(),
                routes: vec![SwapAmountOutRoute {
                    pool_id,
                    token_in_denom: token_in_denom.to_string(),
                }],
                token_in_max_amount: token_in_max_amount.to_string(),
                token_out: Some(exact_asset_out.as_coin().unwrap().into()),
            },
            sender,
        )
    }

    pub fn swap_on_pair(
        &self,
        sender: &SigningAccount,
        pair_contract: &str,
        offer_asset: &Asset,
        max_spread: Option<Decimal>,
    ) -> RunnerExecuteResult<MsgExecuteContractResponse> {
        self.swap_full_params(sender, pair_contract, offer_asset, max_spread, None)
    }

    pub fn swap_full_params(
        &self,
        sender: &SigningAccount,
        pair_contract: &str,
        offer_asset: &Asset,
        max_spread: Option<Decimal>,
        belief_price: Option<Decimal>,
    ) -> RunnerExecuteResult<MsgExecuteContractResponse> {
        let msg = ExecuteMsg::Swap {
            offer_asset: offer_asset.clone(),
            ask_asset_info: None,
            belief_price,
            max_spread,
            to: None,
        };

        self.wasm.execute(
            pair_contract,
            &msg,
            &[offer_asset.as_coin().unwrap()],
            sender,
        )
    }

    // pub fn give_me_money(&self, to: &str, coins: &[Coin]) {
    //     self.bank
    //         .send(
    //             MsgSend {
    //                 from_address: self.signer.address().to_string(),
    //                 to_address: to.to_string(),
    //                 amount: coins.into_iter().map(Into::into).collect(),
    //             },
    //             &self.signer,
    //         )
    //         .unwrap();
    // }

    pub fn store_code<P>(&self, contract_path: P) -> AnyResult<u64>
    where
        P: AsRef<Path>,
    {
        // Load the contract wasm bytecode
        let wasm_byte_code = std::fs::read(contract_path)?;

        // Store the code
        self.wasm
            .store_code(&wasm_byte_code, None, &self.signer)
            .map(|res| res.data.code_id)
            .map_err(Into::into)
    }

    pub fn init_contract<T>(&self, name: &str, msg: &T, funds: &[Coin]) -> AnyResult<String>
    where
        T: ?Sized + Serialize,
    {
        self.wasm
            .instantiate(self.code_ids[name], msg, None, None, funds, &self.signer)
            .map(|res| res.data.address)
            .map_err(Into::into)
    }
}
