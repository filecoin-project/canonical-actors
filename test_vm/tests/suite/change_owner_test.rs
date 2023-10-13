use fil_actors_integration_tests::tests::{
    change_owner_fail_test, change_owner_success_test, keep_beneficiary_when_owner_changed_test,
};
use fil_actors_runtime::test_blockstores::TrackingMemBlockstore;
use test_vm::TestVM;

#[test]
fn change_owner_success() {
    let store = TrackingMemBlockstore::new();
    let v = TestVM::<TrackingMemBlockstore>::new_with_singletons(&store);
    change_owner_success_test(&v);
}

#[test]
fn keep_beneficiary_when_owner_changed() {
    let store = TrackingMemBlockstore::new();
    let v = TestVM::<TrackingMemBlockstore>::new_with_singletons(&store);
    keep_beneficiary_when_owner_changed_test(&v);
}

#[test]
fn change_owner_fail() {
    let store = TrackingMemBlockstore::new();
    let v = TestVM::<TrackingMemBlockstore>::new_with_singletons(&store);
    change_owner_fail_test(&v);
}
