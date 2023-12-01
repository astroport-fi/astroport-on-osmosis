## Astroport Passive Concentrated Liquidity (PCL) pool customized for Osmosis

This implementation of PCL is customized for Osmosis. It is based on the original
PCL [implementation](https://github.com/astroport-fi/astroport-core/tree/main/contracts/pair_concentrated).
The only differences are:
- issue TokenFactory LP tokens instead of cw20s;
- route general cosmwasm swap messages through Osmosis DEX module instead of processing them inplace;
- this implementation is not integrated with the Astroport Generator;

### Limitations
1. Only official Astroport Factory on Osmosis is able to create PCL pools.
2. Liquidity management (withdraw/provide) can be done only through PCL contract interface.
3. We pin factory address into wasm binary to prevent using Astroport PCL pools on Osmosis without
paying fees to Astroport protocol. Users are discouraged from instantiating PCL pools using 
usual Osmosis tools as such pool won't be included in Astroport routing as well as swap and withdraw endpoints will be broken forever.
4. We keep Config structure as general as possible across all PCL implementations. However, liquidity_token on osmosis is not a cw20 contract, but a native token. 
Addr::unchecked() in config.pair_info.liquidity_token is a little hack but devs shouldn't consider it as a cw20 contract on Osmosis.