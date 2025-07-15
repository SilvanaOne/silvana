module coordination::developer;

use coordination::agent::Agent;
use std::string::String;
use sui::clock::{timestamp_ms, Clock};
use sui::event;
use sui::object_table;

public struct Developer has key, store {
    id: UID,
    name: String,
    github: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    agents: object_table::ObjectTable<String, Agent>,
    owner: address,
    created_at: u64,
    updated_at: u64,
    version: u64,
}

public struct DeveloperCreatedEvent has copy, drop {
    id: address,
    name: String,
    github: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    owner: address,
    created_at: u64,
}

public struct DeveloperUpdatedEvent has copy, drop {
    id: address,
    name: String,
    github: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    owner: address,
    updated_at: u64,
    version: u64,
}

public struct DeveloperDeletedEvent has copy, drop {
    id: address,
    name: String,
    github: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    version: u64,
    deleted_at: u64,
}

public struct DeveloperNames has key, store {
    id: UID,
    developer: address,
    names: vector<String>,
    version: u64,
}

public struct DeveloperNamesCreatedEvent has copy, drop {
    id: address,
    developer: address,
    names: vector<String>,
    version: u64,
}

public struct DeveloperNamesUpdatedEvent has copy, drop {
    id: address,
    developer: address,
    names: vector<String>,
    version: u64,
}

#[error]
const EInvalidOwner: vector<u8> = b"Invalid owner";

public(package) fun create_developer(
    name: String,
    github: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    clock: &Clock,
    ctx: &mut TxContext,
): Developer {
    let developer_id = object::new(ctx);
    let address = developer_id.to_address();
    let timestamp = clock.timestamp_ms();
    let developer = Developer {
        id: developer_id,
        name,
        github,
        image,
        description,
        site,
        agents: object_table::new(ctx),
        owner: ctx.sender(),
        created_at: timestamp,
        updated_at: timestamp,
        version: 1,
    };
    event::emit(DeveloperCreatedEvent {
        id: address,
        name,
        github,
        image,
        description,
        site,
        owner: ctx.sender(),
        created_at: timestamp,
    });
    developer
}

public(package) fun update_developer(
    developer: &mut Developer,
    github: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(developer.owner == ctx.sender(), EInvalidOwner);
    developer.github = github;
    developer.image = image;
    developer.description = description;
    developer.site = site;
    developer.updated_at = clock.timestamp_ms();
    developer.version = developer.version + 1;

    event::emit(DeveloperUpdatedEvent {
        id: developer.id.to_address(),
        name: developer.name,
        github,
        image,
        description,
        site,
        owner: ctx.sender(),
        updated_at: developer.updated_at,
        version: developer.version,
    });
}

public(package) fun delete_developer(
    developer: Developer,
    agent_names: vector<String>,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(developer.owner == ctx.sender(), EInvalidOwner);
    event::emit(DeveloperDeletedEvent {
        id: developer.owner,
        name: developer.name,
        github: developer.github,
        image: developer.image,
        description: developer.description,
        site: developer.site,
        version: developer.version,
        deleted_at: clock.timestamp_ms(),
    });

    let Developer { id, mut agents, .. } = developer;
    object::delete(id);
    vector::do!(agent_names, |s| {
        let agent = agents.remove(s);
        coordination::agent::destroy_agent(agent);
    });
    agents.destroy_empty();
}

public(package) fun add_agent_to_developer(
    developer: &mut Developer,
    agent: Agent,
    ctx: &TxContext,
) {
    assert!(developer.owner == ctx.sender(), EInvalidOwner);
    let agent_name = coordination::agent::agent_name(&agent);
    developer.agents.add(agent_name, agent);
}

public(package) fun remove_agent_from_developer(
    developer: &mut Developer,
    agent_name: String,
    ctx: &TxContext,
): Agent {
    assert!(developer.owner == ctx.sender(), EInvalidOwner);
    developer.agents.remove(agent_name)
}

public(package) fun get_agent_from_developer(
    developer: &Developer,
    agent_name: String,
): &Agent {
    developer.agents.borrow(agent_name)
}

public(package) fun get_agent_from_developer_mut(
    developer: &mut Developer,
    agent_name: String,
    ctx: &TxContext,
): &mut Agent {
    assert!(developer.owner == ctx.sender(), EInvalidOwner);
    developer.agents.borrow_mut(agent_name)
}

public(package) fun create_developer_names(
    developer: address,
    names: vector<String>,
    ctx: &mut TxContext,
): DeveloperNames {
    let developer_names = DeveloperNames {
        id: object::new(ctx),
        developer,
        names,
        version: 1,
    };
    event::emit(DeveloperNamesCreatedEvent {
        id: developer_names.id.to_address(),
        developer,
        names,
        version: 1,
    });
    developer_names
}

public(package) fun update_developer_names(
    developer_names: &mut DeveloperNames,
    names: vector<String>,
    ctx: &TxContext,
) {
    assert!(developer_names.developer == ctx.sender(), EInvalidOwner);
    developer_names.names = names;
    developer_names.version = developer_names.version + 1;
    event::emit(DeveloperNamesUpdatedEvent {
        id: developer_names.id.to_address(),
        developer: developer_names.developer,
        names,
        version: developer_names.version,
    });
}

public(package) fun add_name_to_developer_names(
    developer_names: &mut DeveloperNames,
    name: String,
    ctx: &TxContext,
) {
    assert!(developer_names.developer == ctx.sender(), EInvalidOwner);
    developer_names.names.push_back(name);
    developer_names.version = developer_names.version + 1;
    event::emit(DeveloperNamesUpdatedEvent {
        id: developer_names.id.to_address(),
        developer: developer_names.developer,
        names: developer_names.names,
        version: developer_names.version,
    });
}

public(package) fun remove_name_from_developer_names(
    developer_names: &mut DeveloperNames,
    name: String,
    ctx: &TxContext,
) {
    assert!(developer_names.developer == ctx.sender(), EInvalidOwner);
    let (found, index) = vector::index_of(&developer_names.names, &name);
    if (found) {
        vector::remove(&mut developer_names.names, index);
    };
    developer_names.version = developer_names.version + 1;
    event::emit(DeveloperNamesUpdatedEvent {
        id: developer_names.id.to_address(),
        developer: developer_names.developer,
        names: developer_names.names,
        version: developer_names.version,
    });
}

public(package) fun delete_developer_names(
    developer_names: DeveloperNames,
    ctx: &TxContext,
) {
    assert!(developer_names.developer == ctx.sender(), EInvalidOwner);
    let DeveloperNames { id, .. } = developer_names;
    object::delete(id);
}

// Admin versions of functions that don't check ownership
public(package) fun admin_remove_name_from_developer_names(
    developer_names: &mut DeveloperNames,
    name: String,
) {
    let (found, index) = vector::index_of(&developer_names.names, &name);
    if (found) {
        vector::remove(&mut developer_names.names, index);
    };
    developer_names.version = developer_names.version + 1;
    event::emit(DeveloperNamesUpdatedEvent {
        id: developer_names.id.to_address(),
        developer: developer_names.developer,
        names: developer_names.names,
        version: developer_names.version,
    });
}

public(package) fun admin_delete_developer_names(
    developer_names: DeveloperNames,
) {
    let DeveloperNames { id, .. } = developer_names;
    object::delete(id);
}

public(package) fun admin_delete_developer(
    developer: Developer,
    agent_names: vector<String>,
    clock: &Clock,
) {
    event::emit(DeveloperDeletedEvent {
        id: developer.owner,
        name: developer.name,
        github: developer.github,
        image: developer.image,
        description: developer.description,
        site: developer.site,
        version: developer.version,
        deleted_at: clock.timestamp_ms(),
    });

    let Developer { id, mut agents, .. } = developer;
    object::delete(id);
    vector::do!(agent_names, |s| {
        let agent = agents.remove(s);
        coordination::agent::destroy_agent(agent);
    });
    agents.destroy_empty();
}

// Getter functions
public(package) fun developer_owner(developer: &Developer): address {
    developer.owner
}

public(package) fun developer_name(developer: &Developer): String {
    developer.name
}

public(package) fun developer_names_is_empty(
    developer_names: &DeveloperNames,
): bool {
    vector::is_empty(&developer_names.names)
}

// Method management functions for agents within developer
public(package) fun add_method_to_agent(
    developer: &mut Developer,
    agent_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(developer.owner == ctx.sender(), EInvalidOwner);
    let agent = developer.agents.borrow_mut(agent_name);
    coordination::agent::registry_add_method(
        agent,
        developer.name,
        method_name,
        docker_image,
        docker_sha256,
        min_memory_gb,
        min_cpu_cores,
        requires_tee,
        clock,
    );
}

public(package) fun update_method_on_agent(
    developer: &mut Developer,
    agent_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(developer.owner == ctx.sender(), EInvalidOwner);
    let agent = developer.agents.borrow_mut(agent_name);
    coordination::agent::registry_update_method(
        agent,
        developer.name,
        method_name,
        docker_image,
        docker_sha256,
        min_memory_gb,
        min_cpu_cores,
        requires_tee,
        clock,
    );
}

public(package) fun remove_method_from_agent(
    developer: &mut Developer,
    agent_name: String,
    method_name: String,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(developer.owner == ctx.sender(), EInvalidOwner);
    let agent = developer.agents.borrow_mut(agent_name);
    coordination::agent::registry_remove_method(
        agent,
        developer.name,
        method_name,
        clock,
    );
}

public(package) fun set_default_method_on_agent(
    developer: &mut Developer,
    agent_name: String,
    method_name: String,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(developer.owner == ctx.sender(), EInvalidOwner);
    let agent = developer.agents.borrow_mut(agent_name);
    coordination::agent::registry_set_default_method(
        agent,
        developer.name,
        method_name,
        clock,
    );
}

public(package) fun remove_default_method_from_agent(
    developer: &mut Developer,
    agent_name: String,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(developer.owner == ctx.sender(), EInvalidOwner);
    let agent = developer.agents.borrow_mut(agent_name);
    coordination::agent::registry_remove_default_method(
        agent,
        developer.name,
        clock,
    );
}

public(package) fun update_agent_in_developer(
    developer: &mut Developer,
    agent_name: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    chains: vector<String>,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(developer.owner == ctx.sender(), EInvalidOwner);
    let agent = developer.agents.borrow_mut(agent_name);
    coordination::agent::registry_update_agent(
        agent,
        image,
        description,
        site,
        chains,
        clock,
    );
}

// Functions for registry use - operate on developers table directly
public(package) fun registry_update_agent(
    developers: &mut object_table::ObjectTable<String, Developer>,
    developer_name: String,
    agent_name: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    chains: vector<String>,
    clock: &Clock,
    ctx: &TxContext,
) {
    let developer = developers.borrow_mut(developer_name);
    update_agent_in_developer(
        developer,
        agent_name,
        image,
        description,
        site,
        chains,
        clock,
        ctx,
    );
}

public(package) fun registry_add_method(
    developers: &mut object_table::ObjectTable<String, Developer>,
    developer_name: String,
    agent_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
    clock: &Clock,
    ctx: &TxContext,
) {
    let developer = developers.borrow_mut(developer_name);
    add_method_to_agent(
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

public(package) fun registry_update_method(
    developers: &mut object_table::ObjectTable<String, Developer>,
    developer_name: String,
    agent_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
    clock: &Clock,
    ctx: &TxContext,
) {
    let developer = developers.borrow_mut(developer_name);
    update_method_on_agent(
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

public(package) fun registry_remove_method(
    developers: &mut object_table::ObjectTable<String, Developer>,
    developer_name: String,
    agent_name: String,
    method_name: String,
    clock: &Clock,
    ctx: &TxContext,
) {
    let developer = developers.borrow_mut(developer_name);
    remove_method_from_agent(
        developer,
        agent_name,
        method_name,
        clock,
        ctx,
    );
}

public(package) fun registry_set_default_method(
    developers: &mut object_table::ObjectTable<String, Developer>,
    developer_name: String,
    agent_name: String,
    method_name: String,
    clock: &Clock,
    ctx: &TxContext,
) {
    let developer = developers.borrow_mut(developer_name);
    set_default_method_on_agent(
        developer,
        agent_name,
        method_name,
        clock,
        ctx,
    );
}

public(package) fun registry_remove_default_method(
    developers: &mut object_table::ObjectTable<String, Developer>,
    developer_name: String,
    agent_name: String,
    clock: &Clock,
    ctx: &TxContext,
) {
    let developer = developers.borrow_mut(developer_name);
    remove_default_method_from_agent(
        developer,
        agent_name,
        clock,
        ctx,
    );
}
