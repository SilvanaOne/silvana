module coordination::registry;

use coordination::agent::Agent;
use coordination::developer::{Developer, DeveloperNames};
use std::string::String;
use sui::clock::Clock;
use sui::display;
use sui::event;
use sui::object_table::{Self, remove};
use sui::package;

public struct AgentRegistry has key, store {
    id: UID,
    name: String,
    version: u32,
    admin: address,
    developers: object_table::ObjectTable<String, Developer>,
    developers_index: object_table::ObjectTable<address, DeveloperNames>,
}

public struct RegistryCreatedEvent has copy, drop {
    id: address,
    name: String,
    admin: address,
}

public struct REGISTRY has drop {}

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
    let registry = AgentRegistry {
        id: object::new(ctx),
        name,
        version: 1,
        admin: ctx.sender(),
        developers: object_table::new(ctx),
        developers_index: object_table::new(ctx),
    };
    event::emit(RegistryCreatedEvent {
        id: registry.id.to_address(),
        name,
        admin: ctx.sender(),
    });
    transfer::share_object(registry);
}

public fun add_developer(
    registry: &mut AgentRegistry,
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
    registry: &mut AgentRegistry,
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
        ctx,
    );
}

public fun remove_developer(
    registry: &mut AgentRegistry,
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
    registry: &mut AgentRegistry,
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
        ctx,
    );
}

public fun update_agent(
    registry: &mut AgentRegistry,
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
        ctx,
    );
}

public fun remove_agent(
    registry: &mut AgentRegistry,
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
        ctx,
    );

    coordination::agent::delete_agent(agent, developer_owner, clock, ctx);
}

public fun add_method(
    registry: &mut AgentRegistry,
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
        ctx,
    );
}

public fun update_method(
    registry: &mut AgentRegistry,
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
        ctx,
    );
}

public fun remove_method(
    registry: &mut AgentRegistry,
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
        ctx,
    );
}

public fun set_default_method(
    registry: &mut AgentRegistry,
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
        ctx,
    );
}

public fun remove_default_method(
    registry: &mut AgentRegistry,
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
        ctx,
    );
}

public fun get_agent(
    registry: &AgentRegistry,
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
