use proto::{
    AgentStartedJobEvent, CoordinatorStartedEvent, Event, get_protobuf_full_name_from_instance,
};

fn main() {
    println!("=== Using Proto Utility Functions ===");

    // Method 1: Get full name using proto structs
    println!("\nMethod 1 - Using proto structs:");
    let coordinator_event = CoordinatorStartedEvent::default();
    let full_name = get_protobuf_full_name_from_instance(&coordinator_event);
    println!("CoordinatorStartedEvent: {}", full_name);

    let agent_event = AgentStartedJobEvent::default();
    let agent_full_name = get_protobuf_full_name_from_instance(&agent_event);
    println!("AgentStartedJobEvent: {}", agent_full_name);

    let event_wrapper = Event::default();
    let event_full_name = get_protobuf_full_name_from_instance(&event_wrapper);
    println!("Event: {}", event_full_name);
}
