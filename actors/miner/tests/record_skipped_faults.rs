use fil_actor_miner::power_for_sector;
use fil_actor_miner::power_for_sectors;
use fil_actor_miner::select_sectors;
use fil_actor_miner::Partition;
use fil_actor_miner::SectorOnChainInfo;
use fil_actor_miner::Sectors;
use fil_actor_miner::SECTORS_AMT_BITWIDTH;
use fil_actors_runtime::runtime::Policy;
use fil_actors_runtime::test_utils::MessageAccumulator;
use fil_actors_runtime::test_utils::MockRuntime;
use fil_actors_runtime::ActorDowncast;
use fil_actors_runtime::ActorError;
use fvm_ipld_amt::Amt;
use fvm_ipld_bitfield::BitField;
use fvm_ipld_bitfield::UnvalidatedBitField;
use fvm_ipld_blockstore::MemoryBlockstore;
use fvm_shared::clock::ChainEpoch;
use fvm_shared::clock::QuantSpec;
use fvm_shared::error::ExitCode;
use fvm_shared::sector::SectorSize;
use std::ops::Neg;

mod util;
use crate::util::*;

fn sectors() -> Vec<SectorOnChainInfo> {
    vec![
        test_sector(2, 1, 50, 60, 1000),
        test_sector(3, 2, 51, 61, 1001),
        test_sector(7, 3, 52, 62, 1002),
        test_sector(8, 4, 53, 63, 1003),
        test_sector(11, 5, 54, 64, 1004),
        test_sector(13, 6, 55, 65, 1005),
    ]
}

const SECTOR_SIZE: SectorSize = SectorSize::_32GiB;
const QUANT_SPEC: QuantSpec = QuantSpec { unit: 4, offset: 1 };
const EXP: ChainEpoch = 100;

fn setup() -> (MemoryBlockstore, Partition) {
    let store = MemoryBlockstore::default();
    let mut partition = Partition::new(&store).unwrap();

    let power = partition.add_sectors(&store, true, &sectors(), SECTOR_SIZE, QUANT_SPEC).unwrap();
    let expected_power = power_for_sectors(SECTOR_SIZE, &sectors());
    assert_eq!(expected_power, power);
    (store, partition)
}

#[test]
fn fail_if_all_declared_sectors_are_not_in_the_partition() {
    let (store, mut partition) = setup();
    let sector_arr = sectors_arr(&store, sectors());

    let mut skipped: UnvalidatedBitField = BitField::try_from_bits(1..100).unwrap().into();

    let err: ActorError = partition
        .record_skipped_faults(&store, &sector_arr, SECTOR_SIZE, QUANT_SPEC, EXP, &mut skipped)
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(ExitCode::USR_ILLEGAL_ARGUMENT, err.exit_code());
}

#[test]
fn already_faulty_and_terminated_sectors_are_ignored() {
    let policy = Policy::default();
    let (store, mut partition) = setup();
    let sector_arr = sectors_arr(&store, sectors());

    // terminate 1 AND 2
    let mut terminations: BitField = BitField::try_from_bits([1, 2]).unwrap();
    let termination_epoch = 3;
    partition
        .terminate_sectors(
            &policy,
            &store,
            &sector_arr,
            termination_epoch,
            &mut (terminations.clone().into()),
            SECTOR_SIZE,
            QUANT_SPEC,
        )
        .unwrap();
    assert_partition_state(
        &store,
        &partition,
        QUANT_SPEC,
        SECTOR_SIZE,
        &sectors(),
        &BitField::try_from_bits(1..=6).unwrap(),
        &BitField::new(),
        &BitField::new(),
        &terminations,
        &BitField::new(),
    );

    // declare 4 & 5 as faulty
    let fault_set = BitField::try_from_bits([4, 5]).unwrap();
    partition
        .record_faults(
            &store,
            &sector_arr,
            &mut fault_set.clone().into(),
            7,
            SECTOR_SIZE,
            QUANT_SPEC,
        )
        .unwrap();
    assert_partition_state(
        &store,
        &partition,
        QUANT_SPEC,
        SECTOR_SIZE,
        &sectors(),
        &BitField::try_from_bits(1..=6).unwrap(),
        &fault_set,
        &BitField::new(),
        &terminations,
        &BitField::new(),
    );

    // record skipped faults such that some of them are already faulty/terminated
    let skipped = BitField::try_from_bits([1, 2, 3, 4, 5]).unwrap();
    let (power_delta, new_fault_power, retracted_power, new_faults) = partition
        .record_skipped_faults(
            &store,
            &sector_arr,
            SECTOR_SIZE,
            QUANT_SPEC,
            EXP,
            &mut skipped.clone().into(),
        )
        .unwrap();
    assert!(retracted_power.is_zero());
    let expected_faulty_power = power_for_sectors(
        SECTOR_SIZE,
        &select_sectors(&sectors(), &BitField::try_from_bits([3]).unwrap()).unwrap(),
    );
    assert_eq!(expected_faulty_power, new_fault_power);
    assert_eq!(power_delta, new_fault_power.neg());
    assert!(new_faults);

    assert_partition_state(
        &store,
        &partition,
        QUANT_SPEC,
        SECTOR_SIZE,
        &sectors(),
        &BitField::try_from_bits(1..=6).unwrap(),
        &BitField::try_from_bits([3, 4, 5]).unwrap(),
        &BitField::new(),
        &BitField::try_from_bits([1, 2]).unwrap(),
        &BitField::new(),
    );
}

// 	t.Run("recoveries are retracted without being marked as new faulty power", func(t *testing.T) {
// 		store, partition := setup(t)
// 		sectorArr := sectorsArr(t, store, sectors)

// 		// make 4, 5 and 6 faulty
// 		faultSet := bf(4, 5, 6)
// 		_, _, _, err := partition.RecordFaults(store, sectorArr, faultSet, abi.ChainEpoch(7), sectorSize, quantSpec)
// 		require.NoError(t, err)

// 		// add 4 and 5 as recoveries
// 		recoverSet := bf(4, 5)
// 		err = partition.DeclareFaultsRecovered(sectorArr, sectorSize, recoverSet)
// 		require.NoError(t, err)

// 		assertPartitionState(t, store, partition, quantSpec, sectorSize, sectors, bf(1, 2, 3, 4, 5, 6), bf(4, 5, 6), bf(4, 5), bf(), bf())

// 		// record skipped faults such that some of them have been marked as recovered
// 		skipped := bitfield.NewFromSet([]uint64{1, 4, 5})
// 		powerDelta, newFaultPower, recoveryPower, newFaults, err := partition.RecordSkippedFaults(store, sectorArr, sectorSize, quantSpec, exp, skipped)
// 		require.NoError(t, err)
// 		require.True(t, newFaults)

// 		// only 1 is marked for fault power as 4 & 5 are recovering
// 		expectedFaultyPower := miner.PowerForSectors(sectorSize, selectSectors(t, sectors, bf(1)))
// 		require.EqualValues(t, expectedFaultyPower, newFaultPower)
// 		require.EqualValues(t, expectedFaultyPower.Neg(), powerDelta)

// 		// 4 & 5 are marked for recovery power
// 		expectedRecoveryPower := miner.PowerForSectors(sectorSize, selectSectors(t, sectors, bf(4, 5)))
// 		require.EqualValues(t, expectedRecoveryPower, recoveryPower)

// 		assertPartitionState(t, store, partition, quantSpec, sectorSize, sectors, bf(1, 2, 3, 4, 5, 6), bf(1, 4, 5, 6), bf(), bf(), bf())
// 	})

// 	t.Run("successful when skipped fault set is empty", func(t *testing.T) {
// 		store, partition := setup(t)
// 		sectorArr := sectorsArr(t, store, sectors)

// 		powerDelta, newFaultPower, recoveryPower, newFaults, err := partition.RecordSkippedFaults(store, sectorArr, sectorSize, quantSpec, exp, bf())
// 		require.NoError(t, err)
// 		require.EqualValues(t, miner.NewPowerPairZero(), newFaultPower)
// 		require.EqualValues(t, miner.NewPowerPairZero(), recoveryPower)
// 		require.EqualValues(t, miner.NewPowerPairZero(), powerDelta)
// 		require.False(t, newFaults)

// 		assertPartitionState(t, store, partition, quantSpec, sectorSize, sectors, bf(1, 2, 3, 4, 5, 6), bf(), bf(), bf(), bf())
// 	})
// }

fn assert_partition_state(
    store: &MemoryBlockstore,
    partition: &Partition,
    quant: QuantSpec,
    sector_size: SectorSize,
    sectors: &[SectorOnChainInfo],
    all_sector_ids: &BitField,
    faults: &BitField,
    recovering: &BitField,
    terminations: &BitField,
    unproven: &BitField,
) {
    assert_eq!(faults, &partition.faults);
    assert_eq!(recovering, &partition.recoveries);
    assert_eq!(terminations, &partition.terminated);
    assert_eq!(unproven, &partition.unproven);
    assert_eq!(all_sector_ids, &partition.sectors);

    let msgs = MessageAccumulator::default();
    PartitionStateSummary::check_partition_state_invariants(
        partition,
        store,
        quant,
        sector_size,
        &sectors_as_map(sectors),
        &msgs,
    );
    msgs.assert_empty();
}

pub fn sectors_arr<'a>(
    store: &'a MemoryBlockstore,
    sectors_info: Vec<SectorOnChainInfo>,
) -> Sectors<'a, MemoryBlockstore> {
    let empty_array =
        Amt::<(), _>::new_with_bit_width(store, SECTORS_AMT_BITWIDTH).flush().unwrap();
    let mut sectors = Sectors::load(store, &empty_array).unwrap();
    sectors.store(sectors_info).unwrap();
    sectors
}
