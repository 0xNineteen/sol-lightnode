## solana lightnode 
- `solana/` submodule includes code to spin up a local cluster with >> 1 node 
  - see that repos `readme.md` for instructions 
- `src/main.rs` has two main functions to tx verification 
  - `verify_slot` which sends a simple transfer SOL tx and requests a tx proof using NEW `get_block_headers` RPC method and verifies there is path from the tx to the bankhash 
  - `parse_block_votes` which requests the next N slot blocks from the slot tx - vote txs are then parsed out of the blocks (validators vote on bankhashes)
  - after you verify a tx is included in a specific bankhash, it checks for a supermajority vote on the transaction