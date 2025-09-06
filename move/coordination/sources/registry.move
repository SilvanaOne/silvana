module coordination::registry;

use coordination::agent::Agent;
use coordination::app_instance::AppInstanceCap;
use coordination::app_method::AppMethod;
use coordination::developer::{Developer, DeveloperNames};
use coordination::silvana_app::{SilvanaApp, AppNames};
use std::string::String;
use sui::clock::Clock;
use sui::display;
use sui::event;
use sui::object_table::{Self, remove};
use sui::package;

public struct SilvanaRegistry has key, store {
    id: UID,
    name: String,
    version: u32,
    admin: address,
    developers: object_table::ObjectTable<String, Developer>,
    developers_index: object_table::ObjectTable<address, DeveloperNames>,
    apps: object_table::ObjectTable<String, SilvanaApp>,
    apps_index: object_table::ObjectTable<address, AppNames>,
}

public struct RegistryCreatedEvent has copy, drop {
    id: address,
    name: String,
    admin: address,
}

public struct REGISTRY has drop {}

// Error codes
#[error]
const ENotAdmin: vector<u8> = b"Not admin";

fun init(otw: REGISTRY, ctx: &mut TxContext) {
    let publisher = package::claim(otw, ctx);

    let developer_keys = vector[
        b"name".to_string(),
        b"description".to_string(),
        b"image_url".to_string(),
        b"thumbnail_url".to_string(),
        b"link".to_string(),
        b"project_url".to_string(),
    ];

    let developer_values = vector[
        b"{name}".to_string(),
        b"{description}".to_string(),
        b"{image}".to_string(),
        b"{image}".to_string(),
        b"https://github.com/{github}".to_string(),
        b"{site}".to_string(),
    ];
    let mut display_developers = display::new_with_fields<Developer>(
        &publisher,
        developer_keys,
        developer_values,
        ctx,
    );

    let agent_keys = vector[
        b"name".to_string(),
        b"description".to_string(),
        b"image_url".to_string(),
        b"thumbnail_url".to_string(),
        b"project_url".to_string(),
    ];

    let agent_values = vector[
        b"{name}".to_string(),
        b"{description}".to_string(),
        b"{image}".to_string(),
        b"{image}".to_string(),
        b"{site}".to_string(),
    ];
    let mut display_agents = display::new_with_fields<Agent>(
        &publisher,
        agent_keys,
        agent_values,
        ctx,
    );

    let registry_keys = vector[
        b"name".to_string(),
        b"description".to_string(),
        b"image_url".to_string(),
        b"thumbnail_url".to_string(),
        b"project_url".to_string(),
    ];

    let registry_values = vector[
        b"{name}".to_string(),
        b"Registry for Silvana agents and developers".to_string(),
        b"https://silvana.one/_next/static/media/logo.b97230ea.svg".to_string(),
        b"https://silvana.one/_next/static/media/logo.b97230ea.svg".to_string(),
        b"https://silvana.one".to_string(),
    ];
    let mut display_registry = display::new_with_fields<Agent>(
        &publisher,
        registry_keys,
        registry_values,
        ctx,
    );

    display_developers.update_version();
    display_agents.update_version();
    display_registry.update_version();
    transfer::public_transfer(publisher, ctx.sender());
    transfer::public_transfer(display_developers, ctx.sender());
    transfer::public_transfer(display_agents, ctx.sender());
    transfer::public_transfer(display_registry, ctx.sender());
}

public fun create_registry(name: String, ctx: &mut TxContext) {
    let registry = SilvanaRegistry {
        id: object::new(ctx),
        name,
        version: 1,
        admin: ctx.sender(),
        developers: object_table::new(ctx),
        developers_index: object_table::new(ctx),
        apps: object_table::new(ctx),
        apps_index: object_table::new(ctx),
    };
    event::emit(RegistryCreatedEvent {
        id: registry.id.to_address(),
        name,
        admin: ctx.sender(),
    });
    transfer::share_object(registry);
}

public fun add_developer(
    registry: &mut SilvanaRegistry,
    name: String,
    github: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    let developer = coordination::developer::create_developer(
        name,
        github,
        image,
        description,
        site,
        clock,
        ctx,
    );

    registry.developers.add(name, developer);

    if (registry.developers_index.contains(ctx.sender())) {
        let developer_names = registry
            .developers_index
            .borrow_mut(ctx.sender());
        coordination::developer::add_name_to_developer_names(
            developer_names,
            name,
            ctx,
        );
    } else {
        let developer_names = coordination::developer::create_developer_names(
            ctx.sender(),
            vector[name],
            ctx,
        );
        registry.developers_index.add(ctx.sender(), developer_names);
    }
}

public fun update_developer(
    registry: &mut SilvanaRegistry,
    name: String,
    github: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    let developer = registry.developers.borrow_mut(name);
    coordination::developer::update_developer(
        developer,
        github,
        image,
        description,
        site,
        clock,
        registry.admin,
        ctx,
    );
}

public fun remove_developer(
    registry: &mut SilvanaRegistry,
    name: String,
    agent_names: vector<String>,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    assert!(registry.admin == ctx.sender(), ENotAdmin);
    let developer = registry.developers.remove(name);
    let developer_owner = coordination::developer::developer_owner(&developer);

    if (registry.developers_index.contains(developer_owner)) {
        let developer_names = registry
            .developers_index
            .borrow_mut(developer_owner);
        coordination::developer::admin_remove_name_from_developer_names(
            developer_names,
            name,
        );

        if (
            coordination::developer::developer_names_is_empty(developer_names)
        ) {
            let developer_names = registry
                .developers_index
                .remove(developer_owner);
            coordination::developer::admin_delete_developer_names(
                developer_names,
            );
        };
    };

    coordination::developer::admin_delete_developer(
        developer,
        agent_names,
        clock,
    );
}

public fun add_agent(
    registry: &mut SilvanaRegistry,
    developer: String,
    name: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    chains: vector<String>,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    let developer_object = registry.developers.borrow_mut(developer);
    let developer_owner = coordination::developer::developer_owner(
        developer_object,
    );

    let agent = coordination::agent::create_agent(
        developer_owner,
        name,
        image,
        description,
        site,
        chains,
        clock,
        ctx,
    );

    coordination::developer::add_agent_to_developer(
        developer_object,
        agent,
        registry.admin,
        ctx,
    );
}

public fun update_agent(
    registry: &mut SilvanaRegistry,
    developer: String,
    name: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    chains: vector<String>,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    coordination::developer::registry_update_agent(
        &mut registry.developers,
        developer,
        name,
        image,
        description,
        site,
        chains,
        clock,
        registry.admin,
        ctx,
    );
}

public fun remove_agent(
    registry: &mut SilvanaRegistry,
    developer: String,
    name: String,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    let developer_object = registry.developers.borrow_mut(developer);
    let developer_owner = coordination::developer::developer_owner(
        developer_object,
    );
    let agent = coordination::developer::remove_agent_from_developer(
        developer_object,
        name,
        registry.admin,
        ctx,
    );

    coordination::agent::delete_agent(agent, developer_owner, clock, registry.admin, ctx);
}

public fun add_method(
    registry: &mut SilvanaRegistry,
    developer: String,
    agent_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    coordination::developer::registry_add_method(
        &mut registry.developers,
        developer,
        agent_name,
        method_name,
        docker_image,
        docker_sha256,
        min_memory_gb,
        min_cpu_cores,
        requires_tee,
        clock,
        registry.admin,
        ctx,
    );
}

public fun update_method(
    registry: &mut SilvanaRegistry,
    developer: String,
    agent_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    coordination::developer::registry_update_method(
        &mut registry.developers,
        developer,
        agent_name,
        method_name,
        docker_image,
        docker_sha256,
        min_memory_gb,
        min_cpu_cores,
        requires_tee,
        clock,
        registry.admin,
        ctx,
    );
}

public fun remove_method(
    registry: &mut SilvanaRegistry,
    developer: String,
    agent_name: String,
    method_name: String,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    coordination::developer::registry_remove_method(
        &mut registry.developers,
        developer,
        agent_name,
        method_name,
        clock,
        registry.admin,
        ctx,
    );
}

public fun set_default_method(
    registry: &mut SilvanaRegistry,
    developer: String,
    agent_name: String,
    method_name: String,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    coordination::developer::registry_set_default_method(
        &mut registry.developers,
        developer,
        agent_name,
        method_name,
        clock,
        registry.admin,
        ctx,
    );
}

public fun remove_default_method(
    registry: &mut SilvanaRegistry,
    developer: String,
    agent_name: String,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    coordination::developer::registry_remove_default_method(
        &mut registry.developers,
        developer,
        agent_name,
        clock,
        registry.admin,
        ctx,
    );
}

public fun get_agent(
    registry: &SilvanaRegistry,
    developer: String,
    agent: String,
): (&Developer, &Agent) {
    let developer_object = registry.developers.borrow(developer);
    (
        developer_object,
        coordination::developer::get_agent_from_developer(
            developer_object,
            agent,
        ),
    )
}

public fun add_app(
    registry: &mut SilvanaRegistry,
    name: String,
    description: Option<String>,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    let app = coordination::silvana_app::create_app(
        name,
        description,
        clock,
        ctx,
    );

    registry.apps.add(name, app);

    if (registry.apps_index.contains(ctx.sender())) {
        let app_names = registry.apps_index.borrow_mut(ctx.sender());
        coordination::silvana_app::add_name_to_app_names(
            app_names,
            name,
            ctx,
        );
    } else {
        let app_names = coordination::silvana_app::create_app_names(
            ctx.sender(),
            vector[name],
            ctx,
        );
        registry.apps_index.add(ctx.sender(), app_names);
    }
}

public fun update_app(
    registry: &mut SilvanaRegistry,
    name: String,
    description: Option<String>,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    let app = registry.apps.borrow_mut(name);
    coordination::silvana_app::update_app(
        app,
        description,
        clock,
        registry.admin,
        ctx,
    );
}

public fun remove_app(
    registry: &mut SilvanaRegistry,
    name: String,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    assert!(registry.admin == ctx.sender(), ENotAdmin);
    let app = registry.apps.remove(name);
    let app_owner = coordination::silvana_app::app_owner(&app);

    if (registry.apps_index.contains(app_owner)) {
        let app_names = registry.apps_index.borrow_mut(app_owner);
        coordination::silvana_app::admin_remove_name_from_app_names(
            app_names,
            name,
        );

        if (coordination::silvana_app::app_names_is_empty(app_names)) {
            let app_names = registry.apps_index.remove(app_owner);
            coordination::silvana_app::admin_delete_app_names(
                app_names,
            );
        };
    };

    coordination::silvana_app::admin_delete_app(
        app,
        clock,
    );
}

public fun get_app(registry: &SilvanaRegistry, name: String): &SilvanaApp {
    registry.apps.borrow(name)
}

public fun add_method_to_app(
    registry: &mut SilvanaRegistry,
    app_name: String,
    method_name: String,
    method: AppMethod,
    ctx: &mut TxContext,
) {
    let app = registry.apps.borrow_mut(app_name);
    coordination::silvana_app::add_method_to_app(
        app,
        method_name,
        method,
        registry.admin,
        ctx,
    );
}

public fun remove_method_from_app(
    registry: &mut SilvanaRegistry,
    app_name: String,
    method_name: String,
    ctx: &mut TxContext,
): AppMethod {
    let app = registry.apps.borrow_mut(app_name);
    coordination::silvana_app::remove_method_from_app(
        app,
        method_name,
        registry.admin,
        ctx,
    )
}

public fun add_instance_to_app(
    registry: &mut SilvanaRegistry,
    app_name: String,
    instance_owner: address,
    ctx: &mut TxContext,
) {
    let app = registry.apps.borrow_mut(app_name);
    coordination::silvana_app::add_instance_to_app(
        app,
        instance_owner,
        registry.admin,
        ctx,
    );
}

public fun remove_instance_from_app(
    registry: &mut SilvanaRegistry,
    app_name: String,
    instance_owner: address,
    ctx: &mut TxContext,
) {
    let app = registry.apps.borrow_mut(app_name);
    coordination::silvana_app::remove_instance_from_app(
        app,
        instance_owner,
        registry.admin,
        ctx,
    );
}

public fun has_instance_in_app(
    registry: &SilvanaRegistry,
    app_name: String,
    instance_owner: address,
): bool {
    let app = registry.apps.borrow(app_name);
    coordination::silvana_app::has_instance_in_app(app, instance_owner)
}

public fun get_app_instance_owners(
    registry: &SilvanaRegistry,
    app_name: String,
): vector<address> {
    let app = registry.apps.borrow(app_name);
    coordination::silvana_app::get_instance_owners(app)
}

public fun create_app_instance_from_registry(
    registry: &mut SilvanaRegistry,
    app_name: String,
    description: Option<String>,
    settlement_chains: vector<String>, // vector of chain names
    settlement_addresses: vector<Option<String>>, // vector of optional settlement addresses
    min_time_between_blocks: u64, // Minimum time between blocks in milliseconds
    clock: &Clock,
    ctx: &mut TxContext,
): AppInstanceCap {
    let app = registry.apps.borrow_mut(app_name);
    coordination::app_instance::create_app_instance(
        app,
        description,
        settlement_chains,
        settlement_addresses,
        min_time_between_blocks,
        clock,
        ctx,
    )
}
