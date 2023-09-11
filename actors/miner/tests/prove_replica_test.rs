use fvm_shared::sector::{SectorNumber, StoragePower};
use fvm_shared::{bigint::Zero, clock::ChainEpoch, ActorID};

use fil_actor_miner::ProveReplicaUpdates2Return;
use fil_actor_miner::State;
use fil_actors_runtime::{runtime::Runtime, BatchReturn, DealWeight, EPOCHS_IN_DAY};
use util::*;

mod util;

const CLIENT_ID: ActorID = 1000;
const DEFAULT_SECTOR_EXPIRATION_DAYS: ChainEpoch = 220;
const FIRST_SECTOR_NUMBER: SectorNumber = 100;

#[test]
fn prove_basic_updates() {
    let h = ActorHarness::new_with_options(HarnessOptions::default());
    let rt = h.new_runtime();
    rt.set_balance(BIG_BALANCE.clone());
    h.construct_and_verify(&rt);

    // Onboard a batch of empty sectors.
    rt.set_epoch(1);
    let sector_expiry = *rt.epoch.borrow() + DEFAULT_SECTOR_EXPIRATION_DAYS * EPOCHS_IN_DAY;
    let sector_count = 4;
    let sectors = onboard_empty_sectors(&rt, &h, sector_expiry, FIRST_SECTOR_NUMBER, sector_count);
    let snos = sectors.iter().map(|s| s.sector_number).collect::<Vec<_>>();

    // Update them in batch, each with a single piece.
    let st: State = h.get_state(&rt);
    let store = rt.store();
    let piece_size = h.sector_size as u64;
    let sector_updates = vec![
        make_update_manifest(&st, store, &sectors[0], &[(piece_size, 0, 0, 0)]), // No alloc or deal
        make_update_manifest(&st, store, &sectors[1], &[(piece_size, CLIENT_ID, 1000, 0)]), // Just an alloc
        make_update_manifest(&st, store, &sectors[2], &[(piece_size, 0, 0, 2000)]), // Just a deal
        make_update_manifest(&st, store, &sectors[3], &[(piece_size, CLIENT_ID, 1001, 2001)]), // Alloc and deal
    ];

    let cfg = ProveReplicaUpdatesConfig::default();
    let result = h.prove_replica_updates2_batch(&rt, &sector_updates, true, true, cfg).unwrap();
    assert_eq!(
        ProveReplicaUpdates2Return { activation_results: BatchReturn::ok(sectors.len() as u32) },
        result
    );

    let duration = sector_expiry - *rt.epoch.borrow();
    let expected_weight = DealWeight::from(piece_size) * duration;
    let raw_power = StoragePower::from(h.sector_size as u64);
    let verified_power = &raw_power * 10;
    let raw_pledge = h.initial_pledge_for_power(&rt, &raw_power);
    let verified_pledge = h.initial_pledge_for_power(&rt, &verified_power);

    // Sector 0: Even though there's no "deal", the data weight is set.
    verify_weights(&rt, &h, snos[0], &expected_weight, &DealWeight::zero(), &raw_pledge);
    // Sector 1: With an allocation, the verified weight is set instead.
    verify_weights(&rt, &h, snos[1], &DealWeight::zero(), &expected_weight, &verified_pledge);
    // Sector 2: Deal weight is set.
    verify_weights(&rt, &h, snos[2], &expected_weight, &DealWeight::zero(), &raw_pledge);
    // Sector 3: Deal doesn't make a difference to verified weight only set.
    verify_weights(&rt, &h, snos[3], &DealWeight::zero(), &expected_weight, &verified_pledge);
}