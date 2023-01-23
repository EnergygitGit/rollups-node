use crate::machine::BrokerReceive;
use crate::tx_sender::TxSender;

use state_fold_types::ethereum_types::Address;
use types::foldables::claims::History;

use anyhow::Result;

use tracing::{info, instrument, trace};

#[derive(Debug)]
pub struct BlockchainDriver {
    dapp_address: Address,
}

impl BlockchainDriver {
    pub fn new(dapp_address: Address) -> Self {
        Self { dapp_address }
    }

    #[instrument(level = "trace", skip_all)]
    pub async fn react<TS: TxSender + Sync + Send>(
        &self,
        history: &History,
        broker: &impl BrokerReceive,
        mut tx_sender: TS,
    ) -> Result<TS> {
        let claims_sent = claims_sent(history, &self.dapp_address);
        trace!(?claims_sent);

        while let Some(claim) = broker.next_claim().await? {
            trace!("Got claim `{:?}` from broker", claim);
            if claim.number > claims_sent {
                info!("Sending claim `{:?}`", claim);
                tx_sender = tx_sender.send_claim_tx(&claim.hash).await?;
            }
        }

        Ok(tx_sender)
    }
}

fn claims_sent(history: &History, dapp_address: &Address) -> u64 {
    match history.dapp_claims.get(dapp_address) {
        Some(c) => c.claims.len() as u64,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use im::{hashmap, Vector};
    use rand::Rng;
    use state_fold_types::ethereum_types::{Address, H160, H256};
    use std::sync::Arc;
    use types::foldables::claims::{Claim, DAppClaims, History};

    use crate::{drivers::mock, machine::RollupClaim};

    use super::BlockchainDriver;

    // --------------------------------------------------------------------------------------------

    #[test]
    fn test_new() {
        let dapp_address = H160::default();
        let blockchain_driver = BlockchainDriver::new(dapp_address);
        assert_eq!(blockchain_driver.dapp_address, dapp_address);
    }

    // --------------------------------------------------------------------------------------------

    fn random_claim() -> Claim {
        let mut rng = rand::thread_rng();
        let start_input_index = rng.gen();
        Claim {
            epoch_hash: H256::random(),
            start_input_index,
            end_input_index: start_input_index + 5,
            claim_timestamp: rng.gen(),
        }
    }

    fn random_claims(n: usize) -> Vec<Claim> {
        let mut claims = Vec::new();
        claims.resize_with(n, || random_claim());
        claims
    }

    fn new_history() -> History {
        History {
            history_address: Arc::new(H160::random()),
            dapp_claims: Arc::new(hashmap! {}),
        }
    }

    fn update_history(
        history: &History,
        dapp_address: Address,
        n: usize,
    ) -> History {
        let claims = random_claims(n)
            .iter()
            .map(|x| Arc::new(x.clone()))
            .collect::<Vec<_>>();
        let claims = Vector::from(claims);
        let dapp_claims = history
            .dapp_claims
            .update(Arc::new(dapp_address), Arc::new(DAppClaims { claims }));
        History {
            history_address: history.history_address.clone(),
            dapp_claims: Arc::new(dapp_claims),
        }
    }

    // --------------------------------------------------------------------------------------------

    #[test]
    fn test_claims_sent_some_0() {
        let dapp_address = H160::random();
        let history = new_history();
        let history = update_history(&history, dapp_address, 0);
        let n = super::claims_sent(&history, &dapp_address);
        assert_eq!(n, 0);
    }

    #[test]
    fn test_claims_sent_some_1() {
        let dapp_address1 = H160::random();
        let dapp_address2 = H160::random();
        let history = new_history();
        let history = update_history(&history, dapp_address1, 0);
        let history = update_history(&history, dapp_address2, 1);
        let n = super::claims_sent(&history, &dapp_address1);
        assert_eq!(n, 0);
        let n = super::claims_sent(&history, &dapp_address2);
        assert_eq!(n, 1);
    }

    #[test]
    fn test_claims_sent_some_n() {
        let dapp_address1 = H160::random();
        let dapp_address2 = H160::random();
        let history = new_history();
        let history = update_history(&history, dapp_address1, 5);
        let history = update_history(&history, dapp_address2, 2);
        let n = super::claims_sent(&history, &dapp_address1);
        assert_eq!(n, 5);
        let n = super::claims_sent(&history, &dapp_address2);
        assert_eq!(n, 2);
    }

    #[test]
    fn test_claims_sent_none() {
        let dapp_address1 = H160::random();
        let dapp_address2 = H160::random();
        let history = new_history();
        let history = update_history(&history, dapp_address1, 1);
        let n = super::claims_sent(&history, &dapp_address2);
        assert_eq!(n, 0);
    }

    // --------------------------------------------------------------------------------------------

    async fn test_react(next_claims: Vec<u64>, n: usize) {
        let dapp_address = H160::random();
        let blockchain_driver = BlockchainDriver::new(dapp_address);

        let history = new_history();
        let history = update_history(&history, dapp_address, 5);
        let history = update_history(&history, H160::random(), 2);

        let next_claims = next_claims
            .iter()
            .map(|n| {
                let mut rng = rand::thread_rng();
                let hash = (0..32).map(|_| rng.gen()).collect::<Vec<u8>>();
                assert_eq!(hash.len(), 32);
                let hash: [u8; 32] = hash.try_into().unwrap();
                RollupClaim { hash, number: *n }
            })
            .collect();
        let broker = mock::Broker::new(vec![], next_claims);
        let tx_sender = mock::TxSender::new();

        let result =
            blockchain_driver.react(&history, &broker, tx_sender).await;
        assert!(result.is_ok());
        let tx_sender = result.unwrap();
        assert_eq!(tx_sender.count(), n);
    }

    #[tokio::test]
    async fn test_react_no_claim() {
        test_react(vec![], 0).await;
    }

    // broker has 1 (new) claim -- sent 1 claim
    #[tokio::test]
    async fn test_react_1_new_claim_sent_1_claim() {
        test_react(vec![6], 1).await;
    }

    // broker has 1 (old) claim -- sent 0 claims
    #[tokio::test]
    async fn test_react_1_old_claim_sent_0_claims() {
        test_react(vec![5], 0).await;
    }

    // broker has 2 claims (1 old, 1 new) -- sent 1 claim
    #[tokio::test]
    async fn test_react_2_claims_sent_1_claim() {
        test_react(vec![5, 6], 1).await;
    }

    // broker has interleaved old and new claims -- sent 5 new claims
    #[tokio::test]
    async fn test_react_interleaved_old_new_claims_sent_5_claims() {
        test_react(vec![1, 5, 6, 2, 3, 7, 8, 4, 5, 9, 10], 5).await;
    }
}