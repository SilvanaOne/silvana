#[test_only]
module add::app_tests;

use add::main::{create_app, init_app_with_instance, add, multiply, get_value, get_sum};
use coordination::app_instance::AppInstance;
use coordination::registry::{SilvanaRegistry, create_registry, add_app};
use std::string::String;
use sui::clock;
use sui::test_scenario as test;

// Helper function to create a test registry with a SilvanaApp
fun setup_test_registry_and_app(
    scenario: &mut test::Scenario,
    clock: &clock::Clock,
) {
    // Create registry
    create_registry(
        b"Test Registry".to_string(),
        test::ctx(scenario),
    );
    test::next_tx(scenario, @0x1);
    
    // Add app to registry
    let mut registry = test::take_shared<SilvanaRegistry>(scenario);
    add_app(
        &mut registry,
        b"add_app".to_string(),
        @0x1, // owner address
        option::some(b"Test application for adding values".to_string()),
        clock,
        test::ctx(scenario),
    );
    
    // Add methods to the app
    let init_method = coordination::app_method::new(
        option::none(),
        b"Developer".to_string(),
        b"Agent".to_string(),
        b"init".to_string(),
    );
    coordination::registry::add_method_to_app(
        &mut registry,
        b"add_app".to_string(),
        b"init".to_string(),
        init_method,
        test::ctx(scenario),
    );
    
    let add_method = coordination::app_method::new(
        option::none(),
        b"Developer".to_string(),
        b"Agent".to_string(),
        b"add".to_string(),
    );
    coordination::registry::add_method_to_app(
        &mut registry,
        b"add_app".to_string(),
        b"add".to_string(),
        add_method,
        test::ctx(scenario),
    );
    
    let multiply_method = coordination::app_method::new(
        option::none(),
        b"Developer".to_string(),
        b"Agent".to_string(),
        b"multiply".to_string(),
    );
    coordination::registry::add_method_to_app(
        &mut registry,
        b"add_app".to_string(),
        b"multiply".to_string(),
        multiply_method,
        test::ctx(scenario),
    );
    
    test::return_shared(registry);
}

#[test]
fun test_create_app() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    setup_test_registry_and_app(&mut scenario, &clock);
    test::next_tx(&mut scenario, @0x1);
    
    let mut registry = test::take_shared<SilvanaRegistry>(&scenario);
    let app = create_app(
        &mut registry,
        vector::empty<String>(), // settlement_chains
        vector::empty<Option<String>>(), // settlement_addresses
        300000, // block_creation_interval_ms (5 minutes)
        &clock,
        test::ctx(&mut scenario)
    );
    test::return_shared(registry);
    
    // After creating app, the AppInstance should be shared
    test::next_tx(&mut scenario, @0x1);
    let mut instance = test::take_shared<AppInstance>(&scenario);
    
    // Initialize the app state
    init_app_with_instance(&app, &mut instance, &clock, test::ctx(&mut scenario));

    // Initial sum should be 0 since no elements exist yet
    assert!(get_sum(&instance) == 0, 0);
    // Initial state should be 0 since no state elements exist yet at any index > 0
    assert!(get_value(&instance, 1) == 0, 0);
    assert!(get_value(&instance, 5) == 0, 0);

    sui::transfer::public_transfer(app, @0x1);
    test::return_shared(instance);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_add_function_single_index() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    setup_test_registry_and_app(&mut scenario, &clock);
    test::next_tx(&mut scenario, @0x1);
    
    let mut registry = test::take_shared<SilvanaRegistry>(&scenario);
    let mut app = create_app(
        &mut registry,
        vector::empty<String>(), // settlement_chains
        vector::empty<Option<String>>(), // settlement_addresses
        300000, // block_creation_interval_ms (5 minutes)
        &clock,
        test::ctx(&mut scenario)
    );
    test::return_shared(registry);
    
    test::next_tx(&mut scenario, @0x1);
    let mut instance = test::take_shared<AppInstance>(&scenario);
    init_app_with_instance(&app, &mut instance, &clock, test::ctx(&mut scenario));

    // Initial state is 0 at index 1, add 5 should make it 5
    add(&mut app, &mut instance, 1, 5, &clock, test::ctx(&mut scenario));
    assert!(get_value(&instance, 1) == 5, 0);
    assert!(get_sum(&instance) == 5, 0); // Sum should be updated

    // Add 10 more to index 1, should be 15
    add(&mut app, &mut instance, 1, 10, &clock, test::ctx(&mut scenario));
    assert!(get_value(&instance, 1) == 15, 0);
    assert!(get_sum(&instance) == 15, 0); // Sum should be updated

    // Other indexes should still be 0
    assert!(get_value(&instance, 2) == 0, 0);
    assert!(get_value(&instance, 3) == 0, 0);

    sui::transfer::public_transfer(app, @0x1);
    test::return_shared(instance);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_multiply_function_single_index() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    setup_test_registry_and_app(&mut scenario, &clock);
    test::next_tx(&mut scenario, @0x1);
    
    let mut registry = test::take_shared<SilvanaRegistry>(&scenario);
    let mut app = create_app(
        &mut registry,
        vector::empty<String>(), // settlement_chains
        vector::empty<Option<String>>(), // settlement_addresses
        300000, // block_creation_interval_ms (5 minutes)
        &clock,
        test::ctx(&mut scenario)
    );
    test::return_shared(registry);
    
    test::next_tx(&mut scenario, @0x1);
    let mut instance = test::take_shared<AppInstance>(&scenario);
    init_app_with_instance(&app, &mut instance, &clock, test::ctx(&mut scenario));

    // Initial state is 0 at index 1, multiply by 3 should still be 0 (0 * 3 = 0)
    multiply(&mut app, &mut instance, 1, 3, &clock, test::ctx(&mut scenario));
    assert!(get_value(&instance, 1) == 0, 0);
    assert!(get_sum(&instance) == 0, 0); // Sum should remain 0

    // Add 5 first to index 1 to get a non-zero value, then multiply by 2
    add(&mut app, &mut instance, 1, 5, &clock, test::ctx(&mut scenario));
    assert!(get_value(&instance, 1) == 5, 1);
    assert!(get_sum(&instance) == 5, 1);
    multiply(&mut app, &mut instance, 1, 2, &clock, test::ctx(&mut scenario));
    assert!(get_value(&instance, 1) == 10, 2);
    // Sum calculation: old_sum + new_value - old_value = 5 + 10 - 5 = 10
    assert!(get_sum(&instance) == 10, 2);

    sui::transfer::public_transfer(app, @0x1);
    test::return_shared(instance);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_multiple_indexes_sequential() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    setup_test_registry_and_app(&mut scenario, &clock);
    test::next_tx(&mut scenario, @0x1);
    
    let mut registry = test::take_shared<SilvanaRegistry>(&scenario);
    let mut app = create_app(
        &mut registry,
        vector::empty<String>(), // settlement_chains
        vector::empty<Option<String>>(), // settlement_addresses
        300000, // block_creation_interval_ms (5 minutes)
        &clock,
        test::ctx(&mut scenario)
    );
    test::return_shared(registry);
    
    test::next_tx(&mut scenario, @0x1);
    let mut instance = test::take_shared<AppInstance>(&scenario);
    init_app_with_instance(&app, &mut instance, &clock, test::ctx(&mut scenario));

    // Test sequential indexes 1, 2, 3, 4 (index 0 is reserved)
    add(&mut app, &mut instance, 1, 10, &clock, test::ctx(&mut scenario));
    assert!(get_sum(&instance) == 10, 0);
    add(&mut app, &mut instance, 2, 20, &clock, test::ctx(&mut scenario));
    assert!(get_sum(&instance) == 30, 0);
    add(&mut app, &mut instance, 3, 30, &clock, test::ctx(&mut scenario));
    assert!(get_sum(&instance) == 60, 0);
    add(&mut app, &mut instance, 4, 40, &clock, test::ctx(&mut scenario));
    assert!(get_sum(&instance) == 100, 0);

    assert!(get_value(&instance, 1) == 10, 0);
    assert!(get_value(&instance, 2) == 20, 0);
    assert!(get_value(&instance, 3) == 30, 0);
    assert!(get_value(&instance, 4) == 40, 0);

    // Apply multiply operations to sequential indexes
    multiply(&mut app, &mut instance, 1, 2, &clock, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 100 + 20 - 10 = 110
    assert!(get_sum(&instance) == 110, 0);
    multiply(&mut app, &mut instance, 2, 3, &clock, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 110 + 60 - 20 = 150
    assert!(get_sum(&instance) == 150, 0);
    multiply(&mut app, &mut instance, 3, 2, &clock, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 150 + 60 - 30 = 180
    assert!(get_sum(&instance) == 180, 0);
    multiply(&mut app, &mut instance, 4, 2, &clock, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 180 + 80 - 40 = 220
    assert!(get_sum(&instance) == 220, 0);

    assert!(get_value(&instance, 1) == 20, 0); // 10 * 2
    assert!(get_value(&instance, 2) == 60, 0); // 20 * 3
    assert!(get_value(&instance, 3) == 60, 0); // 30 * 2
    assert!(get_value(&instance, 4) == 80, 0); // 40 * 2

    sui::transfer::public_transfer(app, @0x1);
    test::return_shared(instance);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
#[expected_failure(abort_code = 2, location = add::main)] // EReservedIndex
fun test_add_reserved_index_0() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    setup_test_registry_and_app(&mut scenario, &clock);
    test::next_tx(&mut scenario, @0x1);
    
    let mut registry = test::take_shared<SilvanaRegistry>(&scenario);
    let mut app = create_app(
        &mut registry,
        vector::empty<String>(), // settlement_chains
        vector::empty<Option<String>>(), // settlement_addresses
        300000, // block_creation_interval_ms (5 minutes)
        &clock,
        test::ctx(&mut scenario)
    );
    test::return_shared(registry);
    
    test::next_tx(&mut scenario, @0x1);
    let mut instance = test::take_shared<AppInstance>(&scenario);
    init_app_with_instance(&app, &mut instance, &clock, test::ctx(&mut scenario));

    // This should fail as index 0 is reserved
    add(&mut app, &mut instance, 0, 5, &clock, test::ctx(&mut scenario));

    sui::transfer::public_transfer(app, @0x1);
    test::return_shared(instance);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
#[expected_failure(abort_code = 0, location = add::main)] // EInvalidValue  
fun test_add_invalid_value_100() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    setup_test_registry_and_app(&mut scenario, &clock);
    test::next_tx(&mut scenario, @0x1);
    
    let mut registry = test::take_shared<SilvanaRegistry>(&scenario);
    let mut app = create_app(
        &mut registry,
        vector::empty<String>(), // settlement_chains
        vector::empty<Option<String>>(), // settlement_addresses
        300000, // block_creation_interval_ms (5 minutes)
        &clock,
        test::ctx(&mut scenario)
    );
    test::return_shared(registry);
    
    test::next_tx(&mut scenario, @0x1);
    let mut instance = test::take_shared<AppInstance>(&scenario);
    init_app_with_instance(&app, &mut instance, &clock, test::ctx(&mut scenario));

    // This should fail as value >= 100
    add(&mut app, &mut instance, 1, 100, &clock, test::ctx(&mut scenario));

    sui::transfer::public_transfer(app, @0x1);
    test::return_shared(instance);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_zero_operations() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    setup_test_registry_and_app(&mut scenario, &clock);
    test::next_tx(&mut scenario, @0x1);
    
    let mut registry = test::take_shared<SilvanaRegistry>(&scenario);
    let mut app = create_app(
        &mut registry,
        vector::empty<String>(), // settlement_chains
        vector::empty<Option<String>>(), // settlement_addresses
        300000, // block_creation_interval_ms (5 minutes)
        &clock,
        test::ctx(&mut scenario)
    );
    test::return_shared(registry);
    
    test::next_tx(&mut scenario, @0x1);
    let mut instance = test::take_shared<AppInstance>(&scenario);
    init_app_with_instance(&app, &mut instance, &clock, test::ctx(&mut scenario));

    // Adding 0 should not change the state
    add(&mut app, &mut instance, 1, 0, &clock, test::ctx(&mut scenario));
    assert!(get_value(&instance, 1) == 0, 0);
    assert!(get_sum(&instance) == 0, 0);

    // Add non-zero value
    add(&mut app, &mut instance, 1, 5, &clock, test::ctx(&mut scenario));
    assert!(get_value(&instance, 1) == 5, 0);
    assert!(get_sum(&instance) == 5, 0);

    // Multiplying by 0 should make state 0
    multiply(&mut app, &mut instance, 1, 0, &clock, test::ctx(&mut scenario));
    assert!(get_value(&instance, 1) == 0, 0);
    assert!(get_sum(&instance) == 0, 0);

    sui::transfer::public_transfer(app, @0x1);
    test::return_shared(instance);
    clock::destroy_for_testing(clock);
    scenario.end();
}