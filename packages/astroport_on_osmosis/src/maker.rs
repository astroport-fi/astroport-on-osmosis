use cosmwasm_schema::cw_serde;

#[cw_serde]
pub struct InstantiateMsg {
    /// The contract's owner, who can update config
    pub owner: String,
    /// ASTRO denom
    pub astro_denom: String,
}
