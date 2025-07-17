#[test_only]
module app::app_tests;

use app::main::{create_app, add, multiply, get_value, get_sum};
use sui::clock;
use sui::test_scenario as test;

#[test]
fun test_create_app() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let app = create_app(&clock, test::ctx(&mut scenario));

    // Initial sum should be 0 since no elements exist yet
    assert!(get_sum(&app) == 0, 0);
    // Initial state should be 0 since no state elements exist yet at any index > 0
    assert!(get_value(&app, 1) == 0, 0);
    assert!(get_value(&app, 5) == 0, 0);

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_add_function_single_index() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // Initial state is 0 at index 1, add 5 should make it 5
    add(&mut app, 1, 5, test::ctx(&mut scenario));
    assert!(get_value(&app, 1) == 5, 0);
    assert!(get_sum(&app) == 5, 0); // Sum should be updated

    // Add 10 more to index 1, should be 15
    add(&mut app, 1, 10, test::ctx(&mut scenario));
    assert!(get_value(&app, 1) == 15, 0);
    assert!(get_sum(&app) == 15, 0); // Sum should be updated

    // Other indexes should still be 0
    assert!(get_value(&app, 2) == 0, 0);
    assert!(get_value(&app, 3) == 0, 0);

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_multiply_function_single_index() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // Initial state is 0 at index 1, multiply by 3 should still be 0 (0 * 3 = 0)
    multiply(&mut app, 1, 3, test::ctx(&mut scenario));
    assert!(get_value(&app, 1) == 0, 0);
    assert!(get_sum(&app) == 0, 0); // Sum should remain 0

    // Add 5 first to index 1 to get a non-zero value, then multiply by 2
    add(&mut app, 1, 5, test::ctx(&mut scenario));
    assert!(get_value(&app, 1) == 5, 1);
    assert!(get_sum(&app) == 5, 1);
    multiply(&mut app, 1, 2, test::ctx(&mut scenario));
    assert!(get_value(&app, 1) == 10, 2);
    // Sum calculation: old_sum + new_value - old_value = 5 + 10 - 5 = 10
    assert!(get_sum(&app) == 10, 2);

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_multiple_indexes_sequential() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // Test sequential indexes 1, 2, 3, 4 (index 0 is reserved)
    add(&mut app, 1, 10, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 10, 0);
    add(&mut app, 2, 20, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 30, 0);
    add(&mut app, 3, 30, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 60, 0);
    add(&mut app, 4, 40, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 100, 0);

    assert!(get_value(&app, 1) == 10, 0);
    assert!(get_value(&app, 2) == 20, 0);
    assert!(get_value(&app, 3) == 30, 0);
    assert!(get_value(&app, 4) == 40, 0);

    // Apply multiply operations to sequential indexes
    multiply(&mut app, 1, 2, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 100 + 20 - 10 = 110
    assert!(get_sum(&app) == 110, 0);
    multiply(&mut app, 2, 3, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 110 + 60 - 20 = 150
    assert!(get_sum(&app) == 150, 0);
    multiply(&mut app, 3, 2, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 150 + 60 - 30 = 180
    assert!(get_sum(&app) == 180, 0);
    multiply(&mut app, 4, 2, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 180 + 80 - 40 = 220
    assert!(get_sum(&app) == 220, 0);

    assert!(get_value(&app, 1) == 20, 0); // 10 * 2
    assert!(get_value(&app, 2) == 60, 0); // 20 * 3
    assert!(get_value(&app, 3) == 60, 0); // 30 * 2
    assert!(get_value(&app, 4) == 80, 0); // 40 * 2

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_multiple_indexes_non_sequential() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // Test non-sequential indexes: 5, 2, 10, 1
    add(&mut app, 5, 15, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 15, 0);
    add(&mut app, 2, 25, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 40, 0);
    add(&mut app, 10, 35, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 75, 0);
    add(&mut app, 1, 45, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 120, 0);

    assert!(get_value(&app, 5) == 15, 0);
    assert!(get_value(&app, 2) == 25, 0);
    assert!(get_value(&app, 10) == 35, 0);
    assert!(get_value(&app, 1) == 45, 0);

    // Verify that other indexes are still 0
    assert!(get_value(&app, 3) == 0, 0);
    assert!(get_value(&app, 4) == 0, 0);
    assert!(get_value(&app, 6) == 0, 0);

    // Apply operations to non-sequential indexes in different order
    multiply(&mut app, 10, 2, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 120 + 70 - 35 = 155
    assert!(get_sum(&app) == 155, 0);
    multiply(&mut app, 1, 2, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 155 + 90 - 45 = 200
    assert!(get_sum(&app) == 200, 0);
    add(&mut app, 5, 5, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 205, 0);
    add(&mut app, 2, 10, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 215, 0);

    assert!(get_value(&app, 10) == 70, 0); // 35 * 2
    assert!(get_value(&app, 1) == 90, 0); // 45 * 2
    assert!(get_value(&app, 5) == 20, 0); // 15 + 5
    assert!(get_value(&app, 2) == 35, 0); // 25 + 10

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_mixed_operations_same_index() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // Test multiple operations on the same index
    let index = 7;

    add(&mut app, index, 4, test::ctx(&mut scenario));
    assert!(get_value(&app, index) == 4, 0);
    assert!(get_sum(&app) == 4, 0);

    multiply(&mut app, index, 2, test::ctx(&mut scenario));
    assert!(get_value(&app, index) == 8, 0);
    // Sum calculation: old_sum + new_value - old_value = 4 + 8 - 4 = 8
    assert!(get_sum(&app) == 8, 0);

    add(&mut app, index, 15, test::ctx(&mut scenario));
    assert!(get_value(&app, index) == 23, 0);
    assert!(get_sum(&app) == 23, 0); // 8 + 15

    multiply(&mut app, index, 3, test::ctx(&mut scenario));
    assert!(get_value(&app, index) == 69, 0);
    // Sum calculation: old_sum + new_value - old_value = 23 + 69 - 23 = 69
    assert!(get_sum(&app) == 69, 0);

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_interleaved_operations_multiple_indexes() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // Interleave operations between different indexes
    add(&mut app, 1, 10, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 10, 0);
    add(&mut app, 5, 20, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 30, 0);
    multiply(&mut app, 1, 2, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 30 + 20 - 10 = 40
    assert!(get_sum(&app) == 40, 0);
    add(&mut app, 12, 8, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 48, 0);
    multiply(&mut app, 5, 3, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 48 + 60 - 20 = 88
    assert!(get_sum(&app) == 88, 0);
    add(&mut app, 1, 5, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 93, 0);
    multiply(&mut app, 12, 4, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 93 + 32 - 8 = 117
    assert!(get_sum(&app) == 117, 0);

    assert!(get_value(&app, 1) == 25, 0); // (10 * 2) + 5 = 25
    assert!(get_value(&app, 5) == 60, 0); // 20 * 3 = 60
    assert!(get_value(&app, 12) == 32, 0); // 8 * 4 = 32

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_boundary_values_multiple_indexes() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // Test with value 99 (should work as it's < 100) on different indexes
    add(&mut app, 1, 99, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 99, 0);
    add(&mut app, 100, 99, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 198, 0);
    add(&mut app, 255, 99, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 297, 0);

    assert!(get_value(&app, 1) == 99, 0);
    assert!(get_value(&app, 100) == 99, 0);
    assert!(get_value(&app, 255) == 99, 0);

    // Test multiply by 99 on different indexes
    add(&mut app, 2, 1, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 298, 0);
    add(&mut app, 50, 1, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 299, 0);
    multiply(&mut app, 2, 99, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 299 + 99 - 1 = 397
    assert!(get_sum(&app) == 397, 0);
    multiply(&mut app, 50, 99, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 397 + 99 - 1 = 495
    assert!(get_sum(&app) == 495, 0);

    assert!(get_value(&app, 2) == 99, 0);
    assert!(get_value(&app, 50) == 99, 0);

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
#[expected_failure(abort_code = 2, location = app::main)] // EReservedIndex
fun test_add_reserved_index_0() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // This should fail as index 0 is reserved
    add(&mut app, 0, 5, test::ctx(&mut scenario));

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
#[expected_failure(abort_code = 2, location = app::main)] // EReservedIndex
fun test_multiply_reserved_index_0() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // This should fail as index 0 is reserved
    multiply(&mut app, 0, 2, test::ctx(&mut scenario));

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
#[expected_failure(abort_code = 0, location = app::main)] // EInvalidValue
fun test_add_invalid_value_100() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // This should fail as value >= 100
    add(&mut app, 1, 100, test::ctx(&mut scenario));

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
#[expected_failure(abort_code = 0, location = app::main)] // EInvalidValue
fun test_add_invalid_value_greater_than_100() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // This should fail as value >= 100
    add(&mut app, 42, 150, test::ctx(&mut scenario));

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
#[expected_failure(abort_code = 0, location = app::main)] // EInvalidValue
fun test_multiply_invalid_value_100() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // This should fail as value >= 100
    multiply(&mut app, 10, 100, test::ctx(&mut scenario));

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
#[expected_failure(abort_code = 0, location = app::main)] // EInvalidValue
fun test_multiply_invalid_value_greater_than_100() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // This should fail as value >= 100
    multiply(&mut app, 99, 200, test::ctx(&mut scenario));

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_zero_operations_multiple_indexes() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // Adding 0 should not change the state at different indexes
    add(&mut app, 1, 0, test::ctx(&mut scenario));
    add(&mut app, 15, 0, test::ctx(&mut scenario));
    assert!(get_value(&app, 1) == 0, 0);
    assert!(get_value(&app, 15) == 0, 0);
    assert!(get_sum(&app) == 0, 0);

    // Add non-zero values to different indexes
    add(&mut app, 1, 5, test::ctx(&mut scenario));
    add(&mut app, 15, 7, test::ctx(&mut scenario));
    assert!(get_value(&app, 1) == 5, 0);
    assert!(get_value(&app, 15) == 7, 0);
    assert!(get_sum(&app) == 12, 0);

    // Multiplying by 0 should make state 0 at those indexes
    multiply(&mut app, 1, 0, test::ctx(&mut scenario));
    assert!(get_value(&app, 1) == 0, 0);
    // Sum calculation: old_sum + new_value - old_value = 12 + 0 - 5 = 7
    assert!(get_sum(&app) == 7, 0);
    multiply(&mut app, 15, 0, test::ctx(&mut scenario));
    assert!(get_value(&app, 15) == 0, 0);
    // Sum calculation: old_sum + new_value - old_value = 7 + 0 - 7 = 0
    assert!(get_sum(&app) == 0, 0);

    // Adding to 0 should work at different indexes
    add(&mut app, 1, 42, test::ctx(&mut scenario));
    add(&mut app, 15, 33, test::ctx(&mut scenario));
    assert!(get_value(&app, 1) == 42, 0);
    assert!(get_value(&app, 15) == 33, 0);
    assert!(get_sum(&app) == 75, 0); // 0 + 42 + 33

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_large_index_values() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // Test with large index values (must be < MAX_INDEX = 1,073,741,824)
    let large_index1 = 1000000;
    let large_index2 = 1073741823; // MAX_INDEX - 1

    add(&mut app, large_index1, 25, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 25, 0);
    add(&mut app, large_index2, 50, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 75, 0);

    assert!(get_value(&app, large_index1) == 25, 0);
    assert!(get_value(&app, large_index2) == 50, 0);

    multiply(&mut app, large_index1, 2, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 75 + 50 - 25 = 100
    assert!(get_sum(&app) == 100, 0);
    multiply(&mut app, large_index2, 2, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 100 + 100 - 50 = 150
    assert!(get_sum(&app) == 150, 0);

    assert!(get_value(&app, large_index1) == 50, 0);
    assert!(get_value(&app, large_index2) == 100, 0);

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_sum_functionality() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // Initial sum should be 0
    assert!(get_sum(&app) == 0, 0);

    // Add values and verify sum is updated correctly
    add(&mut app, 1, 10, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 10, 0);

    add(&mut app, 2, 20, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 30, 0);

    add(&mut app, 3, 5, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 35, 0);

    // Test multiply operations affect sum according to the new algorithm
    multiply(&mut app, 1, 2, test::ctx(&mut scenario));
    // Sum calculation: old_sum + new_value - old_value = 35 + 20 - 10 = 45
    assert!(get_sum(&app) == 45, 0);

    // Add more to verify sum continues to work
    add(&mut app, 4, 15, test::ctx(&mut scenario));
    assert!(get_sum(&app) == 60, 0);

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
#[expected_failure(abort_code = 1, location = app::main)] // EIndexTooLarge
fun test_index_too_large() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // This should fail as index >= MAX_INDEX
    add(&mut app, 1073741824, 10, test::ctx(&mut scenario)); // MAX_INDEX

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}

#[test]
fun test_multiply_by_zero_safe() {
    let mut scenario = test::begin(@0x1);
    let clock = clock::create_for_testing(test::ctx(&mut scenario));

    let mut app = create_app(&clock, test::ctx(&mut scenario));

    // Add a non-zero value then multiply by 0 to test the new safe calculation
    add(&mut app, 1, 5, test::ctx(&mut scenario));
    assert!(get_value(&app, 1) == 5, 0);
    assert!(get_sum(&app) == 5, 0);

    // Now multiplying by 0 should work safely with new calculation:
    // new_sum = old_sum + new_value - old_value = 5 + 0 - 5 = 0
    multiply(&mut app, 1, 0, test::ctx(&mut scenario));
    assert!(get_value(&app, 1) == 0, 0);
    assert!(get_sum(&app) == 0, 0);

    sui::transfer::public_transfer(app, @0x1);
    clock::destroy_for_testing(clock);
    scenario.end();
}
