module coordination::app_method;

use std::string::String;

public struct AppMethod has copy, drop, store {
    description: Option<String>,
    developer: String,
    agent: String,
    agent_method: String,
}

// Constructor
public fun new(
    description: Option<String>,
    developer: String,
    agent: String,
    agent_method: String,
): AppMethod {
    AppMethod {
        description,
        developer,
        agent,
        agent_method,
    }
}

// Getters
public fun description(method: &AppMethod): &Option<String> {
    &method.description
}

public fun developer(method: &AppMethod): &String {
    &method.developer
}

public fun agent(method: &AppMethod): &String {
    &method.agent
}

public fun agent_method(method: &AppMethod): &String {
    &method.agent_method
}