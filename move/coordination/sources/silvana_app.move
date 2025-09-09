module coordination::silvana_app;

use coordination::app_method::AppMethod;
use std::string::String;
use sui::clock::{timestamp_ms, Clock};
use sui::event;
use sui::vec_map::VecMap;
use sui::vec_set::VecSet;

public struct SilvanaApp has key, store {
    id: UID,
    name: String,
    description: Option<String>,
    methods: VecMap<String, AppMethod>,
    owner: address,
    created_at: u64,
    updated_at: u64,
    version: u64,
    instances: VecSet<address>,
}

public struct AppCreatedEvent has copy, drop {
    id: address,
    name: String,
    description: Option<String>,
    owner: address,
    created_at: u64,
}

public struct AppUpdatedEvent has copy, drop {
    id: address,
    name: String,
    description: Option<String>,
    owner: address,
    updated_at: u64,
    version: u64,
}

public struct AppDeletedEvent has copy, drop {
    id: address,
    name: String,
    description: Option<String>,
    owner: address,
    version: u64,
    deleted_at: u64,
}

public struct AppNames has key, store {
    id: UID,
    app_owner: address,
    names: vector<String>,
    version: u64,
}

public struct AppNamesCreatedEvent has copy, drop {
    id: address,
    app_owner: address,
    names: vector<String>,
    version: u64,
}

public struct AppNamesUpdatedEvent has copy, drop {
    id: address,
    app_owner: address,
    names: vector<String>,
    version: u64,
}

// Error codes
#[error]
const EInvalidOwner: vector<u8> = b"Invalid owner";

public(package) fun create_app(
    name: String,
    description: Option<String>,
    clock: &Clock,
    ctx: &mut TxContext,
): SilvanaApp {
    let app_id = object::new(ctx);
    let address = app_id.to_address();
    let timestamp = clock.timestamp_ms();
    let app = SilvanaApp {
        id: app_id,
        name,
        description,
        methods: sui::vec_map::empty(),
        owner: ctx.sender(),
        created_at: timestamp,
        updated_at: timestamp,
        version: 1,
        instances: sui::vec_set::empty(),
    };
    event::emit(AppCreatedEvent {
        id: address,
        name,
        description,
        owner: ctx.sender(),
        created_at: timestamp,
    });
    app
}

public(package) fun update_app(
    app: &mut SilvanaApp,
    description: Option<String>,
    clock: &Clock,
    admin_address: address,
    ctx: &TxContext,
) {
    assert!(app.owner == ctx.sender() || admin_address == ctx.sender(), EInvalidOwner);
    app.description = description;
    app.updated_at = clock.timestamp_ms();
    app.version = app.version + 1;

    event::emit(AppUpdatedEvent {
        id: app.id.to_address(),
        name: app.name,
        description,
        owner: ctx.sender(),
        updated_at: app.updated_at,
        version: app.version,
    });
}

public(package) fun delete_app(
    app: SilvanaApp,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(app.owner == ctx.sender(), EInvalidOwner);
    event::emit(AppDeletedEvent {
        id: app.owner,
        name: app.name,
        description: app.description,
        owner: app.owner,
        version: app.version,
        deleted_at: clock.timestamp_ms(),
    });

    let SilvanaApp { id, .. } = app;
    object::delete(id);
}

public(package) fun create_app_names(
    app_owner: address,
    names: vector<String>,
    ctx: &mut TxContext,
): AppNames {
    let app_names = AppNames {
        id: object::new(ctx),
        app_owner,
        names,
        version: 1,
    };
    event::emit(AppNamesCreatedEvent {
        id: app_names.id.to_address(),
        app_owner,
        names,
        version: 1,
    });
    app_names
}

public(package) fun update_app_names(
    app_names: &mut AppNames,
    names: vector<String>,
    ctx: &TxContext,
) {
    assert!(app_names.app_owner == ctx.sender(), EInvalidOwner);
    app_names.names = names;
    app_names.version = app_names.version + 1;
    event::emit(AppNamesUpdatedEvent {
        id: app_names.id.to_address(),
        app_owner: app_names.app_owner,
        names,
        version: app_names.version,
    });
}

public(package) fun add_name_to_app_names(
    app_names: &mut AppNames,
    name: String,
    ctx: &TxContext,
) {
    assert!(app_names.app_owner == ctx.sender(), EInvalidOwner);
    app_names.names.push_back(name);
    app_names.version = app_names.version + 1;
    event::emit(AppNamesUpdatedEvent {
        id: app_names.id.to_address(),
        app_owner: app_names.app_owner,
        names: app_names.names,
        version: app_names.version,
    });
}

public(package) fun remove_name_from_app_names(
    app_names: &mut AppNames,
    name: String,
    ctx: &TxContext,
) {
    assert!(app_names.app_owner == ctx.sender(), EInvalidOwner);
    let (found, index) = vector::index_of(&app_names.names, &name);
    if (found) {
        vector::remove(&mut app_names.names, index);
    };
    app_names.version = app_names.version + 1;
    event::emit(AppNamesUpdatedEvent {
        id: app_names.id.to_address(),
        app_owner: app_names.app_owner,
        names: app_names.names,
        version: app_names.version,
    });
}

public(package) fun delete_app_names(app_names: AppNames, ctx: &TxContext) {
    assert!(app_names.app_owner == ctx.sender(), EInvalidOwner);
    let AppNames { id, .. } = app_names;
    object::delete(id);
}

// Admin versions of functions that don't check ownership
public(package) fun admin_remove_name_from_app_names(
    app_names: &mut AppNames,
    name: String,
) {
    let (found, index) = vector::index_of(&app_names.names, &name);
    if (found) {
        vector::remove(&mut app_names.names, index);
    };
    app_names.version = app_names.version + 1;
    event::emit(AppNamesUpdatedEvent {
        id: app_names.id.to_address(),
        app_owner: app_names.app_owner,
        names: app_names.names,
        version: app_names.version,
    });
}

public(package) fun admin_delete_app_names(app_names: AppNames) {
    let AppNames { id, .. } = app_names;
    object::delete(id);
}

public(package) fun admin_delete_app(app: SilvanaApp, clock: &Clock) {
    event::emit(AppDeletedEvent {
        id: app.owner,
        name: app.name,
        description: app.description,
        owner: app.owner,
        version: app.version,
        deleted_at: clock.timestamp_ms(),
    });

    let SilvanaApp { id, .. } = app;
    object::delete(id);
}

// Getter functions
public(package) fun app_owner(app: &SilvanaApp): address {
    app.owner
}

public(package) fun app_name(app: &SilvanaApp): String {
    app.name
}

public(package) fun app_names_is_empty(app_names: &AppNames): bool {
    vector::is_empty(&app_names.names)
}

// Method management functions
public(package) fun add_method_to_app(
    app: &mut SilvanaApp,
    method_name: String,
    method: AppMethod,
    admin_address: address,
    ctx: &TxContext,
) {
    assert!(app.owner == ctx.sender() || admin_address == ctx.sender(), EInvalidOwner);
    app.methods.insert(method_name, method);
    app.version = app.version + 1;
}

public(package) fun remove_method_from_app(
    app: &mut SilvanaApp,
    method_name: String,
    admin_address: address,
    ctx: &TxContext,
): AppMethod {
    assert!(app.owner == ctx.sender() || admin_address == ctx.sender(), EInvalidOwner);
    let (_, method) = app.methods.remove(&method_name);
    app.version = app.version + 1;
    method
}

public(package) fun get_method_from_app(
    app: &SilvanaApp,
    method_name: &String,
): &AppMethod {
    app.methods.get(method_name)
}

public(package) fun get_method_from_app_mut(
    app: &mut SilvanaApp,
    method_name: &String,
    ctx: &TxContext,
): &mut AppMethod {
    assert!(app.owner == ctx.sender(), EInvalidOwner);
    app.methods.get_mut(method_name)
}

// Instance management functions
public(package) fun add_instance_to_app(
    app: &mut SilvanaApp,
    instance_owner: address,
    admin_address: address,
    ctx: &TxContext,
) {
    assert!(app.owner == ctx.sender() || admin_address == ctx.sender(), EInvalidOwner);
    app.instances.insert(instance_owner);
    app.version = app.version + 1;
}

public(package) fun remove_instance_from_app(
    app: &mut SilvanaApp,
    instance_owner: address,
    admin_address: address,
    ctx: &TxContext,
) {
    assert!(app.owner == ctx.sender() || admin_address == ctx.sender(), EInvalidOwner);
    app.instances.remove(&instance_owner);
    app.version = app.version + 1;
}

public(package) fun has_instance_in_app(
    app: &SilvanaApp,
    instance_owner: address,
): bool {
    app.instances.contains(&instance_owner)
}

public(package) fun get_instance_owners(app: &SilvanaApp): vector<address> {
    app.instances.into_keys()
}

// Get all methods from the app
public fun get_app_methods(app: &SilvanaApp): &VecMap<String, AppMethod> {
    &app.methods
}

// Clone methods for app instance creation
public(package) fun clone_app_methods(app: &SilvanaApp): VecMap<String, AppMethod> {
    let mut cloned = sui::vec_map::empty<String, AppMethod>();
    let keys = sui::vec_map::keys(&app.methods);
    let mut i = 0;
    while (i < vector::length(&keys)) {
        let key = vector::borrow(&keys, i);
        let method = *sui::vec_map::get(&app.methods, key);
        sui::vec_map::insert(&mut cloned, *key, method);
        i = i + 1;
    };
    cloned
}
