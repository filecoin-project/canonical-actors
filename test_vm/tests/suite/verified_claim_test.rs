use fil_actors_integration_tests::tests::{
    deal_passes_claim_fails_test, expired_allocations_test, verified_claim_scenario_test,
};
use fil_actors_runtime::test_blockstores::TrackingMemBlockstore;
use test_vm::TestVM;

// Tests a scenario involving a verified deal from the built-in market, with associated
// allocation and claim.
// This test shares some set-up copied from extend_sectors_test.
#[test]
fn verified_claim_scenario() {
    let store = TrackingMemBlockstore::new();
    let v = TestVM::<TrackingMemBlockstore>::new_with_singletons(&store);
    verified_claim_scenario_test(&v);
}

#[test]
fn expired_allocations() {
    let store = TrackingMemBlockstore::new();
    let v = TestVM::<TrackingMemBlockstore>::new_with_singletons(&store);
    expired_allocations_test(&v);
}

#[test]
fn deal_passes_claim_fails() {
    let store = TrackingMemBlockstore::new();
    let v = TestVM::<TrackingMemBlockstore>::new_with_singletons(&store);
    deal_passes_claim_fails_test(&v);
}
