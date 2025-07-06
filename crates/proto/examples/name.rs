use proto::{
    AgentStartedJobEvent, CoordinatorStartedEvent, Event, get_protobuf_full_name_from_instance,
};

fn main() {
    let coordinator_event = CoordinatorStartedEvent::default();
    let full_name = get_protobuf_full_name_from_instance(&coordinator_event);
    println!("CoordinatorStartedEvent: {}", full_name);

    let agent_event = AgentStartedJobEvent::default();
    let agent_full_name = get_protobuf_full_name_from_instance(&agent_event);
    println!("AgentStartedJobEvent: {}", agent_full_name);

    let event_wrapper = Event::default();
    let event_full_name = get_protobuf_full_name_from_instance(&event_wrapper);
    println!("Event name: {}", event_full_name);
    println!("Event: {:?}", event_wrapper);
    let event_type = event_wrapper.event_type;
    println!("Event type: {:?}", event_type);
}
