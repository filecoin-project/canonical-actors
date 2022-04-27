use cid::Cid;
use fvm_ipld_blockstore::Blockstore;
use fvm_ipld_encoding::{BytesDe, RawBytes};
use fvm_ipld_hamt::BytesKey;
use fvm_ipld_hamt::Error;
use fvm_shared::address::Address;
use fvm_shared::bigint::bigint_ser::BigIntDe;
use fvm_shared::clock::ChainEpoch;
use fvm_shared::econ::TokenAmount;
use fvm_shared::error::ExitCode;
use fvm_shared::sector::{RegisteredPoStProof, RegisteredSealProof, StoragePower};
use fvm_shared::smooth::FilterEstimate;
use fvm_shared::MethodNum;
use lazy_static::lazy_static;
use num_traits::Zero;
use serde::de::DeserializeOwned;
use serde::Serialize;

use fil_actor_power::ext::init::ExecParams;
use fil_actor_power::ext::miner::MinerConstructorParams;
use fil_actor_power::{
    ext, Claim, CreateMinerParams, CreateMinerReturn, CurrentTotalPowerReturn, Method, State,
    UpdateClaimedPowerParams,
};
use fil_actors_runtime::builtin::HAMT_BIT_WIDTH;
use fil_actors_runtime::runtime::Runtime;
use fil_actors_runtime::test_utils::{
    MockRuntime, ACCOUNT_ACTOR_CODE_ID, MINER_ACTOR_CODE_ID, MULTISIG_ACTOR_CODE_ID,
    SYSTEM_ACTOR_CODE_ID,
};
use fil_actors_runtime::{
    make_map_with_root_and_bitwidth, ActorError, Map, INIT_ACTOR_ADDR, STORAGE_POWER_ACTOR_ADDR,
    SYSTEM_ACTOR_ADDR,
};

use crate::PowerActor;

lazy_static! {
    pub static ref OWNER: Address = Address::new_id(101);
    pub static ref MINER: Address = Address::new_id(201);
    pub static ref ACTOR: Address = Address::new_actor("actor".as_bytes());
}

pub fn new_runtime() -> MockRuntime {
    MockRuntime {
        receiver: *STORAGE_POWER_ACTOR_ADDR,
        caller: *SYSTEM_ACTOR_ADDR,
        caller_type: *SYSTEM_ACTOR_CODE_ID,
        ..Default::default()
    }
}

pub fn new_harness() -> Harness {
    let rwd = TokenAmount::from(10) * TokenAmount::from(10_i128.pow(18));
    Harness {
        miner_seq: 0,
        seal_proof: RegisteredSealProof::StackedDRG32GiBV1P1,
        window_post_proof: RegisteredPoStProof::StackedDRGWindow32GiBV1,
        this_epoch_baseline_power: StoragePower::from(1i64 << 50),
        this_epoch_reward_smoothed: FilterEstimate::new(rwd, TokenAmount::zero()),
    }
}

pub fn setup() -> (Harness, MockRuntime) {
    let mut rt = new_runtime();
    let h = new_harness();
    h.construct(&mut rt);
    (h, rt)
}

#[allow(dead_code)]
pub struct Harness {
    miner_seq: i64,
    seal_proof: RegisteredSealProof,
    window_post_proof: RegisteredPoStProof,
    this_epoch_baseline_power: StoragePower,
    this_epoch_reward_smoothed: FilterEstimate,
}

impl Harness {
    pub fn construct(&self, rt: &mut MockRuntime) {
        rt.expect_validate_caller_addr(vec![*SYSTEM_ACTOR_ADDR]);
        rt.call::<PowerActor>(Method::Constructor as MethodNum, &RawBytes::default()).unwrap();
        rt.verify()
    }

    pub fn construct_and_verify(&self, rt: &mut MockRuntime) {
        self.construct(rt);
        let st: State = rt.get_state();
        assert_eq!(StoragePower::zero(), st.total_raw_byte_power);
        assert_eq!(StoragePower::zero(), st.total_bytes_committed);
        assert_eq!(StoragePower::zero(), st.total_quality_adj_power);
        assert_eq!(StoragePower::zero(), st.total_qa_bytes_committed);
        assert_eq!(TokenAmount::zero(), st.total_pledge_collateral);
        assert_eq!(StoragePower::zero(), st.total_raw_byte_power);
        assert_eq!(StoragePower::zero(), st.this_epoch_quality_adj_power);
        assert_eq!(TokenAmount::zero(), st.this_epoch_pledge_collateral);
        assert_eq!(ChainEpoch::zero(), st.first_cron_epoch);
        assert_eq!(0, st.miner_count);
        assert_eq!(0, st.miner_above_min_power_count);

        verify_empty_map(rt, st.claims);
        verify_empty_map(rt, st.cron_event_queue);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_miner(
        &self,
        rt: &mut MockRuntime,
        owner: &Address,
        worker: &Address,
        miner: &Address,
        robust: &Address,
        peer: Vec<u8>,
        multiaddrs: Vec<BytesDe>,
        window_post_proof_type: RegisteredPoStProof,
        value: &TokenAmount,
    ) -> Result<(), ActorError> {
        rt.set_caller(*ACCOUNT_ACTOR_CODE_ID, *owner);
        rt.set_value(value.clone());
        rt.set_balance(value.clone());
        rt.expect_validate_caller_type(vec![*ACCOUNT_ACTOR_CODE_ID, *MULTISIG_ACTOR_CODE_ID]);

        let miner_ctor_params = MinerConstructorParams {
            owner: *owner,
            worker: *worker,
            control_addresses: vec![],
            window_post_proof_type,
            peer_id: peer.clone(),
            multi_addresses: multiaddrs.clone(),
        };
        let expected_init_params = ExecParams {
            code_cid: *MINER_ACTOR_CODE_ID,
            constructor_params: RawBytes::serialize(miner_ctor_params).unwrap(),
        };
        let create_miner_ret = CreateMinerReturn { id_address: *miner, robust_address: *robust };
        rt.expect_send(
            *INIT_ACTOR_ADDR,
            ext::init::EXEC_METHOD,
            RawBytes::serialize(expected_init_params).unwrap(),
            value.clone(),
            RawBytes::serialize(create_miner_ret).unwrap(),
            ExitCode::OK,
        );
        let params = CreateMinerParams {
            owner: *owner,
            worker: *worker,
            window_post_proof_type,
            peer,
            multiaddrs,
        };
        rt.call::<PowerActor>(
            Method::CreateMiner as MethodNum,
            &RawBytes::serialize(params).unwrap(),
        )?;
        Ok(())
    }

    pub fn create_miner_basic(
        &mut self,
        rt: &mut MockRuntime,
        owner: Address,
        worker: Address,
        miner: Address,
    ) -> Result<(), ActorError> {
        let label = format!("{}", self.miner_seq);
        let actr_addr = Address::new_actor(label.as_bytes());
        self.miner_seq += 1;
        let peer = label.as_bytes().to_vec();
        self.create_miner(
            rt,
            &owner,
            &worker,
            &miner,
            &actr_addr,
            peer,
            vec![],
            self.window_post_proof,
            &TokenAmount::from(0),
        )
    }

    pub fn list_miners(&self, rt: &MockRuntime) -> Vec<Address> {
        let st: State = rt.get_state();
        let claims: Map<_, Claim> =
            make_map_with_root_and_bitwidth(&st.claims, rt.store(), HAMT_BIT_WIDTH).unwrap();
        let keys = collect_keys(claims).unwrap();
        keys.iter().map(|k| Address::from_bytes(k).unwrap()).collect::<Vec<_>>()
    }

    pub fn get_claim(&self, rt: &MockRuntime, miner: &Address) -> Option<Claim> {
        let st: State = rt.get_state();
        let claims =
            make_map_with_root_and_bitwidth(&st.claims, rt.store(), HAMT_BIT_WIDTH).unwrap();
        claims.get(&miner.to_bytes()).unwrap().cloned()
    }

    pub fn check_state(&self) {
        // TODO: https://github.com/filecoin-project/builtin-actors/issues/44
    }

    pub fn update_pledge_total(&self, rt: &mut MockRuntime, miner: Address, delta: &TokenAmount) {
        let st: State = rt.get_state();
        let prev = st.total_pledge_collateral;

        rt.set_caller(*MINER_ACTOR_CODE_ID, miner);
        rt.expect_validate_caller_type(vec![*MINER_ACTOR_CODE_ID]);
        rt.call::<PowerActor>(
            Method::UpdatePledgeTotal as MethodNum,
            &RawBytes::serialize(BigIntDe(delta.clone())).unwrap(),
        )
        .unwrap();
        rt.verify();

        let st: State = rt.get_state();
        assert_eq!(prev + delta, st.total_pledge_collateral);
    }

    pub fn current_power_total(&self, rt: &mut MockRuntime) -> CurrentTotalPowerReturn {
        rt.expect_validate_caller_any();
        let ret: CurrentTotalPowerReturn = rt
            .call::<PowerActor>(Method::CurrentTotalPower as u64, &RawBytes::default())
            .unwrap()
            .deserialize()
            .unwrap();
        rt.verify();
        ret
    }

    pub fn update_claimed_power(
        &self,
        rt: &mut MockRuntime,
        miner: Address,
        raw_delta: &StoragePower,
        qa_delta: &StoragePower,
    ) {
        let prev_cl = self.get_claim(rt, &miner).unwrap();

        let params = UpdateClaimedPowerParams {
            raw_byte_delta: raw_delta.clone(),
            quality_adjusted_delta: qa_delta.clone(),
        };
        rt.set_caller(*MINER_ACTOR_CODE_ID, miner);
        rt.expect_validate_caller_type(vec![*MINER_ACTOR_CODE_ID]);
        rt.call::<PowerActor>(
            Method::UpdateClaimedPower as MethodNum,
            &RawBytes::serialize(params).unwrap(),
        )
        .unwrap();
        rt.verify();

        let cl = self.get_claim(rt, &miner).unwrap();
        let expected_raw = &prev_cl.raw_byte_power + raw_delta;
        let expected_adjusted = &prev_cl.quality_adj_power + qa_delta;
        if expected_raw.is_zero() {
            assert!(cl.raw_byte_power.is_zero());
        } else {
            assert_eq!(prev_cl.raw_byte_power + raw_delta, cl.raw_byte_power);
        }

        if expected_adjusted.is_zero() {
            assert!(cl.quality_adj_power.is_zero());
        } else {
            assert_eq!(prev_cl.quality_adj_power + qa_delta, cl.quality_adj_power);
        }
    }

    pub fn expect_total_power_eager(
        &self,
        rt: &mut MockRuntime,
        expected_raw: &StoragePower,
        expected_qa: &StoragePower,
    ) {
        let st: State = rt.get_state();

        let (raw_byte_power, quality_adj_power) = st.current_total_power();
        assert_eq!(expected_raw, &raw_byte_power);
        assert_eq!(expected_qa, &quality_adj_power);
    }

    pub fn expect_total_pledge_eager(&self, rt: &mut MockRuntime, expected_pledge: &TokenAmount) {
        let st: State = rt.get_state();
        assert_eq!(expected_pledge, &st.total_pledge_collateral);
    }
}

/// Collects all keys from a map into a vector.
fn collect_keys<BS, V>(m: Map<BS, V>) -> Result<Vec<BytesKey>, Error>
where
    BS: Blockstore,
    V: DeserializeOwned + Serialize,
{
    let mut ret_keys = Vec::new();
    m.for_each(|k, _| {
        ret_keys.push(k.clone());
        Ok(())
    })?;

    Ok(ret_keys)
}

pub fn verify_empty_map(rt: &MockRuntime, key: Cid) {
    let map =
        make_map_with_root_and_bitwidth::<_, BigIntDe>(&key, &rt.store, HAMT_BIT_WIDTH).unwrap();
    map.for_each(|_key, _val| panic!("expected no keys")).unwrap();
}
