#[test_only]
module coordination::access_tests;

use coordination::registry::{Self, SilvanaRegistry};
use sui::clock::{Self, Clock};
use sui::test_scenario::{Self as ts, Scenario};

const ADMIN: address = @0xAD;
const DEVELOPER1: address = @0xDE1;
const DEVELOPER2: address = @0xDE2;
const ATTACKER: address = @0xBAD;

fun setup_registry(scenario: &mut Scenario, _clock: &Clock) {
    ts::next_tx(scenario, ADMIN);
    {
        registry::create_registry(b"Test Registry".to_string(), ts::ctx(scenario));
    };
}

fun setup_developer_and_agent(scenario: &mut Scenario, clock: &Clock) {
    ts::next_tx(scenario, DEVELOPER1);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(scenario);
        
        // Developer adds themselves
        registry::add_developer(
            &mut registry,
            DEVELOPER1,
            b"developer1".to_string(),
            b"github1".to_string(),
            option::some(b"image1".to_string()),
            option::some(b"desc1".to_string()),
            option::some(b"site1".to_string()),
            clock,
            ts::ctx(scenario),
        );
        
        // Developer adds an agent
        registry::add_agent(
            &mut registry,
            b"developer1".to_string(),
            b"agent1".to_string(),
            option::some(b"agent_image".to_string()),
            option::some(b"agent_desc".to_string()),
            option::some(b"agent_site".to_string()),
            vector[b"sui".to_string()],
            clock,
            ts::ctx(scenario),
        );
        
        ts::return_shared(registry);
    };
}

// Test: Developer can update their own developer profile
#[test]
fun test_developer_can_update_own_profile() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    setup_developer_and_agent(&mut scenario, &test_clock);
    
    // Developer updates their own profile
    ts::next_tx(&mut scenario, DEVELOPER1);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::update_developer(
            &mut registry,
            b"developer1".to_string(),
            b"new_github".to_string(),
            option::some(b"new_image".to_string()),
            option::some(b"new_desc".to_string()),
            option::some(b"new_site".to_string()),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Admin can update any developer profile
#[test]
fun test_admin_can_update_any_developer() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    setup_developer_and_agent(&mut scenario, &test_clock);
    
    // Admin updates developer's profile
    ts::next_tx(&mut scenario, ADMIN);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::update_developer(
            &mut registry,
            b"developer1".to_string(),
            b"admin_updated_github".to_string(),
            option::some(b"admin_image".to_string()),
            option::some(b"admin_desc".to_string()),
            option::some(b"admin_site".to_string()),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Non-owner non-admin cannot update developer profile
#[test]
#[expected_failure(abort_code = registry::ENotAuthorized)]
fun test_attacker_cannot_update_developer() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    setup_developer_and_agent(&mut scenario, &test_clock);
    
    // Attacker tries to update developer's profile
    ts::next_tx(&mut scenario, ATTACKER);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::update_developer(
            &mut registry,
            b"developer1".to_string(),
            b"hacked_github".to_string(),
            option::some(b"hacked_image".to_string()),
            option::some(b"hacked_desc".to_string()),
            option::some(b"hacked_site".to_string()),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Developer can update their own agent
#[test]
fun test_developer_can_update_own_agent() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    setup_developer_and_agent(&mut scenario, &test_clock);
    
    // Developer updates their own agent
    ts::next_tx(&mut scenario, DEVELOPER1);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::update_agent(
            &mut registry,
            b"developer1".to_string(),
            b"agent1".to_string(),
            option::some(b"updated_image".to_string()),
            option::some(b"updated_desc".to_string()),
            option::some(b"updated_site".to_string()),
            vector[b"sui".to_string(), b"eth".to_string()],
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Admin can update any agent
#[test]
fun test_admin_can_update_any_agent() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    setup_developer_and_agent(&mut scenario, &test_clock);
    
    // Admin updates developer's agent
    ts::next_tx(&mut scenario, ADMIN);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::update_agent(
            &mut registry,
            b"developer1".to_string(),
            b"agent1".to_string(),
            option::some(b"admin_updated_image".to_string()),
            option::some(b"admin_updated_desc".to_string()),
            option::some(b"admin_updated_site".to_string()),
            vector[b"sui".to_string(), b"polygon".to_string()],
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Non-owner non-admin cannot update agent
#[test]
#[expected_failure(abort_code = registry::ENotAuthorized)]
fun test_attacker_cannot_update_agent() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    setup_developer_and_agent(&mut scenario, &test_clock);
    
    // Attacker tries to update agent
    ts::next_tx(&mut scenario, ATTACKER);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::update_agent(
            &mut registry,
            b"developer1".to_string(),
            b"agent1".to_string(),
            option::some(b"hacked_image".to_string()),
            option::some(b"hacked_desc".to_string()),
            option::some(b"hacked_site".to_string()),
            vector[b"malicious".to_string()],
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Developer can add method to their agent
#[test]
fun test_developer_can_add_method_to_own_agent() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    setup_developer_and_agent(&mut scenario, &test_clock);
    
    // Developer adds method to their agent
    ts::next_tx(&mut scenario, DEVELOPER1);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::add_method(
            &mut registry,
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"docker:latest".to_string(),
            option::some(b"sha256:abc123".to_string()),
            4,
            2,
            false,
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Admin can add method to any agent
#[test]
fun test_admin_can_add_method_to_any_agent() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    setup_developer_and_agent(&mut scenario, &test_clock);
    
    // Admin adds method to developer's agent
    ts::next_tx(&mut scenario, ADMIN);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::add_method(
            &mut registry,
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"admin_method".to_string(),
            b"admin_docker:latest".to_string(),
            option::some(b"sha256:def456".to_string()),
            8,
            4,
            true,
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Non-owner non-admin cannot add method
#[test]
#[expected_failure(abort_code = registry::ENotAuthorized)]
fun test_attacker_cannot_add_method() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    setup_developer_and_agent(&mut scenario, &test_clock);
    
    // Attacker tries to add method
    ts::next_tx(&mut scenario, ATTACKER);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::add_method(
            &mut registry,
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"malicious_method".to_string(),
            b"malicious:latest".to_string(),
            option::none(),
            1,
            1,
            false,
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Developer can remove agent
#[test]
fun test_developer_can_remove_own_agent() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    setup_developer_and_agent(&mut scenario, &test_clock);
    
    // Developer removes their own agent
    ts::next_tx(&mut scenario, DEVELOPER1);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::remove_agent(
            &mut registry,
            b"developer1".to_string(),
            b"agent1".to_string(),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Admin can remove any agent
#[test]
fun test_admin_can_remove_any_agent() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    setup_developer_and_agent(&mut scenario, &test_clock);
    
    // Admin removes developer's agent
    ts::next_tx(&mut scenario, ADMIN);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::remove_agent(
            &mut registry,
            b"developer1".to_string(),
            b"agent1".to_string(),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Non-owner non-admin cannot remove agent
#[test]
#[expected_failure(abort_code = registry::ENotAuthorized)]
fun test_attacker_cannot_remove_agent() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    setup_developer_and_agent(&mut scenario, &test_clock);
    
    // Attacker tries to remove agent
    ts::next_tx(&mut scenario, ATTACKER);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::remove_agent(
            &mut registry,
            b"developer1".to_string(),
            b"agent1".to_string(),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: App owner can update their app
#[test]
fun test_app_owner_can_update_own_app() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    
    // Create an app
    ts::next_tx(&mut scenario, DEVELOPER1);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::add_app(
            &mut registry,
            b"app1".to_string(),
            DEVELOPER1,
            option::some(b"app description".to_string()),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    // App owner updates their app
    ts::next_tx(&mut scenario, DEVELOPER1);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::update_app(
            &mut registry,
            b"app1".to_string(),
            option::some(b"updated app description".to_string()),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Admin can update any app
#[test]
fun test_admin_can_update_any_app() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    
    // Create an app
    ts::next_tx(&mut scenario, DEVELOPER1);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::add_app(
            &mut registry,
            b"app1".to_string(),
            DEVELOPER1,
            option::some(b"app description".to_string()),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    // Admin updates the app
    ts::next_tx(&mut scenario, ADMIN);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::update_app(
            &mut registry,
            b"app1".to_string(),
            option::some(b"admin updated description".to_string()),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Non-owner non-admin cannot update app
#[test]
#[expected_failure(abort_code = registry::ENotAuthorized)]
fun test_attacker_cannot_update_app() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    
    // Create an app
    ts::next_tx(&mut scenario, DEVELOPER1);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::add_app(
            &mut registry,
            b"app1".to_string(),
            DEVELOPER1,
            option::some(b"app description".to_string()),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    // Attacker tries to update the app
    ts::next_tx(&mut scenario, ATTACKER);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::update_app(
            &mut registry,
            b"app1".to_string(),
            option::some(b"hacked description".to_string()),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Verify spoofing admin_address doesn't work
// This test verifies that even if someone tries to call internal functions directly
// with a spoofed admin_address, they cannot bypass security
#[test]
#[expected_failure(abort_code = registry::ENotAuthorized)]
fun test_cannot_spoof_admin_address() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    setup_developer_and_agent(&mut scenario, &test_clock);
    
    // Attacker tries to call internal function directly with admin address
    ts::next_tx(&mut scenario, ATTACKER);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        // This should fail because even though ADMIN is passed as admin_address,
        // the actual sender (ATTACKER) is neither the owner nor the admin
        // The registry functions properly pass registry.admin, preventing this attack
        
        // Since we can't call internal functions directly from tests,
        // we test through the registry which properly validates
        registry::update_developer(
            &mut registry,
            b"developer1".to_string(),
            b"spoofed_github".to_string(),
            option::some(b"spoofed_image".to_string()),
            option::some(b"spoofed_desc".to_string()),
            option::some(b"spoofed_site".to_string()),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}

// Test: Multiple developers cannot interfere with each other
#[test]
#[expected_failure(abort_code = registry::ENotAuthorized)]
fun test_developer_cannot_update_other_developer() {
    let mut scenario = ts::begin(ADMIN);
    let test_clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    setup_registry(&mut scenario, &test_clock);
    setup_developer_and_agent(&mut scenario, &test_clock);
    
    // Add second developer
    ts::next_tx(&mut scenario, DEVELOPER2);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::add_developer(
            &mut registry,
            DEVELOPER2,
            b"developer2".to_string(),
            b"github2".to_string(),
            option::some(b"image2".to_string()),
            option::some(b"desc2".to_string()),
            option::some(b"site2".to_string()),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    // Developer2 tries to update Developer1's profile
    ts::next_tx(&mut scenario, DEVELOPER2);
    {
        let mut registry = ts::take_shared<SilvanaRegistry>(&scenario);
        
        registry::update_developer(
            &mut registry,
            b"developer1".to_string(),
            b"unauthorized_github".to_string(),
            option::some(b"unauthorized_image".to_string()),
            option::some(b"unauthorized_desc".to_string()),
            option::some(b"unauthorized_site".to_string()),
            &test_clock,
            ts::ctx(&mut scenario),
        );
        
        ts::return_shared(registry);
    };
    
    clock::destroy_for_testing(test_clock);
    ts::end(scenario);
}