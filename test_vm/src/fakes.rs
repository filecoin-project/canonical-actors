use anyhow::anyhow;
use anyhow::Error;
use cid::multihash::Code;
use cid::multihash::MultihashDigest;
use cid::Cid;
use fvm_shared::address::{Address, SECP_PUB_LEN};
use fvm_shared::crypto::hash::SupportedHashes;
use fvm_shared::crypto::signature::{Signature, SECP_SIG_LEN, SECP_SIG_MESSAGE_HASH_SIZE};
use fvm_shared::piece::PieceInfo;
use fvm_shared::sector::RegisteredSealProof;
use fvm_shared::sector::ReplicaUpdateInfo;
use integer_encoding::VarInt;

use fil_actors_runtime::runtime::Primitives;
use fil_actors_runtime::test_utils::{make_piece_cid, recover_secp_public_key};

/// Fake implementation of runtime primitives.
#[derive(Default, Clone)]
#[allow(clippy::type_complexity)]
pub struct FakePrimitives {
    pub verify_signature_override: Option<fn(&Signature, &Address, &[u8]) -> Result<(), Error>>,
    pub verify_replica_update: Option<fn(&ReplicaUpdateInfo) -> Result<(), Error>>,
}

impl Primitives for FakePrimitives {
    fn hash_blake2b(&self, data: &[u8]) -> [u8; 32] {
        blake2b_simd::Params::new()
            .hash_length(32)
            .to_state()
            .update(data)
            .finalize()
            .as_bytes()
            .try_into()
            .unwrap()
    }

    fn hash(&self, hasher: SupportedHashes, data: &[u8]) -> Vec<u8> {
        let hasher = Code::try_from(hasher as u64).unwrap(); // supported hashes are all implemented in multihash
        hasher.digest(data).digest().to_owned()
    }

    fn hash_64(&self, hasher: SupportedHashes, data: &[u8]) -> ([u8; 64], usize) {
        let hasher = Code::try_from(hasher as u64).unwrap();
        let (len, buf, ..) = hasher.digest(data).into_inner();
        (buf, len as usize)
    }

    fn compute_unsealed_sector_cid(
        &self,
        proof_type: RegisteredSealProof,
        pieces: &[PieceInfo],
    ) -> Result<Cid, Error> {
        // Construct a buffer that depends on all the input data.
        let mut buf: Vec<u8> = Vec::new();
        let ptv: i64 = proof_type.into();
        buf.extend(ptv.encode_var_vec());
        for p in pieces {
            buf.extend(&p.cid.to_bytes());
            buf.extend(p.size.0.encode_var_vec())
        }
        Ok(make_piece_cid(&buf))
    }

    fn verify_signature(
        &self,
        signature: &Signature,
        _signer: &Address,
        plaintext: &[u8],
    ) -> Result<(), Error> {
        if self.verify_signature_override.is_some() {
            return (self.verify_signature_override.unwrap())(signature, _signer, plaintext);
        }

        // default behaviour expects signature bytes to be equal to plaintext
        if signature.bytes != plaintext {
            return Err(anyhow::format_err!(
                "invalid signature (mock sig validation expects siggy bytes to be equal to plaintext)"
            ));
        }
        Ok(())
    }

    fn recover_secp_public_key(
        &self,
        hash: &[u8; SECP_SIG_MESSAGE_HASH_SIZE],
        signature: &[u8; SECP_SIG_LEN],
    ) -> Result<[u8; SECP_PUB_LEN], Error> {
        recover_secp_public_key(hash, signature).map_err(|_| anyhow!("failed to recover pubkey"))
    }

    fn verify_replica_update(&self, replica: &ReplicaUpdateInfo) -> Result<(), Error> {
        if self.verify_replica_update.is_some() {
            return (self.verify_replica_update.unwrap())(replica);
        }

        // default behaviour always succeeds
        Ok(())
    }
}
