use std::{str::FromStr, collections::HashMap, path::Path, fs::File, io::{Read, Write}, thread::sleep, time::Duration};

use serde::{Serialize, Deserialize};
use solana_client::{rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig};
use solana_sdk::{vote::{instruction::VoteInstruction, self}, signature::{Signature, Keypair}, transaction::{VersionedTransaction, SanitizedTransaction, Transaction}, pubkey::Pubkey, signer::Signer, system_instruction::{transfer, self}, commitment_config::CommitmentConfig};
use solana_transaction_status::{EncodedTransaction, UiTransactionEncoding, UiConfirmedBlock, EncodedConfirmedBlock, TransactionBinaryEncoding, BlockHeader, EncodedConfirmedTransactionWithStatusMeta, EntryProof, PartialEntry};
use solana_account_decoder::{self, UiAccountData, parse_stake::{parse_stake, StakeAccountType}, parse_vote::parse_vote};
use solana_entry::{entry::{Entry, EntrySlice, hash_transactions, next_hash}, poh::Poh};
use solana_sdk::hash::Hash;
use solana_sdk::hash::hashv;
use solana_merkle_tree::{MerkleTree, merkle_tree::SolidProof};

// from merkle-tree crate
const LEAF_PREFIX: &[u8] = &[0];
macro_rules! hash_leaf {
    {$d:ident} => {
        hashv(&[LEAF_PREFIX, $d])
    }
}

#[macro_export]
macro_rules! send_rpc_call {
    ($url:expr, $body:expr) => {{
        use reqwest::header::{ACCEPT, CONTENT_TYPE};
        let req_client = reqwest::Client::new();

        let res = req_client
            .post($url)
            .body($body)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .send()
            .await
            .expect("error")
            .text()
            .await
            .expect("error");
        res
    }};
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetBlockResponse {
    pub jsonrpc: String,
    pub result: UiConfirmedBlock,
    pub id: i64,
}

async fn get_block(slot: u64, endpoint: &String) -> GetBlockResponse { 
    let mut block_resp = None;
    loop { 
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getBlock",
            "params":[
                slot,
                { 
                    "encoding": "base58", // better for deserialzing
                    "maxSupportedTransactionVersion": 0,
                }
            ]
        }).to_string();
        let resp = send_rpc_call!(endpoint, request);
        let parsed_resp = serde_json::from_str::<GetBlockResponse>(&resp);
        if parsed_resp.is_err() {  // block is not available yet
            print!(".");
            std::io::stdout().flush().unwrap();
            sleep(Duration::from_millis(500));
            continue;
        }
        block_resp = Some(parsed_resp.unwrap());
        break;
    }

    block_resp.unwrap()
}

async fn parse_block_votes(target_slot: u64, slots_ahead: u64, endpoint: String) -> Option<(u64, HashMap<Hash, u64>)> {
    // let endpoint = "https://rpc.helius.xyz/?api-key=cee342ba-0773-41f7-a6e0-9ff01fff124b";
    let vote_program_id = "Vote111111111111111111111111111111111111111".to_string();
    let vote_program_id = Pubkey::from_str(&vote_program_id).unwrap();

    let client = RpcClient::new(endpoint.clone());
    let vote_accounts = client.get_vote_accounts().unwrap();
    let leader_stakes = vote_accounts.current
        .iter()
        .chain(vote_accounts.delinquent.iter())
        .map(|x| (x.node_pubkey.clone(), x.activated_stake))
        .collect::<HashMap<_, _>>();
    let total_stake = leader_stakes.values().sum::<u64>();

    let mut votes = HashMap::new();

    for i in 0..slots_ahead {
        let slot = target_slot + i;

        println!("requesting block @ slot {}", slot);
        let resp = get_block(slot, &endpoint).await;
        let block = resp.result;
    
        if block.transactions.is_none() { 
            println!("no transactions");
            return None;
        }
    
        for tx in block.transactions.unwrap().iter() {
            let tx = &tx.transaction;
            let tx = match tx { 
                EncodedTransaction::Binary(tx, enc) => {
                    assert!(*enc == TransactionBinaryEncoding::Base58);
                    let tx = bs58::decode(tx).into_vec().unwrap();
                    let tx: VersionedTransaction = bincode::deserialize(&tx[..]).unwrap();
                    tx
                }
                _ => panic!("ahh")
            };
    
            let msg = tx.message;
            if !msg.static_account_keys().contains(&vote_program_id) { 
                // println!("tx doesnt include vote program ...");
                continue;
            }
    
            let ix = msg.instructions().get(0).unwrap();
            let data = &ix.data;
            let vote_ix: VoteInstruction = bincode::deserialize(&data[..]).unwrap();
            let bank_hash = match &vote_ix { 
                VoteInstruction::Vote(v) => Some(v.hash),   
                VoteInstruction::CompactUpdateVoteState(v) => Some(v.hash),
                _ => None
            };
            if bank_hash.is_none() { continue; }
            let bank_hash = bank_hash.unwrap();

            // let slot_vote = vote_ix.last_voted_slot().unwrap_or_default();
            // println!("{:?}", vote_ix);
            // println!("voted for slot {:?} with bank_hash {:?}", slot_vote, bank_hash);
            // println!("{:?} {:?}", node_pubkey, stake_amount);
    
            // verify the signature
            let msg_bytes = msg.serialize();
            let sig_verifies = tx.signatures
                .iter()
                .zip(msg.static_account_keys().iter())
                .map(|(signature, pubkey)| signature.verify(pubkey.as_ref(), &msg_bytes[..]))
                .all(|x| x);

            if sig_verifies { 
                let node_pubkey = msg.static_account_keys().get(0).unwrap().to_string();
                let stake_amount = leader_stakes.get(&node_pubkey).unwrap();

                let entry = votes.entry(bank_hash).or_insert(0);
                *entry += stake_amount; 
            }
        }
    }

    Some((total_stake, votes))
}


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetBlockHeadersResponse {
    pub jsonrpc: String,
    pub result: Vec<u8>,
    pub id: i64,
}

async fn get_block_headers(slot: u64, signature: Signature, endpoint: String) -> GetBlockHeadersResponse { 
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getBlockHeaders",
        "params":[
            slot, 
            signature.as_ref(),
        ]
    }).to_string();
    let resp = send_rpc_call!(endpoint, request);
    let parsed_resp = serde_json::from_str::<GetBlockHeadersResponse>(&resp);
    if parsed_resp.is_err() { 
        println!("ERR: {:?}", resp);
    }
    let parsed_resp = parsed_resp.unwrap();

    parsed_resp
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTransactionResponse {
    pub jsonrpc: String,
    pub result: EncodedConfirmedTransactionWithStatusMeta,
    pub id: i64,
}

async fn get_tx(signtaure: Signature, endpoint: String) -> GetTransactionResponse { 
    let mut tx_resp = None;

    while tx_resp.is_none() { 
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTransaction",
            "params": [signtaure.to_string(),
            {
                "commitment": "confirmed",
                "encoding": "json",
            }]
        }).to_string();
        let resp = send_rpc_call!(&endpoint, request);
        let parsed_resp = serde_json::from_str::<GetTransactionResponse>(&resp);
        if parsed_resp.is_err() {  // tx is not available yet
            print!(".");
            sleep(Duration::from_millis(500));
            continue;
        }

        tx_resp = Some(parsed_resp.unwrap());
    }
    print!("\n");

    tx_resp.unwrap()
}

pub fn next_hash_with_tx_hash(
    start_hash: &Hash,
    num_hashes: u64,
    transaction_hash: Option<Hash>,
) -> Hash {
    if num_hashes == 0 && transaction_hash.is_none() {
        return *start_hash;
    }

    let mut poh = Poh::new(*start_hash, None);
    poh.hash(num_hashes.saturating_sub(1));
    if transaction_hash.is_none() {
        poh.tick().unwrap().hash
    } else {
        poh.record(transaction_hash.unwrap()).unwrap().hash
    }
}

pub fn read_keypair_file<F: AsRef<Path>>(path: F) -> Keypair {
    let mut file = File::open(path.as_ref()).unwrap();
    let mut buf = String::new();
    file.read_to_string(&mut buf).unwrap();
    let bytes: Vec<u8> = serde_json::from_str(&buf).unwrap();
    Keypair::from_bytes(&bytes[..]).unwrap()
}

pub async fn verify_slot() { 
    let endpoint = "http://127.0.0.1:8002";
    let client = RpcClient::new(endpoint);

    let path = "./solana/validator/ledger/node1/validator_id.json";
    let keypair = read_keypair_file(path);
    let balance = client.get_balance(&keypair.pubkey()).unwrap();
    println!("keypair balance: {:?}", balance);

    let path = "./solana/validator/ledger/rando_keys/1.json";
    let random = read_keypair_file(path);
    let mut balance = 0;
    // sometimes takes a while to get the balance from airdrop
    while balance == 0 { 
        balance = client.get_balance(&random.pubkey()).unwrap();
        sleep(Duration::from_millis(500));
    }
    println!("random keypair balance: {:?}", balance);

    // simple tx to verify
    let ix = system_instruction::transfer(
        &keypair.pubkey(), 
        &random.pubkey(), 
        100
    );
    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get latest blockhash.");
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&keypair.pubkey()), &[&keypair], recent_blockhash);
    let tx_sig = client.send_transaction(&tx).unwrap();
    let tx_info = get_tx(tx_sig, endpoint.to_string()).await; 
    let slot = tx_info.result.slot;
    println!("verifying slot {:?}", slot);

    // get headers
    let block_headers = get_block_headers(slot, tx_sig, endpoint.to_string()).await.result;
    let block_headers: BlockHeader = bincode::deserialize(&block_headers).unwrap();
    let entries = block_headers.entries; 

    // find and verify tx signature in entry
    let mut tx_found = false;
    for entry in entries.iter() {
        match entry { 
            EntryProof::MerkleEntry(x) => {
                println!("{:?}", x);

                // verify merkle proof here 
                let leaf = tx_sig.as_ref();
                let candidate = hash_leaf!(leaf);
                // when len == 1 this does nothing
                let verified = x.proof.verify(candidate);
                if !verified { 
                    println!("tx signature not verified!");
                    return;
                }

                tx_found = true;
                println!("tx signature verified!");
                break;
            }, 
            _ => {}
        };
    }
    if !tx_found { 
        println!("tx signature not found in entries...");
        return;
    }

    // verify the entries are valid PoH ticks / path 
    let start_blockhash = block_headers.start_blockhash;
    let genesis = [EntryProof::PartialEntry(PartialEntry {
        num_hashes: 0,
        hash: start_blockhash,
        transaction_hash: None
    })];
    let mut entry_pairs = genesis.iter().chain(entries.iter()).zip(entries.iter());
    let verified = entry_pairs.all(|(x0, x1)| {
        let start_hash = x0.hash();
        let r = match x1 { 
            EntryProof::PartialEntry(x) => {
                next_hash_with_tx_hash(&start_hash, x.num_hashes, x.transaction_hash) == x.hash
            }, 
            EntryProof::MerkleEntry(x) => {
                let tx_hash = if let Some(hash) = x.proof.root() {
                    hash
                } else { 
                    let tx_sig_ref = tx_sig.as_ref();
                    hash_leaf!(tx_sig_ref)
                };
                next_hash_with_tx_hash(&start_hash, x.num_hashes, Some(tx_hash)) == x.hash
            }
        };
        r
    });
    if !verified { 
        println!("entry verification failed ...");
        return;
    }
    println!("entry verification passed!");

    // recompute the bank hash 
    let last_blockhash = entries.last().unwrap().hash();
    let bankhash = hashv(&[
        block_headers.parent_hash.as_ref(),
        block_headers.accounts_delta_hash.as_ref(),
        block_headers.signature_count_buf.as_ref(), 
        last_blockhash.as_ref()
    ]);
    println!("bank hash: {:?}", bankhash);

    println!("parsing votes from block ...");
    let vote_result = parse_block_votes(slot, 5, endpoint.to_string()).await;
    if vote_result.is_none() { 
        println!("vote verification failed ...");
    }
    let (total_stake, votes) = vote_result.unwrap();
    let bankhash_vote_stakes = votes.get(&bankhash).unwrap();
    println!("bankhash vote stakes: {:?} total stakes: {total_stake:?}", bankhash_vote_stakes);

    // bankhash_vote_stakes >= 2/3 * total_stake
    // 3 * bankhash_vote_stakes >= 2 * total_stake
    let is_supermajority = 3 * bankhash_vote_stakes >= 2 * total_stake;
    println!("bankhash has supermajority of votes: {:?}", is_supermajority);
}

#[tokio::main]
async fn main() {
    verify_slot().await;
}
