module coordination::agent;

use std::string::String;
use sui::clock::{timestamp_ms, Clock};
use sui::event;
use sui::vec_map::VecMap;

public struct AgentMethod has copy, drop, store {
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
}

public struct Agent has key, store {
    id: UID,
    name: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    chains: vector<String>,
    methods: VecMap<String, AgentMethod>,
    default_method: Option<AgentMethod>,
    created_at: u64,
    updated_at: u64,
    version: u64,
}

public struct AgentCreatedEvent has copy, drop {
    id: address,
    name: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    chains: vector<String>,
    created_at: u64,
}

public struct AgentUpdatedEvent has copy, drop {
    id: address,
    name: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    chains: vector<String>,
    updated_at: u64,
    version: u64,
}

public struct AgentDeletedEvent has copy, drop {
    id: address,
    name: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    chains: vector<String>,
    version: u64,
    deleted_at: u64,
}

public struct MethodAddedEvent has copy, drop {
    agent_id: address,
    developer_name: String,
    agent_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
    agent_version: u64,
    added_at: u64,
}

public struct MethodUpdatedEvent has copy, drop {
    agent_id: address,
    developer_name: String,
    agent_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
    agent_version: u64,
    updated_at: u64,
}

public struct MethodRemovedEvent has copy, drop {
    agent_id: address,
    developer_name: String,
    agent_name: String,
    method_name: String,
    agent_version: u64,
    removed_at: u64,
}

public struct DefaultMethodSetEvent has copy, drop {
    agent_id: address,
    developer_name: String,
    agent_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
    agent_version: u64,
    set_at: u64,
}

public struct DefaultMethodRemovedEvent has copy, drop {
    agent_id: address,
    developer_name: String,
    agent_name: String,
    agent_version: u64,
    removed_at: u64,
}

#[error]
const EInvalidOwner: vector<u8> = b"Invalid owner";

public(package) fun create_agent(
    developer_owner: address,
    name: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    chains: vector<String>,
    clock: &Clock,
    ctx: &mut TxContext,
): Agent {
    assert!(developer_owner == ctx.sender(), EInvalidOwner);
    let agent_id = object::new(ctx);
    let address = agent_id.to_address();
    let timestamp = clock.timestamp_ms();
    let agent = Agent {
        id: agent_id,
        name,
        image,
        description,
        site,
        methods: sui::vec_map::empty<String, AgentMethod>(),
        default_method: option::none(),
        chains,
        created_at: timestamp,
        updated_at: timestamp,
        version: 1,
    };
    event::emit(AgentCreatedEvent {
        id: address,
        name,
        image,
        description,
        site,
        chains,
        created_at: timestamp,
    });
    agent
}

public(package) fun update_agent(
    agent: &mut Agent,
    owner: address,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    chains: vector<String>,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(owner == ctx.sender(), EInvalidOwner);
    let timestamp = clock.timestamp_ms();
    agent.image = image;
    agent.description = description;
    agent.site = site;
    agent.chains = chains;
    agent.updated_at = timestamp;
    agent.version = agent.version + 1;
    let address = agent.id.to_address();
    event::emit(AgentUpdatedEvent {
        id: address,
        name: agent.name,
        image,
        description,
        site,
        chains,
        updated_at: timestamp,
        version: agent.version,
    });
}

public(package) fun delete_agent(
    agent: Agent,
    owner: address,
    clock: &Clock,
    admin_address: address,
    ctx: &TxContext,
) {
    assert!(owner == ctx.sender() || admin_address == ctx.sender(), EInvalidOwner);
    let timestamp = clock.timestamp_ms();
    event::emit(AgentDeletedEvent {
        id: agent.id.to_address(),
        name: agent.name,
        image: agent.image,
        description: agent.description,
        site: agent.site,
        chains: agent.chains,
        version: agent.version,
        deleted_at: timestamp,
    });
    let Agent { id, .. } = agent;
    object::delete(id);
}

public(package) fun add_method(
    agent: &mut Agent,
    owner: address,
    developer_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(owner == ctx.sender(), EInvalidOwner);

    let method = AgentMethod {
        docker_image,
        docker_sha256,
        min_memory_gb,
        min_cpu_cores,
        requires_tee,
    };

    agent.methods.insert(method_name, method);
    agent.updated_at = clock.timestamp_ms();
    agent.version = agent.version + 1;

    event::emit(MethodAddedEvent {
        agent_id: agent.id.to_address(),
        developer_name,
        agent_name: agent.name,
        method_name,
        docker_image,
        docker_sha256,
        min_memory_gb,
        min_cpu_cores,
        requires_tee,
        agent_version: agent.version,
        added_at: agent.updated_at,
    });

    event::emit(AgentUpdatedEvent {
        id: agent.id.to_address(),
        name: agent.name,
        image: agent.image,
        description: agent.description,
        site: agent.site,
        chains: agent.chains,
        updated_at: agent.updated_at,
        version: agent.version,
    });
}

public(package) fun update_method(
    agent: &mut Agent,
    owner: address,
    developer_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(owner == ctx.sender(), EInvalidOwner);

    let method = AgentMethod {
        docker_image,
        docker_sha256,
        min_memory_gb,
        min_cpu_cores,
        requires_tee,
    };

    let (_, _) = agent.methods.remove(&method_name);
    agent.methods.insert(method_name, method);
    agent.updated_at = clock.timestamp_ms();
    agent.version = agent.version + 1;

    event::emit(MethodUpdatedEvent {
        agent_id: agent.id.to_address(),
        developer_name,
        agent_name: agent.name,
        method_name,
        docker_image,
        docker_sha256,
        min_memory_gb,
        min_cpu_cores,
        requires_tee,
        agent_version: agent.version,
        updated_at: agent.updated_at,
    });

    event::emit(AgentUpdatedEvent {
        id: agent.id.to_address(),
        name: agent.name,
        image: agent.image,
        description: agent.description,
        site: agent.site,
        chains: agent.chains,
        updated_at: agent.updated_at,
        version: agent.version,
    });
}

public(package) fun remove_method(
    agent: &mut Agent,
    owner: address,
    developer_name: String,
    method_name: String,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(owner == ctx.sender(), EInvalidOwner);

    let (_, _) = agent.methods.remove(&method_name);
    agent.updated_at = clock.timestamp_ms();
    agent.version = agent.version + 1;

    event::emit(MethodRemovedEvent {
        agent_id: agent.id.to_address(),
        developer_name,
        agent_name: agent.name,
        method_name,
        agent_version: agent.version,
        removed_at: agent.updated_at,
    });

    event::emit(AgentUpdatedEvent {
        id: agent.id.to_address(),
        name: agent.name,
        image: agent.image,
        description: agent.description,
        site: agent.site,
        chains: agent.chains,
        updated_at: agent.updated_at,
        version: agent.version,
    });
}

public(package) fun set_default_method(
    agent: &mut Agent,
    owner: address,
    developer_name: String,
    method_name: String,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(owner == ctx.sender(), EInvalidOwner);

    let method = *agent.methods.get(&method_name);
    agent.default_method = option::some(method);
    agent.updated_at = clock.timestamp_ms();
    agent.version = agent.version + 1;

    event::emit(DefaultMethodSetEvent {
        agent_id: agent.id.to_address(),
        developer_name,
        agent_name: agent.name,
        method_name,
        docker_image: method.docker_image,
        docker_sha256: method.docker_sha256,
        min_memory_gb: method.min_memory_gb,
        min_cpu_cores: method.min_cpu_cores,
        requires_tee: method.requires_tee,
        agent_version: agent.version,
        set_at: agent.updated_at,
    });

    event::emit(AgentUpdatedEvent {
        id: agent.id.to_address(),
        name: agent.name,
        image: agent.image,
        description: agent.description,
        site: agent.site,
        chains: agent.chains,
        updated_at: agent.updated_at,
        version: agent.version,
    });
}

public(package) fun remove_default_method(
    agent: &mut Agent,
    owner: address,
    developer_name: String,
    clock: &Clock,
    ctx: &TxContext,
) {
    assert!(owner == ctx.sender(), EInvalidOwner);

    agent.default_method = option::none();
    agent.updated_at = clock.timestamp_ms();
    agent.version = agent.version + 1;

    event::emit(DefaultMethodRemovedEvent {
        agent_id: agent.id.to_address(),
        developer_name,
        agent_name: agent.name,
        agent_version: agent.version,
        removed_at: agent.updated_at,
    });

    event::emit(AgentUpdatedEvent {
        id: agent.id.to_address(),
        name: agent.name,
        image: agent.image,
        description: agent.description,
        site: agent.site,
        chains: agent.chains,
        updated_at: agent.updated_at,
        version: agent.version,
    });
}

// Getter functions
public(package) fun agent_name(agent: &Agent): String {
    agent.name
}

public(package) fun agent_id(agent: &Agent): address {
    agent.id.to_address()
}

// Helper function to delete agent for use by other modules
public(package) fun destroy_agent(agent: Agent) {
    let Agent { id, .. } = agent;
    object::delete(id);
}

// Simplified functions for registry use (no ownership validation)
public(package) fun registry_update_agent(
    agent: &mut Agent,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    chains: vector<String>,
    clock: &Clock,
) {
    let timestamp = clock.timestamp_ms();
    agent.image = image;
    agent.description = description;
    agent.site = site;
    agent.chains = chains;
    agent.updated_at = timestamp;
    agent.version = agent.version + 1;
    let address = agent.id.to_address();
    event::emit(AgentUpdatedEvent {
        id: address,
        name: agent.name,
        image,
        description,
        site,
        chains,
        updated_at: timestamp,
        version: agent.version,
    });
}

public(package) fun registry_add_method(
    agent: &mut Agent,
    developer_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
    clock: &Clock,
) {
    let method = AgentMethod {
        docker_image,
        docker_sha256,
        min_memory_gb,
        min_cpu_cores,
        requires_tee,
    };

    agent.methods.insert(method_name, method);
    agent.updated_at = clock.timestamp_ms();
    agent.version = agent.version + 1;

    event::emit(MethodAddedEvent {
        agent_id: agent.id.to_address(),
        developer_name,
        agent_name: agent.name,
        method_name,
        docker_image,
        docker_sha256,
        min_memory_gb,
        min_cpu_cores,
        requires_tee,
        agent_version: agent.version,
        added_at: agent.updated_at,
    });

    event::emit(AgentUpdatedEvent {
        id: agent.id.to_address(),
        name: agent.name,
        image: agent.image,
        description: agent.description,
        site: agent.site,
        chains: agent.chains,
        updated_at: agent.updated_at,
        version: agent.version,
    });
}

public(package) fun registry_update_method(
    agent: &mut Agent,
    developer_name: String,
    method_name: String,
    docker_image: String,
    docker_sha256: Option<String>,
    min_memory_gb: u16,
    min_cpu_cores: u16,
    requires_tee: bool,
    clock: &Clock,
) {
    let method = AgentMethod {
        docker_image,
        docker_sha256,
        min_memory_gb,
        min_cpu_cores,
        requires_tee,
    };

    let (_, _) = agent.methods.remove(&method_name);
    agent.methods.insert(method_name, method);
    agent.updated_at = clock.timestamp_ms();
    agent.version = agent.version + 1;

    event::emit(MethodUpdatedEvent {
        agent_id: agent.id.to_address(),
        developer_name,
        agent_name: agent.name,
        method_name,
        docker_image,
        docker_sha256,
        min_memory_gb,
        min_cpu_cores,
        requires_tee,
        agent_version: agent.version,
        updated_at: agent.updated_at,
    });

    event::emit(AgentUpdatedEvent {
        id: agent.id.to_address(),
        name: agent.name,
        image: agent.image,
        description: agent.description,
        site: agent.site,
        chains: agent.chains,
        updated_at: agent.updated_at,
        version: agent.version,
    });
}

public(package) fun registry_remove_method(
    agent: &mut Agent,
    developer_name: String,
    method_name: String,
    clock: &Clock,
) {
    let (_, _) = agent.methods.remove(&method_name);
    agent.updated_at = clock.timestamp_ms();
    agent.version = agent.version + 1;

    event::emit(MethodRemovedEvent {
        agent_id: agent.id.to_address(),
        developer_name,
        agent_name: agent.name,
        method_name,
        agent_version: agent.version,
        removed_at: agent.updated_at,
    });

    event::emit(AgentUpdatedEvent {
        id: agent.id.to_address(),
        name: agent.name,
        image: agent.image,
        description: agent.description,
        site: agent.site,
        chains: agent.chains,
        updated_at: agent.updated_at,
        version: agent.version,
    });
}

public(package) fun registry_set_default_method(
    agent: &mut Agent,
    developer_name: String,
    method_name: String,
    clock: &Clock,
) {
    let method = *agent.methods.get(&method_name);
    agent.default_method = option::some(method);
    agent.updated_at = clock.timestamp_ms();
    agent.version = agent.version + 1;

    event::emit(DefaultMethodSetEvent {
        agent_id: agent.id.to_address(),
        developer_name,
        agent_name: agent.name,
        method_name,
        docker_image: method.docker_image,
        docker_sha256: method.docker_sha256,
        min_memory_gb: method.min_memory_gb,
        min_cpu_cores: method.min_cpu_cores,
        requires_tee: method.requires_tee,
        agent_version: agent.version,
        set_at: agent.updated_at,
    });

    event::emit(AgentUpdatedEvent {
        id: agent.id.to_address(),
        name: agent.name,
        image: agent.image,
        description: agent.description,
        site: agent.site,
        chains: agent.chains,
        updated_at: agent.updated_at,
        version: agent.version,
    });
}

public(package) fun registry_remove_default_method(
    agent: &mut Agent,
    developer_name: String,
    clock: &Clock,
) {
    agent.default_method = option::none();
    agent.updated_at = clock.timestamp_ms();
    agent.version = agent.version + 1;

    event::emit(DefaultMethodRemovedEvent {
        agent_id: agent.id.to_address(),
        developer_name,
        agent_name: agent.name,
        agent_version: agent.version,
        removed_at: agent.updated_at,
    });

    event::emit(AgentUpdatedEvent {
        id: agent.id.to_address(),
        name: agent.name,
        image: agent.image,
        description: agent.description,
        site: agent.site,
        chains: agent.chains,
        updated_at: agent.updated_at,
        version: agent.version,
    });
}
