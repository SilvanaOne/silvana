//! Proto definitions for Silvana
//!
//! This crate contains all the protobuf definitions and generated types
//! that are shared across different Silvana components.

use once_cell::sync::Lazy;
use prost_reflect::DescriptorPool;
use prost_reflect::ReflectMessage;

pub mod events {
    tonic::include_proto!("silvana.events.v1");

    // File descriptor set for gRPC reflection
    pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("events_descriptor");
}

// Descriptor pool for prost-reflect
pub static DESCRIPTOR_POOL: Lazy<DescriptorPool> =
    Lazy::new(|| DescriptorPool::decode(events::FILE_DESCRIPTOR_SET.as_ref()).unwrap());

// Include the buffer module
pub mod buffer;

// Re-export commonly used types for convenience
pub use events::*;

/// Get the protobuf full name for any message type that implements ReflectMessage
///
/// # Example
/// ```
/// use proto::{get_protobuf_full_name, CoordinatorStartedEvent};
///
/// let full_name = get_protobuf_full_name::<CoordinatorStartedEvent>();
/// assert_eq!(full_name, "silvana.events.v1.CoordinatorStartedEvent");
/// ```
pub fn get_protobuf_full_name<T: ReflectMessage + Default>() -> String {
    let instance = T::default();
    let descriptor = instance.descriptor();
    descriptor.full_name().to_string()
}

/// Get the protobuf full name from a message instance
///
/// # Example
/// ```
/// use proto::{get_protobuf_full_name_from_instance, CoordinatorStartedEvent};
///
/// let event = CoordinatorStartedEvent::default();
/// let full_name = get_protobuf_full_name_from_instance(&event);
/// assert_eq!(full_name, "silvana.events.v1.CoordinatorStartedEvent");
/// ```
pub fn get_protobuf_full_name_from_instance<T: ReflectMessage>(instance: &T) -> String {
    instance.descriptor().full_name().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_reflect::ReflectMessage;

    #[test]
    fn test_prost_reflect_type_names() {
        // Test CoordinatorStartedEvent
        let coordinator_event = CoordinatorStartedEvent::default();
        let descriptor = coordinator_event.descriptor();
        let full_name = descriptor.full_name().to_string();
        println!("CoordinatorStartedEvent full name: {}", full_name);
        assert_eq!(full_name, "silvana.events.v1.CoordinatorStartedEvent");

        // Test AgentStartedJobEvent
        let agent_event = AgentStartedJobEvent::default();
        let descriptor = agent_event.descriptor();
        let full_name = descriptor.full_name().to_string();
        println!("AgentStartedJobEvent full name: {}", full_name);
        assert_eq!(full_name, "silvana.events.v1.AgentStartedJobEvent");

        // Test Event (the main event wrapper)
        let event = Event::default();
        let descriptor = event.descriptor();
        let full_name = descriptor.full_name().to_string();
        println!("Event full name: {}", full_name);
        assert_eq!(full_name, "silvana.events.v1.Event");
    }

    #[test]
    fn test_message_descriptor_details() {
        let coordinator_event = CoordinatorStartedEvent::default();
        let descriptor = coordinator_event.descriptor();

        println!("Message name: {}", descriptor.name());
        println!("Full name: {}", descriptor.full_name());
        println!("Package: {}", descriptor.package_name());

        // Check fields
        println!("Fields:");
        for field in descriptor.fields() {
            println!(
                "  - {} ({}): {:?}",
                field.name(),
                field.number(),
                field.kind()
            );
        }

        assert_eq!(descriptor.name(), "CoordinatorStartedEvent");
        assert_eq!(descriptor.package_name(), "silvana.events.v1");
        assert!(descriptor.fields().len() > 0, "Should have fields defined");
    }

    #[test]
    fn test_file_descriptor_set() {
        // Verify that the file descriptor set is accessible
        assert!(
            !events::FILE_DESCRIPTOR_SET.is_empty(),
            "File descriptor set should not be empty"
        );
        println!(
            "File descriptor set size: {} bytes",
            events::FILE_DESCRIPTOR_SET.len()
        );

        // You could also create a DescriptorPool from the file descriptor set
        use prost_reflect::DescriptorPool;
        let pool = DescriptorPool::decode(events::FILE_DESCRIPTOR_SET.as_ref())
            .expect("Should be able to decode file descriptor set");

        // Find our message in the pool
        let message_descriptor = pool
            .get_message_by_name("silvana.events.v1.CoordinatorStartedEvent")
            .expect("Should find CoordinatorStartedEvent in descriptor pool");

        println!("Found message: {}", message_descriptor.full_name());
        assert_eq!(
            message_descriptor.full_name(),
            "silvana.events.v1.CoordinatorStartedEvent"
        );
    }

    #[test]
    fn demonstration_like_your_example() {
        // This matches your example more closely
        println!("=== Demonstration like your example ===");

        // From the Rust type ➜ protobuf full name
        let coordinator_event = CoordinatorStartedEvent::default();
        let descriptor = coordinator_event.descriptor();
        let full_name = descriptor.full_name();
        println!("{}", full_name); // "silvana.events.v1.CoordinatorStartedEvent"

        // Test with another message type
        let agent_event = AgentStartedJobEvent::default();
        let descriptor = agent_event.descriptor();
        println!("{}", descriptor.full_name()); // "silvana.events.v1.AgentStartedJobEvent"

        // Show more reflection capabilities
        println!("Message details for {}:", full_name);
        println!("  Package: {}", descriptor.package_name());
        println!("  Name: {}", descriptor.name());
        println!("  Field count: {}", descriptor.fields().len());
    }

    #[test]
    fn test_utility_functions() {
        // Test the new utility functions
        let full_name = get_protobuf_full_name::<CoordinatorStartedEvent>();
        assert_eq!(full_name, "silvana.events.v1.CoordinatorStartedEvent");

        let event = AgentStartedJobEvent::default();
        let full_name = get_protobuf_full_name_from_instance(&event);
        assert_eq!(full_name, "silvana.events.v1.AgentStartedJobEvent");

        println!("Utility function results:");
        println!(
            "  CoordinatorStartedEvent: {}",
            get_protobuf_full_name::<CoordinatorStartedEvent>()
        );
        println!(
            "  AgentStartedJobEvent: {}",
            get_protobuf_full_name::<AgentStartedJobEvent>()
        );
        println!("  Event: {}", get_protobuf_full_name::<Event>());
    }
}
