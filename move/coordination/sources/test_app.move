module coordination::test_app;

use coordination::app_instance::AppInstanceCap;
use coordination::registry::{
    SilvanaRegistry,
    create_app_instance_from_registry
};
use std::string::String;
use sui::clock::Clock;
use sui::event;

/// TestApp is a simplified application for testing purposes
/// It contains an AppInstanceCap to manage a shared AppInstance
public struct TestApp has key, store {
    id: UID,
    instance_cap: AppInstanceCap,
}

public struct TestAppCreatedEvent has copy, drop {
    test_app_address: address,
    created_at: u64,
}

/// Creates a new TestApp with an initialized AppInstance
/// The instance is created from the registry and initialized with a default state
public fun create_test_app(
    registry: &mut SilvanaRegistry,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    // Create an app instance from the registry's SilvanaApp
    let instance_cap = create_app_instance_from_registry(
        registry,
        b"test_app".to_string(),
        option::none(),
        vector::empty<String>(),  // no settlement chains
        vector::empty<Option<String>>(),  // no settlement addresses
        60_000, // Use minimum allowed time between blocks (60 seconds)
        clock,
        ctx,
    );

    let test_app_id = object::new(ctx);

    event::emit(TestAppCreatedEvent {
        test_app_address: test_app_id.to_address(),
        created_at: clock.timestamp_ms(),
    });

    transfer::share_object(TestApp {
        id: test_app_id,
        instance_cap,
    });
}

/// Returns a reference to the instance capability for testing purposes
public fun instance_cap(test_app: &TestApp): &AppInstanceCap {
    &test_app.instance_cap
}
