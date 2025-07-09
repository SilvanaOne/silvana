#[test_only]
module coordination::registry_test;

use coordination::registry;
use std::string;
use sui::clock;
use sui::test_scenario::{Self as test, next_tx, ctx};

const ADMIN: address = @0xa;
const DEVELOPER: address = @0xb;
const UNAUTHORIZED: address = @0xc;

#[test]
public fun test_create_registry_add_developer_and_agent() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Test 1: Create Registry
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Silvana Agent Registry"),
            ctx(&mut scenario),
        );
    };

    // Test 2: Add Developer
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"alice"),
            string::utf8(b"alice-github"),
            option::some(string::utf8(b"https://avatar.com/alice.png")),
            option::some(
                string::utf8(b"AI developer specializing in smart contracts"),
            ),
            option::some(string::utf8(b"https://alice.dev")),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Test 3: Add Agent
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"alice"),
            string::utf8(b"trading-bot"),
            option::some(string::utf8(b"https://avatars.com/trading-bot.png")),
            option::some(
                string::utf8(b"Automated trading agent for DeFi protocols"),
            ),
            option::some(string::utf8(b"https://trading-bot.alice.dev")),
            vector[
                string::utf8(b"ethereum"),
                string::utf8(b"sui"),
                string::utf8(b"polygon"),
            ],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Test 4: Verify Agent Creation
    {
        next_tx(&mut scenario, DEVELOPER);
        let registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        let (developer_obj, agent_obj) = registry::get_agent(
            &registry_obj,
            string::utf8(b"alice"),
            string::utf8(b"trading-bot"),
        );

        // Verify developer exists and has correct properties
        assert!(
            coordination::developer::developer_name(developer_obj) == string::utf8(b"alice"),
        );
        assert!(
            coordination::developer::developer_owner(developer_obj) == DEVELOPER,
        );

        // Verify agent exists and has correct properties
        assert!(
            coordination::agent::agent_name(agent_obj) == string::utf8(b"trading-bot"),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
public fun test_add_multiple_developers_and_agents() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Create Registry
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Multi-Developer Registry"),
            ctx(&mut scenario),
        );
    };

    // Add First Developer
    {
        next_tx(&mut scenario, @0xc);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"bob"),
            string::utf8(b"bob-github"),
            option::none(),
            option::some(string::utf8(b"DeFi protocol developer")),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Add Second Developer
    {
        next_tx(&mut scenario, @0xd);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"charlie"),
            string::utf8(b"charlie-github"),
            option::some(string::utf8(b"https://charlie.com/avatar.jpg")),
            option::some(string::utf8(b"NFT marketplace specialist")),
            option::some(string::utf8(b"https://charlie.nft")),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Bob adds an agent
    {
        next_tx(&mut scenario, @0xc);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"bob"),
            string::utf8(b"yield-farmer"),
            option::some(string::utf8(b"https://yield.farm/logo.png")),
            option::some(
                string::utf8(b"Automated yield farming across protocols"),
            ),
            option::some(string::utf8(b"https://yield.farm")),
            vector[string::utf8(b"ethereum"), string::utf8(b"bsc")],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Charlie adds an agent
    {
        next_tx(&mut scenario, @0xd);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"charlie"),
            string::utf8(b"nft-monitor"),
            option::some(string::utf8(b"https://nft.monitor/icon.png")),
            option::some(
                string::utf8(b"Monitors NFT collections for rare drops"),
            ),
            option::some(string::utf8(b"https://nft.monitor")),
            vector[
                string::utf8(b"ethereum"),
                string::utf8(b"solana"),
                string::utf8(b"sui"),
            ],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Verify both agents exist
    {
        next_tx(&mut scenario, ADMIN);
        let registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        // Check Bob's agent
        let (bob_dev, bob_agent) = registry::get_agent(
            &registry_obj,
            string::utf8(b"bob"),
            string::utf8(b"yield-farmer"),
        );
        assert!(
            coordination::developer::developer_name(bob_dev) == string::utf8(b"bob"),
        );
        assert!(
            coordination::agent::agent_name(bob_agent) == string::utf8(b"yield-farmer"),
        );

        // Check Charlie's agent
        let (charlie_dev, charlie_agent) = registry::get_agent(
            &registry_obj,
            string::utf8(b"charlie"),
            string::utf8(b"nft-monitor"),
        );
        assert!(
            coordination::developer::developer_name(charlie_dev) == string::utf8(b"charlie"),
        );
        assert!(
            coordination::agent::agent_name(charlie_agent) == string::utf8(b"nft-monitor"),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
public fun test_agent_method_management() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Setup: Create Registry and Developer
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Method Test Registry"),
            ctx(&mut scenario),
        );
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"dev1"),
            string::utf8(b"dev1-github"),
            option::none(),
            option::some(string::utf8(b"Method testing developer")),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"dev1"),
            string::utf8(b"test-agent"),
            option::none(),
            option::some(string::utf8(b"Agent for testing methods")),
            option::none(),
            vector[string::utf8(b"sui")],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Test: Add Method
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_method(
            &mut registry_obj,
            string::utf8(b"dev1"),
            string::utf8(b"test-agent"),
            string::utf8(b"docker_method"),
            string::utf8(b"myregistry/agent:latest"),
            option::some(string::utf8(b"sha256:abc123")),
            4, // 4GB memory
            2, // 2 CPU cores
            false, // no TEE required
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Test: Set Default Method
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::set_default_method(
            &mut registry_obj,
            string::utf8(b"dev1"),
            string::utf8(b"test-agent"),
            string::utf8(b"docker_method"),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Test: Update Method
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::update_method(
            &mut registry_obj,
            string::utf8(b"dev1"),
            string::utf8(b"test-agent"),
            string::utf8(b"docker_method"),
            string::utf8(b"myregistry/agent:v2.0"),
            option::some(string::utf8(b"sha256:def456")),
            8, // 8GB memory
            4, // 4 CPU cores
            true, // TEE required
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
#[expected_failure(abort_code = coordination::registry::ENotAdmin)]
public fun test_non_admin_cannot_remove_developer() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Create Registry
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Admin Test Registry"),
            ctx(&mut scenario),
        );
    };

    // Add Developer
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"testdev"),
            string::utf8(b"testdev-github"),
            option::none(),
            option::none(),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Try to remove developer as non-admin (should fail)
    {
        next_tx(&mut scenario, DEVELOPER); // Not admin
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::remove_developer(
            &mut registry_obj,
            string::utf8(b"testdev"),
            vector::empty(),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
public fun test_update_developer() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Create Registry
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Update Developer Test"),
            ctx(&mut scenario),
        );
    };

    // Add Developer
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"updatable_dev"),
            string::utf8(b"old-github"),
            option::some(string::utf8(b"https://old-avatar.com/image.png")),
            option::some(string::utf8(b"Old description")),
            option::some(string::utf8(b"https://old-site.com")),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Update Developer
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::update_developer(
            &mut registry_obj,
            string::utf8(b"updatable_dev"),
            string::utf8(b"new-github"),
            option::some(string::utf8(b"https://new-avatar.com/image.png")),
            option::some(string::utf8(b"Updated description with new details")),
            option::some(string::utf8(b"https://new-site.com")),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Add an agent to verify developer update
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"updatable_dev"),
            string::utf8(b"test_agent"),
            option::none(),
            option::some(
                string::utf8(b"Test agent for developer verification"),
            ),
            option::none(),
            vector[string::utf8(b"sui")],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Verify Update
    {
        next_tx(&mut scenario, DEVELOPER);
        let registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        let (developer_obj, _) = registry::get_agent(
            &registry_obj,
            string::utf8(b"updatable_dev"),
            string::utf8(b"test_agent"),
        );

        // Developer should still have the same name and owner
        assert!(
            coordination::developer::developer_name(developer_obj) == string::utf8(b"updatable_dev"),
        );
        assert!(
            coordination::developer::developer_owner(developer_obj) == DEVELOPER,
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
public fun test_update_agent() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Setup
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Update Agent Test"),
            ctx(&mut scenario),
        );
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"agent_dev"),
            string::utf8(b"agent-dev-github"),
            option::none(),
            option::none(),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Add Agent
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"agent_dev"),
            string::utf8(b"updatable_agent"),
            option::some(string::utf8(b"https://old-agent.com/logo.png")),
            option::some(string::utf8(b"Old agent description")),
            option::some(string::utf8(b"https://old-agent.com")),
            vector[string::utf8(b"ethereum")],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Update Agent
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::update_agent(
            &mut registry_obj,
            string::utf8(b"agent_dev"),
            string::utf8(b"updatable_agent"),
            option::some(string::utf8(b"https://new-agent.com/logo.png")),
            option::some(string::utf8(b"Updated agent with new capabilities")),
            option::some(string::utf8(b"https://new-agent.com")),
            vector[
                string::utf8(b"ethereum"),
                string::utf8(b"polygon"),
                string::utf8(b"avalanche"),
            ],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Verify Update
    {
        next_tx(&mut scenario, DEVELOPER);
        let registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        let (_, agent_obj) = registry::get_agent(
            &registry_obj,
            string::utf8(b"agent_dev"),
            string::utf8(b"updatable_agent"),
        );

        // Agent should still have the same name
        assert!(
            coordination::agent::agent_name(agent_obj) == string::utf8(b"updatable_agent"),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
public fun test_remove_agent() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Setup
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Remove Agent Test"),
            ctx(&mut scenario),
        );
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"remove_dev"),
            string::utf8(b"remove-dev-github"),
            option::none(),
            option::none(),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Add two agents
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"remove_dev"),
            string::utf8(b"agent_to_keep"),
            option::none(),
            option::some(string::utf8(b"This agent will stay")),
            option::none(),
            vector[string::utf8(b"sui")],
            &clock,
            ctx(&mut scenario),
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"remove_dev"),
            string::utf8(b"agent_to_remove"),
            option::none(),
            option::some(string::utf8(b"This agent will be removed")),
            option::none(),
            vector[string::utf8(b"ethereum")],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Verify both agents exist
    {
        next_tx(&mut scenario, DEVELOPER);
        let registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        let (_, keep_agent) = registry::get_agent(
            &registry_obj,
            string::utf8(b"remove_dev"),
            string::utf8(b"agent_to_keep"),
        );
        assert!(
            coordination::agent::agent_name(keep_agent) == string::utf8(b"agent_to_keep"),
        );

        let (_, remove_agent) = registry::get_agent(
            &registry_obj,
            string::utf8(b"remove_dev"),
            string::utf8(b"agent_to_remove"),
        );
        assert!(
            coordination::agent::agent_name(remove_agent) == string::utf8(b"agent_to_remove"),
        );

        test::return_shared(registry_obj);
    };

    // Remove one agent
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::remove_agent(
            &mut registry_obj,
            string::utf8(b"remove_dev"),
            string::utf8(b"agent_to_remove"),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Verify only one agent remains
    {
        next_tx(&mut scenario, DEVELOPER);
        let registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        // Should still be able to get the kept agent
        let (_, keep_agent) = registry::get_agent(
            &registry_obj,
            string::utf8(b"remove_dev"),
            string::utf8(b"agent_to_keep"),
        );
        assert!(
            coordination::agent::agent_name(keep_agent) == string::utf8(b"agent_to_keep"),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
public fun test_method_removal_and_default_handling() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Setup
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Method Removal Test"),
            ctx(&mut scenario),
        );
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method-dev-github"),
            option::none(),
            option::none(),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            option::none(),
            option::some(string::utf8(b"Agent for method testing")),
            option::none(),
            vector[string::utf8(b"sui")],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Add multiple methods
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_method(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            string::utf8(b"method_a"),
            string::utf8(b"registry/method-a:v1"),
            option::some(string::utf8(b"sha256:aaa111")),
            2,
            1,
            false,
            &clock,
            ctx(&mut scenario),
        );

        registry::add_method(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            string::utf8(b"method_b"),
            string::utf8(b"registry/method-b:v1"),
            option::some(string::utf8(b"sha256:bbb222")),
            4,
            2,
            true,
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Set default method
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::set_default_method(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            string::utf8(b"method_a"),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Remove non-default method
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::remove_method(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            string::utf8(b"method_b"),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Remove default method
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::remove_default_method(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Remove remaining method
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::remove_method(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            string::utf8(b"method_a"),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
public fun test_admin_remove_developer_success() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Create Registry
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Admin Remove Test"),
            ctx(&mut scenario),
        );
    };

    // Add Developer with Agent
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"removable_dev"),
            string::utf8(b"removable-github"),
            option::none(),
            option::none(),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"removable_dev"),
            string::utf8(b"removable_agent"),
            option::none(),
            option::none(),
            option::none(),
            vector[string::utf8(b"sui")],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Admin removes developer (should succeed)
    {
        next_tx(&mut scenario, ADMIN);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::remove_developer(
            &mut registry_obj,
            string::utf8(b"removable_dev"),
            vector[string::utf8(b"removable_agent")], // List agent names for cleanup
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
public fun test_comprehensive_workflow() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // 1. Create Registry
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Comprehensive Test Registry"),
            ctx(&mut scenario),
        );
    };

    // 2. Add Developer
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"workflow_dev"),
            string::utf8(b"workflow-github"),
            option::some(string::utf8(b"https://avatar.com/workflow.png")),
            option::some(string::utf8(b"Full workflow developer")),
            option::some(string::utf8(b"https://workflow.dev")),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // 3. Add Agent
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"workflow_dev"),
            string::utf8(b"workflow_agent"),
            option::some(string::utf8(b"https://agent.com/workflow.png")),
            option::some(string::utf8(b"Comprehensive workflow agent")),
            option::some(string::utf8(b"https://agent.workflow.dev")),
            vector[string::utf8(b"sui"), string::utf8(b"ethereum")],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // 4. Add Multiple Methods
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_method(
            &mut registry_obj,
            string::utf8(b"workflow_dev"),
            string::utf8(b"workflow_agent"),
            string::utf8(b"cpu_method"),
            string::utf8(b"workflow/cpu:latest"),
            option::some(string::utf8(b"sha256:cpu123")),
            4,
            2,
            false,
            &clock,
            ctx(&mut scenario),
        );

        registry::add_method(
            &mut registry_obj,
            string::utf8(b"workflow_dev"),
            string::utf8(b"workflow_agent"),
            string::utf8(b"tee_method"),
            string::utf8(b"workflow/tee:latest"),
            option::some(string::utf8(b"sha256:tee456")),
            8,
            4,
            true,
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // 5. Set and Change Default Method
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::set_default_method(
            &mut registry_obj,
            string::utf8(b"workflow_dev"),
            string::utf8(b"workflow_agent"),
            string::utf8(b"cpu_method"),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // 6. Update Developer
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::update_developer(
            &mut registry_obj,
            string::utf8(b"workflow_dev"),
            string::utf8(b"new-workflow-github"),
            option::some(string::utf8(b"https://new-avatar.com/workflow.png")),
            option::some(string::utf8(b"Updated workflow developer")),
            option::some(string::utf8(b"https://new-workflow.dev")),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // 7. Update Agent
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::update_agent(
            &mut registry_obj,
            string::utf8(b"workflow_dev"),
            string::utf8(b"workflow_agent"),
            option::some(string::utf8(b"https://new-agent.com/workflow.png")),
            option::some(string::utf8(b"Updated comprehensive workflow agent")),
            option::some(string::utf8(b"https://new-agent.workflow.dev")),
            vector[
                string::utf8(b"sui"),
                string::utf8(b"ethereum"),
                string::utf8(b"polygon"),
                string::utf8(b"avalanche"),
            ],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // 8. Update Method
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::update_method(
            &mut registry_obj,
            string::utf8(b"workflow_dev"),
            string::utf8(b"workflow_agent"),
            string::utf8(b"tee_method"),
            string::utf8(b"workflow/tee:v2.0"),
            option::some(string::utf8(b"sha256:tee789")),
            16,
            8,
            true,
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // 9. Change Default Method
    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::set_default_method(
            &mut registry_obj,
            string::utf8(b"workflow_dev"),
            string::utf8(b"workflow_agent"),
            string::utf8(b"tee_method"),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // 10. Verify Final State
    {
        next_tx(&mut scenario, DEVELOPER);
        let registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        let (developer_obj, agent_obj) = registry::get_agent(
            &registry_obj,
            string::utf8(b"workflow_dev"),
            string::utf8(b"workflow_agent"),
        );

        // Verify developer
        assert!(
            coordination::developer::developer_name(developer_obj) == string::utf8(b"workflow_dev"),
        );
        assert!(
            coordination::developer::developer_owner(developer_obj) == DEVELOPER,
        );

        // Verify agent
        assert!(
            coordination::agent::agent_name(agent_obj) == string::utf8(b"workflow_agent"),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

// === SECURITY TESTS: Unauthorized Access ===

#[test]
#[expected_failure(abort_code = coordination::developer::EInvalidOwner)]
public fun test_unauthorized_update_developer() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Setup: Create registry and developer
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Security Test Registry"),
            ctx(&mut scenario),
        );
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"secure_dev"),
            string::utf8(b"secure-github"),
            option::none(),
            option::some(string::utf8(b"Original description")),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Unauthorized attempt to update developer (should fail)
    {
        next_tx(&mut scenario, UNAUTHORIZED);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::update_developer(
            &mut registry_obj,
            string::utf8(b"secure_dev"),
            string::utf8(b"hacked-github"),
            option::some(string::utf8(b"https://malicious.com/avatar.png")),
            option::some(string::utf8(b"Hacked description")),
            option::some(string::utf8(b"https://malicious.com")),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
#[expected_failure(abort_code = coordination::agent::EInvalidOwner)]
public fun test_unauthorized_add_agent() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Setup: Create registry and developer
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Security Test Registry"),
            ctx(&mut scenario),
        );
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"target_dev"),
            string::utf8(b"target-github"),
            option::none(),
            option::none(),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Unauthorized attempt to add agent (should fail)
    {
        next_tx(&mut scenario, UNAUTHORIZED);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"target_dev"),
            string::utf8(b"malicious_agent"),
            option::some(string::utf8(b"https://malicious.com/agent.png")),
            option::some(
                string::utf8(b"Malicious agent injected by unauthorized user"),
            ),
            option::some(string::utf8(b"https://malicious.com")),
            vector[string::utf8(b"ethereum")],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
#[expected_failure(abort_code = coordination::developer::EInvalidOwner)]
public fun test_unauthorized_update_agent() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Setup: Create registry, developer, and agent
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Security Test Registry"),
            ctx(&mut scenario),
        );
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"target_dev"),
            string::utf8(b"target-github"),
            option::none(),
            option::none(),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"target_dev"),
            string::utf8(b"target_agent"),
            option::some(string::utf8(b"https://legitimate.com/agent.png")),
            option::some(string::utf8(b"Legitimate agent")),
            option::some(string::utf8(b"https://legitimate.com")),
            vector[string::utf8(b"sui")],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Unauthorized attempt to update agent (should fail)
    {
        next_tx(&mut scenario, UNAUTHORIZED);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::update_agent(
            &mut registry_obj,
            string::utf8(b"target_dev"),
            string::utf8(b"target_agent"),
            option::some(string::utf8(b"https://malicious.com/agent.png")),
            option::some(string::utf8(b"Hacked agent description")),
            option::some(string::utf8(b"https://malicious.com")),
            vector[string::utf8(b"ethereum"), string::utf8(b"polygon")],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
#[expected_failure(abort_code = coordination::developer::EInvalidOwner)]
public fun test_unauthorized_remove_agent() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Setup: Create registry, developer, and agent
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Security Test Registry"),
            ctx(&mut scenario),
        );
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"target_dev"),
            string::utf8(b"target-github"),
            option::none(),
            option::none(),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"target_dev"),
            string::utf8(b"valuable_agent"),
            option::some(string::utf8(b"https://valuable.com/agent.png")),
            option::some(string::utf8(b"Valuable agent with important data")),
            option::some(string::utf8(b"https://valuable.com")),
            vector[string::utf8(b"sui")],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Unauthorized attempt to remove agent (should fail)
    {
        next_tx(&mut scenario, UNAUTHORIZED);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::remove_agent(
            &mut registry_obj,
            string::utf8(b"target_dev"),
            string::utf8(b"valuable_agent"),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
#[expected_failure(abort_code = coordination::developer::EInvalidOwner)]
public fun test_unauthorized_add_method() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Setup: Create registry, developer, and agent
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Security Test Registry"),
            ctx(&mut scenario),
        );
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method-github"),
            option::none(),
            option::none(),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            option::none(),
            option::some(string::utf8(b"Agent for method testing")),
            option::none(),
            vector[string::utf8(b"sui")],
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Unauthorized attempt to add method (should fail)
    {
        next_tx(&mut scenario, UNAUTHORIZED);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_method(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            string::utf8(b"malicious_method"),
            string::utf8(b"malicious/backdoor:latest"),
            option::some(string::utf8(b"sha256:malicious123")),
            64, // Excessive memory request
            32, // Excessive CPU request
            false,
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
#[expected_failure(abort_code = coordination::developer::EInvalidOwner)]
public fun test_unauthorized_update_method() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Setup: Create registry, developer, agent, and method
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Security Test Registry"),
            ctx(&mut scenario),
        );
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method-github"),
            option::none(),
            option::none(),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            option::none(),
            option::some(string::utf8(b"Agent for method testing")),
            option::none(),
            vector[string::utf8(b"sui")],
            &clock,
            ctx(&mut scenario),
        );

        registry::add_method(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            string::utf8(b"legitimate_method"),
            string::utf8(b"legitimate/agent:v1.0"),
            option::some(string::utf8(b"sha256:legitimate123")),
            4,
            2,
            false,
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Unauthorized attempt to update method (should fail)
    {
        next_tx(&mut scenario, UNAUTHORIZED);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::update_method(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            string::utf8(b"legitimate_method"),
            string::utf8(b"malicious/backdoor:v2.0"),
            option::some(string::utf8(b"sha256:backdoor456")),
            64,
            32,
            false,
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
#[expected_failure(abort_code = coordination::developer::EInvalidOwner)]
public fun test_unauthorized_remove_method() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Setup: Create registry, developer, agent, and method
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Security Test Registry"),
            ctx(&mut scenario),
        );
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method-github"),
            option::none(),
            option::none(),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            option::none(),
            option::some(string::utf8(b"Agent for method testing")),
            option::none(),
            vector[string::utf8(b"sui")],
            &clock,
            ctx(&mut scenario),
        );

        registry::add_method(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            string::utf8(b"important_method"),
            string::utf8(b"important/agent:v1.0"),
            option::some(string::utf8(b"sha256:important123")),
            4,
            2,
            false,
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Unauthorized attempt to remove method (should fail)
    {
        next_tx(&mut scenario, UNAUTHORIZED);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::remove_method(
            &mut registry_obj,
            string::utf8(b"method_dev"),
            string::utf8(b"method_agent"),
            string::utf8(b"important_method"),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
#[expected_failure(abort_code = coordination::developer::EInvalidOwner)]
public fun test_unauthorized_set_default_method() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Setup: Create registry, developer, agent, and method
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Security Test Registry"),
            ctx(&mut scenario),
        );
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"default_dev"),
            string::utf8(b"default-github"),
            option::none(),
            option::none(),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"default_dev"),
            string::utf8(b"default_agent"),
            option::none(),
            option::some(string::utf8(b"Agent for default method testing")),
            option::none(),
            vector[string::utf8(b"sui")],
            &clock,
            ctx(&mut scenario),
        );

        registry::add_method(
            &mut registry_obj,
            string::utf8(b"default_dev"),
            string::utf8(b"default_agent"),
            string::utf8(b"secure_method"),
            string::utf8(b"secure/agent:v1.0"),
            option::some(string::utf8(b"sha256:secure123")),
            4,
            2,
            false,
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Unauthorized attempt to set default method (should fail)
    {
        next_tx(&mut scenario, UNAUTHORIZED);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::set_default_method(
            &mut registry_obj,
            string::utf8(b"default_dev"),
            string::utf8(b"default_agent"),
            string::utf8(b"secure_method"),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}

#[test]
#[expected_failure(abort_code = coordination::developer::EInvalidOwner)]
public fun test_unauthorized_remove_default_method() {
    let mut scenario = test::begin(ADMIN);
    let clock = clock::create_for_testing(ctx(&mut scenario));

    // Setup: Create registry, developer, agent, method, and set as default
    {
        next_tx(&mut scenario, ADMIN);
        registry::create_registry(
            string::utf8(b"Security Test Registry"),
            ctx(&mut scenario),
        );
    };

    {
        next_tx(&mut scenario, DEVELOPER);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::add_developer(
            &mut registry_obj,
            string::utf8(b"default_dev"),
            string::utf8(b"default-github"),
            option::none(),
            option::none(),
            option::none(),
            &clock,
            ctx(&mut scenario),
        );

        registry::add_agent(
            &mut registry_obj,
            string::utf8(b"default_dev"),
            string::utf8(b"default_agent"),
            option::none(),
            option::some(string::utf8(b"Agent for default method testing")),
            option::none(),
            vector[string::utf8(b"sui")],
            &clock,
            ctx(&mut scenario),
        );

        registry::add_method(
            &mut registry_obj,
            string::utf8(b"default_dev"),
            string::utf8(b"default_agent"),
            string::utf8(b"default_method"),
            string::utf8(b"default/agent:v1.0"),
            option::some(string::utf8(b"sha256:default123")),
            4,
            2,
            false,
            &clock,
            ctx(&mut scenario),
        );

        registry::set_default_method(
            &mut registry_obj,
            string::utf8(b"default_dev"),
            string::utf8(b"default_agent"),
            string::utf8(b"default_method"),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    // Unauthorized attempt to remove default method (should fail)
    {
        next_tx(&mut scenario, UNAUTHORIZED);
        let mut registry_obj = test::take_shared<registry::AgentRegistry>(
            &scenario,
        );

        registry::remove_default_method(
            &mut registry_obj,
            string::utf8(b"default_dev"),
            string::utf8(b"default_agent"),
            &clock,
            ctx(&mut scenario),
        );

        test::return_shared(registry_obj);
    };

    clock::destroy_for_testing(clock);
    test::end(scenario);
}
