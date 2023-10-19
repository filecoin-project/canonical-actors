use fil_actors_integration_tests::tests::batch_onboarding_deals_test;
use fil_actors_runtime::test_blockstores::MemoryBlockstore;
use std::rc::Rc;
use test_vm::TestVM;

#[test]
fn batch_onboarding_deals() {
    let store = MemoryBlockstore::new();
    let v = TestVM::new_with_singletons(Rc::new(store));
    batch_onboarding_deals_test(&v);
}
