use std::{str::FromStr, collections::HashMap};

use serde::{Serialize, Deserialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{vote::{instruction::VoteInstruction, self}, signature::Signature, transaction::{VersionedTransaction, SanitizedTransaction}, pubkey::Pubkey};
use solana_transaction_status::{EncodedTransaction, UiTransactionEncoding, UiConfirmedBlock, EncodedConfirmedBlock, TransactionBinaryEncoding, BlockHeader};
use solana_account_decoder::{self, UiAccountData, parse_stake::{parse_stake, StakeAccountType}, parse_vote::parse_vote};
use solana_entry::entry::{Entry, EntrySlice};
use solana_sdk::hash::Hash;

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


async fn get_block(slot: u64, endpoint: String) -> GetBlockResponse { 
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
    let resp = serde_json::from_str::<GetBlockResponse>(&resp).unwrap();
    resp
}

async fn parse_block_votes() { 
    // let endpoint = "http://127.0.0.1:8002";

    let endpoint = "https://rpc.helius.xyz/?api-key=cee342ba-0773-41f7-a6e0-9ff01fff124b";
    let vote_program_id = "Vote111111111111111111111111111111111111111".to_string();
    let vote_program_id = Pubkey::from_str(&vote_program_id).unwrap();

    let client = RpcClient::new(endpoint);
    let vote_accounts = client.get_vote_accounts().unwrap();
    let leader_stakes = vote_accounts.current
        .iter()
        .chain(vote_accounts.delinquent.iter())
        .map(|x| (x.node_pubkey.clone(), x.activated_stake))
        .collect::<HashMap<_, _>>();
    let total_stake = leader_stakes.iter().fold(0, |sum, i| sum + *i.1);

    // let slot = 354;
    let slot = 194458133;
    let resp = get_block(slot, endpoint.to_string()).await;
    let block = resp.result;

    // // doesnt support new version txs 
    // let block = client.get_block(slot).unwrap();
    // println!("{:#?}", block);

    if block.transactions.is_none() { 
        println!("no transactions");
        return;
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
            println!("tx doesnt include vote program ...");
            continue;
        }

        let ix = msg.instructions().get(0).unwrap();
        let data = &ix.data;
        let vote_ix: VoteInstruction = bincode::deserialize(&data[..]).unwrap();
        let slot_vote = vote_ix.last_voted_slot().unwrap_or_default();
        let bank_hash = match &vote_ix { 
            VoteInstruction::Vote(v) => Some(v.hash),   
            VoteInstruction::CompactUpdateVoteState(v) => Some(v.hash),
            _ => None
        };

        println!("{:?}", vote_ix);
        println!("voted for slot {:?} with bank_hash {:?}", slot_vote, bank_hash);

        let node_pubkey = msg.static_account_keys().get(0).unwrap().to_string();
        let stake_amount = leader_stakes.get(&node_pubkey).unwrap();
        println!("{:?} {:?}", node_pubkey, stake_amount);

        // verify the signature
        let msg_bytes = msg.serialize();
        let sig_verifies: Vec<_> = tx.signatures
            .iter()
            .zip(msg.static_account_keys().iter())
            .map(|(signature, pubkey)| signature.verify(pubkey.as_ref(), &msg_bytes[..]))
            .collect();

        println!("{:?}", sig_verifies);

        break;
    }
}


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetBlockHeadersResponse {
    pub jsonrpc: String,
    pub result: Vec<u8>,
    pub id: i64,
}

async fn get_block_headers(slot: u64, endpoint: String) -> GetBlockHeadersResponse { 
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getBlockHeaders",
        "params":[
            slot
        ]
    }).to_string();
    let resp = send_rpc_call!(endpoint, request);
    let resp = serde_json::from_str::<GetBlockHeadersResponse>(&resp).unwrap();
    resp
}

pub async fn verify_slot() { 
    let endpoint = "http://127.0.0.1:8002";

    let client = RpcClient::new(endpoint);

    let slot = client.get_slot().unwrap();
    println!("verifying slot {:?}", slot);

    let block_headers = get_block_headers(slot, endpoint.to_string()).await.result;
    let block_headers: BlockHeader = bincode::deserialize(&block_headers).unwrap();

    let entries = block_headers.entries; 
    let last_blockhash = block_headers.last_blockhash;
    let verified = entries.verify(&last_blockhash);
    if !verified { 
        println!("entry verification failed ...");
        return;
    }
    println!("entry verification passed!");

}

#[tokio::main]
async fn main() {
    // parse_block_votes().await;
    verify_slot().await;

    // let resp = "{\"jsonrpc\":\"2.0\",\"result\":{\"accountsDeltaHash\":[72,37,80,44,121,2,123,218,154,95,56,154,190,231,20,7,220,13,70,32,251,65,167,171,241,50,39,232,177,201,254,73],\"entries\":[{\"hash\":[212,86,6,81,196,181,6,88,197,210,122,250,109,2,227,222,161,81,88,228,167,230,80,6,52,159,169,47,99,2,138,178],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[6,206,221,230,210,169,171,138,182,226,199,100,249,80,33,57,50,29,94,40,191,105,95,106,221,47,42,29,252,184,92,98],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[87,186,12,5,192,239,24,192,219,179,221,60,105,139,225,223,104,46,189,197,135,215,24,165,194,180,48,71,163,250,229,102],\"num_hashes\":1,\"transactions\":[{\"message\":[{\"accountKeys\":[[3],[44,100,235,64,24,246,103,20,190,251,182,50,183,81,229,196,26,101,177,222,37,199,163,58,206,241,131,197,74,145,136,251],[231,48,37,79,101,132,138,40,45,206,91,238,53,37,233,97,39,216,26,210,56,231,168,16,224,153,119,252,159,48,189,125],[7,97,72,29,53,116,116,187,124,77,118,36,235,211,189,179,216,53,94,115,209,16,67,252,13,163,83,128,0,0,0,0]],\"header\":{\"numReadonlySignedAccounts\":0,\"numReadonlyUnsignedAccounts\":1,\"numRequiredSignatures\":2},\"instructions\":[[1],{\"accounts\":[[2],1,1],\"data\":[[56],12,0,0,0,0,0,0,0,0,0,0,0,1,1,1,45,75,228,94,42,243,209,168,30,179,50,65,95,22,219,70,203,229,124,42,201,195,184,107,105,166,23,208,182,211,86,158,1,116,28,109,100,0,0,0,0],\"programIdIndex\":2}],\"recentBlockhash\":[24,110,123,52,199,36,82,29,99,70,104,3,62,234,249,43,199,200,126,209,251,81,68,153,45,205,48,245,159,243,149,151]}],\"signatures\":[[2],[227,126,93,66,187,230,93,36,44,148,73,253,157,241,120,134,17,13,20,203,172,153,58,57,230,34,152,141,80,213,240,188,41,245,158,56,14,242,75,26,36,73,168,180,169,113,195,14,192,84,29,233,250,52,117,8,240,121,90,33,183,70,60,11],[145,223,45,134,64,154,208,231,193,156,190,241,121,26,214,135,60,246,155,255,231,62,83,98,155,255,48,41,144,177,45,171,41,64,221,84,211,102,150,253,74,203,29,129,22,197,2,220,196,62,252,240,112,76,7,232,127,124,4,41,112,76,157,5]]}]},{\"hash\":[23,159,141,119,231,255,245,65,152,107,223,174,136,84,41,53,155,15,175,146,143,33,132,28,109,101,78,62,21,218,76,67],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[93,14,104,112,190,114,218,68,8,230,239,216,243,162,190,154,220,7,10,227,4,145,150,8,8,10,247,139,60,55,178,125],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[207,114,168,122,229,150,47,140,108,80,255,1,148,29,223,67,163,33,108,249,89,202,75,124,238,69,162,164,254,143,16,18],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[181,119,155,8,17,93,178,229,97,233,5,217,114,82,1,113,90,154,101,70,95,159,141,90,237,0,217,57,78,83,29,38],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[243,38,64,133,58,77,10,227,157,86,214,2,212,63,241,124,108,26,243,123,191,171,98,143,81,77,52,109,236,248,55,31],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[217,93,86,38,75,195,10,90,11,176,91,195,212,206,221,228,135,199,130,195,172,79,63,177,166,54,142,190,28,68,129,100],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[25,188,218,79,178,29,23,174,144,149,157,196,79,166,106,48,129,159,28,151,187,142,44,24,181,248,2,88,177,108,202,92],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[205,12,128,215,156,32,214,41,174,97,57,47,43,118,209,181,167,154,228,180,71,110,61,54,255,205,6,135,181,5,128,231],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[204,76,29,100,202,148,192,25,70,222,17,152,114,37,26,106,129,224,6,108,186,160,229,248,93,187,247,130,137,90,33,204],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[228,70,163,208,13,2,100,160,152,104,5,37,103,228,85,15,139,165,226,52,128,166,145,157,9,200,68,116,215,195,217,186],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[44,216,219,143,184,19,126,68,234,9,247,7,7,87,111,90,86,55,228,139,187,151,147,27,70,181,225,230,65,1,141,231],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[94,232,172,246,216,171,1,156,224,21,201,198,69,110,12,249,35,135,101,136,59,249,234,123,55,231,40,141,19,139,140,212],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[166,20,83,226,164,30,245,231,10,36,59,110,227,132,213,184,75,184,82,6,88,29,127,77,75,89,199,194,213,1,159,103],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[125,3,192,124,79,24,79,233,202,171,84,145,57,200,210,241,138,255,222,72,47,130,85,181,95,10,244,103,199,10,221,116],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[154,176,122,126,70,53,140,199,252,19,226,180,174,139,11,219,102,31,171,46,140,121,223,162,11,172,224,6,55,94,12,53],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[15,227,126,214,152,135,24,30,62,108,173,64,197,158,223,51,211,99,123,234,147,37,229,90,167,199,152,146,25,142,165,178],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[37,85,207,74,136,48,148,75,95,91,202,97,136,200,208,167,28,212,146,252,233,189,101,240,74,52,14,41,65,135,197,27],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[235,77,197,226,32,254,49,113,169,238,201,41,167,88,98,123,212,7,67,170,87,146,159,7,130,212,150,130,120,120,136,229],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[193,210,82,3,251,253,103,178,170,30,238,106,244,35,216,180,247,17,206,223,220,42,61,164,147,147,17,55,18,0,15,115],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[196,38,241,6,148,176,243,207,62,198,95,124,185,34,127,103,121,189,146,175,198,34,73,55,22,148,130,115,57,133,117,140],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[147,178,116,173,139,210,194,4,205,64,25,252,55,84,39,122,193,96,106,226,15,201,50,209,155,249,66,173,8,249,7,194],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[146,173,28,206,93,90,105,243,52,32,159,105,169,106,231,115,212,168,217,249,57,246,216,73,48,160,217,50,23,241,234,75],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[75,149,61,86,154,173,10,117,158,199,189,66,23,65,34,178,206,46,80,204,48,212,145,123,25,180,203,108,12,166,124,9],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[126,21,167,13,111,234,73,119,83,141,119,249,131,253,171,244,206,181,97,94,19,3,90,165,99,243,188,86,12,205,187,49],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[27,60,69,82,11,168,7,209,80,92,66,14,149,152,229,163,69,209,211,119,251,13,254,189,100,124,4,77,70,147,205,89],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[250,213,62,39,137,104,242,19,251,98,192,48,230,169,36,48,58,238,14,250,175,84,243,217,99,111,33,14,210,76,146,82],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[181,230,147,69,47,56,237,104,162,238,196,68,85,189,52,116,1,53,143,63,114,200,38,96,117,165,107,233,107,28,56,6],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[194,38,199,156,39,219,56,102,106,57,115,59,161,50,97,191,87,193,200,207,50,18,246,232,5,170,48,111,20,152,84,147],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[78,178,167,149,185,195,237,56,233,203,253,185,151,180,202,253,131,10,1,173,71,129,50,65,57,225,182,80,184,106,68,55],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[59,48,236,226,108,246,249,21,85,163,29,70,183,96,81,99,206,61,57,227,107,218,91,201,28,25,108,172,188,214,243,40],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[139,11,162,18,31,242,162,128,234,240,30,69,52,201,168,237,245,120,60,59,101,56,55,193,107,148,115,100,54,9,110,87],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[166,138,174,66,171,144,20,130,68,2,1,160,215,68,220,164,159,164,134,44,102,143,150,49,12,69,57,150,229,50,82,114],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[186,158,44,26,210,76,102,105,153,184,87,27,210,82,114,221,26,22,240,227,185,199,49,251,81,15,92,60,8,72,162,235],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[21,162,12,73,5,99,248,246,156,29,142,56,229,88,235,125,158,197,223,14,217,234,5,159,194,199,62,254,97,138,7,132],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[199,95,211,144,225,246,210,170,9,212,1,76,81,163,40,234,205,79,17,108,138,124,92,82,225,50,236,33,118,242,226,82],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[22,107,75,32,200,2,139,39,219,50,134,172,206,23,172,9,155,221,12,194,211,119,8,222,47,81,64,136,61,226,198,32],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[219,90,231,76,95,245,222,59,194,188,24,197,128,132,63,66,218,210,186,236,216,113,142,17,55,102,18,63,188,137,209,18],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[8,176,169,176,146,139,232,106,134,162,205,171,154,201,100,206,234,178,191,119,145,13,190,131,95,37,91,59,53,71,237,223],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[8,29,212,116,83,223,65,100,129,252,90,94,75,152,71,103,178,113,56,128,149,133,77,143,237,86,147,124,158,62,68,87],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[228,186,176,12,107,103,101,50,76,163,44,144,34,143,208,157,150,19,20,190,235,137,146,252,234,49,117,118,49,195,193,222],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[180,13,193,93,219,49,74,221,168,202,241,41,243,171,26,55,52,69,171,81,198,100,241,3,179,254,123,87,25,197,196,39],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[15,236,225,31,233,226,8,75,112,57,44,247,10,117,227,197,48,125,91,111,129,23,113,42,23,62,12,176,116,13,147,162],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[89,238,193,76,177,141,197,2,218,26,46,245,197,215,97,30,65,240,15,89,44,189,136,51,213,130,197,55,61,223,44,11],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[36,85,98,10,229,140,246,248,100,159,41,31,249,168,33,19,199,203,228,252,176,43,111,242,44,158,73,181,227,80,87,248],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[12,4,4,17,239,10,114,202,235,105,147,234,150,4,94,132,46,121,188,65,16,99,224,134,65,94,117,204,185,127,20,11],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[133,252,75,28,124,193,46,227,211,32,156,72,44,167,196,110,90,78,134,202,106,200,247,157,127,168,71,26,54,234,178,255],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[97,216,220,129,102,107,139,19,29,227,221,156,130,25,50,242,33,126,35,183,170,202,213,222,187,109,125,86,112,141,158,71],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[57,48,234,76,162,163,220,131,1,100,207,85,178,191,117,40,112,142,192,79,124,253,90,200,170,36,171,109,127,112,14,110],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[90,46,25,37,192,128,41,252,183,29,109,58,241,107,251,167,34,114,101,45,110,176,199,184,125,190,226,108,113,92,85,48],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[77,154,65,234,115,252,163,29,141,38,71,90,51,211,128,157,79,217,228,237,8,186,199,228,160,54,21,18,182,189,205,6],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[244,41,227,107,207,54,169,61,244,54,202,177,242,58,34,219,235,44,99,23,232,238,40,146,82,26,156,12,104,162,237,33],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[232,182,230,139,147,216,143,93,128,48,253,49,230,174,136,42,147,30,155,52,13,197,100,4,181,46,169,56,8,193,158,154],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[222,4,171,101,236,224,106,45,102,200,69,255,172,94,106,186,10,13,230,43,39,29,226,44,79,102,155,219,129,248,232,110],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[150,58,198,13,80,45,96,182,64,164,108,100,80,13,31,73,197,183,70,131,83,7,51,3,242,155,154,10,239,25,95,219],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[74,81,18,8,206,102,228,220,113,246,219,194,81,159,207,20,172,47,103,243,233,192,34,75,35,223,160,183,116,145,171,178],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[11,15,5,32,131,221,16,42,126,163,171,14,36,11,144,68,144,191,241,135,78,83,149,146,238,82,209,137,80,176,12,125],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[187,204,228,254,210,64,150,254,105,103,2,225,166,157,198,144,126,36,111,43,170,108,66,210,84,20,90,125,199,7,205,7],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[236,136,104,103,82,131,5,75,191,10,211,45,56,232,38,119,112,144,28,11,65,157,137,54,57,127,203,166,213,54,62,249],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[105,57,214,163,220,160,165,50,137,198,246,118,189,46,155,234,75,26,152,214,36,79,158,183,124,131,170,63,211,236,209,227],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[212,90,58,231,219,117,24,81,125,87,103,34,149,178,142,250,152,236,50,204,249,6,186,141,119,213,223,55,153,26,143,193],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[193,22,15,35,154,253,0,141,207,191,49,111,185,160,137,146,155,135,212,202,194,56,31,67,94,95,49,177,136,95,6,115],\"num_hashes\":1,\"transactions\":[]},{\"hash\":[235,35,218,107,117,236,30,38,104,80,237,175,108,213,219,28,27,101,97,44,62,73,6,220,170,107,139,96,231,223,185,151],\"num_hashes\":1,\"transactions\":[]}],\"lastBlockhash\":[235,35,218,107,117,236,30,38,104,80,237,175,108,213,219,28,27,101,97,44,62,73,6,220,170,107,139,96,231,223,185,151],\"parentHash\":[45,75,228,94,42,243,209,168,30,179,50,65,95,22,219,70,203,229,124,42,201,195,184,107,105,166,23,208,182,211,86,158],\"signatureCountBuf\":[2,0,0,0,0,0,0,0]},\"id\":1}\n";
    // let resp = serde_json::from_str::<GetBlockHeadersResponse>(&resp).unwrap();
    // println!("{:#?}", resp);



    // let endpoint = "http://127.0.0.1:8002";

    // // // GPA on stake times out here
    // // let endpoint = "https://rpc.helius.xyz/?api-key=cee342ba-0773-41f7-a6e0-9ff01fff124b";
    // let client = RpcClient::new(endpoint);

    // let vote_accounts = client.get_vote_accounts().unwrap();
    // let leader_stakes = vote_accounts.current
    //     .iter()
    //     .chain(vote_accounts.delinquent.iter())
    //     .map(|x| (x.node_pubkey.clone(), x.activated_stake))
    //     .collect::<HashMap<_, _>>();
    // println!("{:?}", leader_stakes);

    // println!("---");
    // let stake_program = Pubkey::from_str("Stake11111111111111111111111111111111111111").unwrap();
    // let stake_accounts = client.get_program_accounts(&stake_program).unwrap();
    // for (pubkey, account) in stake_accounts.iter() { 
    //     let stake = parse_stake(account.data.as_slice()).unwrap();
    //     match stake {
    //         StakeAccountType::Initialized(stake) => println!("{:?}", stake),
    //         StakeAccountType::Delegated(stake) => println!("{:?}", stake),
    //         _ => {}
    //     }
    // }

    // println!("---");
    // let vote_program = Pubkey::from_str("Vote111111111111111111111111111111111111111").unwrap();
    // let vote_accounts = client.get_program_accounts(&vote_program).unwrap();
    // for (_, account) in vote_accounts.iter() { 
    //     let vote = parse_vote(account.data.as_slice()).unwrap();
    //     println!("{:?}", vote);
    // }
    
    // println!("---");
    // let leader_schedule = client.get_leader_schedule(None).unwrap().unwrap();
    // println!("{:?}", leader_schedule);

    // let slot = 194458133;
    // let leader_schedule = client.get_leader_schedule(Some(slot)).unwrap().unwrap();
    // let leaders = leader_schedule.iter().map(|(pubkey, _)| Pubkey::from_str(pubkey).unwrap()).collect::<Vec<_>>();
    // let stakes = leaders.iter().map(|leader| { 
    //     // todo: get stake account pubkey
    //     let stake = client.get_stake_activation(*leader, None).unwrap();
    //     let stake_amount = stake.active;
    //     stake_amount
    // });

    // let leader_stakes = leaders.iter().zip(stakes).collect::<HashMap<_, _>>();
    // println!("{:#?}", leader_stakes);

}
