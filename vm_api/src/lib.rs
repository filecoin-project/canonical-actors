use std::collections::BTreeMap;

use anyhow::Error;
use cid::Cid;
use fvm_ipld_blockstore::Blockstore;
use fvm_ipld_encoding::{
    ipld_block::IpldBlock,
    tuple::{serde_tuple, Deserialize_tuple, Serialize_tuple},
};
use fvm_shared::{
    address::Address,
    clock::ChainEpoch,
    crypto::{
        hash::SupportedHashes,
        signature::{Signature, SECP_PUB_LEN, SECP_SIG_LEN, SECP_SIG_MESSAGE_HASH_SIZE},
    },
    econ::TokenAmount,
    error::ExitCode,
    piece::PieceInfo,
    sector::{RegisteredSealProof, ReplicaUpdateInfo},
    MethodNum,
};

use builtin::*;
pub use error::*;
use trace::*;

pub mod builtin;
mod error;
pub mod trace;
#[cfg(feature = "testing")]
pub mod util;

/// An abstract VM that is injected into integration tests
pub trait VM {
    /// Returns the underlying blockstore of the VM
    fn blockstore(&self) -> &dyn Blockstore;

    /// Get information about an actor
    fn actor(&self, address: &Address) -> Option<ActorState>;

    /// Upsert an actor into the state tree
    fn set_actor(&self, key: &Address, a: ActorState);

    /// Get the balance of the specified actor
    fn balance(&self, address: &Address) -> TokenAmount;

    /// Get the ID for the specified address
    fn resolve_id_address(&self, address: &Address) -> Option<Address>;

    /// Send a message between the two specified actors
    fn execute_message(
        &self,
        from: &Address,
        to: &Address,
        value: &TokenAmount,
        method: MethodNum,
        params: Option<IpldBlock>,
    ) -> Result<MessageResult, VMError>;

    /// Send a message without charging gas
    fn execute_message_implicit(
        &self,
        from: &Address,
        to: &Address,
        value: &TokenAmount,
        method: MethodNum,
        params: Option<IpldBlock>,
    ) -> Result<MessageResult, VMError>;

    /// Take all the invocations that have been made since the last call to this method
    fn take_invocations(&self) -> Vec<InvocationTrace>;

    /// Provides access to VM primitives
    fn primitives(&self) -> Box<dyn Primitives>;

    /// Return a map of actor code CIDs to their corresponding types
    fn actor_manifest(&self) -> BTreeMap<Cid, Type>;

    /// Returns a map of all actor addresses to their corresponding states
    fn actor_states(&self) -> BTreeMap<Address, ActorState>;

    // Overridable constants and extern behaviour

    /// Get the current chain epoch
    fn epoch(&self) -> ChainEpoch;

    /// Sets the epoch to the specified value
    fn set_epoch(&self, epoch: ChainEpoch);

    /// Get the circulating supply constant for the network
    fn circulating_supply(&self) -> TokenAmount;

    /// Set the circulating supply constant for the network
    fn set_circulating_supply(&self, supply: TokenAmount);

    /// Get the current base fee
    fn base_fee(&self) -> TokenAmount;

    /// Set the current base fee
    fn set_base_fee(&self, amount: TokenAmount);

    /// Get the current timestamp
    fn timestamp(&self) -> u64;

    /// Set the current timestamp
    fn set_timestamp(&self, timestamp: u64);

    /// Get the initial state root of the block
    fn initial_state_root(&self) -> Cid;

    /// Set the initial state root of the block
    fn set_initial_state_root(&self, state_root: Cid);

    // Override the signature verification behaviour
    fn override_verify_signature(
        &self,
        verify_signature: fn(&Signature, &Address, &[u8]) -> Result<(), Error>,
    );

    fn override_verifiy_replica_update(
        &self,
        verify_replica_update: fn(&ReplicaUpdateInfo) -> Result<(), anyhow::Error>,
    );
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MessageResult {
    pub code: ExitCode,
    pub message: String,
    pub ret: Option<IpldBlock>,
}

// Duplicates an internal FVM type (fvm::state_tree::ActorState) that cannot be depended on here
#[derive(Serialize_tuple, Deserialize_tuple, Clone, PartialEq, Eq, Debug)]
pub struct ActorState {
    /// Link to code for the actor.
    pub code: Cid,
    /// Link to the state of the actor.
    pub state: Cid,
    /// Sequence of the actor.
    pub sequence: u64,
    /// Tokens available to the actor.
    pub balance: TokenAmount,
    /// The actor's "delegated" address, if assigned.
    ///
    /// This field is set on actor creation and never modified.
    pub delegated_address: Option<Address>,
}

pub fn new_actor(
    code: Cid,
    state: Cid,
    sequence: u64,
    balance: TokenAmount,
    delegated_address: Option<Address>,
) -> ActorState {
    ActorState { code, state, sequence, balance, delegated_address }
}

/// Pure functions implemented as primitives by the runtime.
pub trait Primitives {
    /// Hashes input data using blake2b with 256 bit output.
    fn hash_blake2b(&self, data: &[u8]) -> [u8; 32];

    /// Hashes input data using a supported hash function.
    fn hash(&self, hasher: SupportedHashes, data: &[u8]) -> Vec<u8>;

    /// Hashes input into a 64 byte buffer
    fn hash_64(&self, hasher: SupportedHashes, data: &[u8]) -> ([u8; 64], usize);

    /// Computes an unsealed sector CID (CommD) from its constituent piece CIDs (CommPs) and sizes.
    fn compute_unsealed_sector_cid(
        &self,
        proof_type: RegisteredSealProof,
        pieces: &[PieceInfo],
    ) -> Result<Cid, Error>;

    /// Verifies that a signature is valid for an address and plaintext.
    fn verify_signature(
        &self,
        signature: &Signature,
        signer: &Address,
        plaintext: &[u8],
    ) -> Result<(), Error>;

    fn recover_secp_public_key(
        &self,
        hash: &[u8; SECP_SIG_MESSAGE_HASH_SIZE],
        signature: &[u8; SECP_SIG_LEN],
    ) -> Result<[u8; SECP_PUB_LEN], Error>;

    fn verify_replica_update(&self, replica: &ReplicaUpdateInfo) -> Result<(), Error>;
}
