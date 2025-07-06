//! Implementation of BufferableEvent for Event
//!
//! This module contains the implementation of BufferableEvent for Event.
//! It is used to estimate the size of the event and to get the event type name.
//!
//! # Example
//!
//! ```
//! use proto::Event;
//!
//!
use crate::{Event, get_protobuf_full_name_from_instance};
use buffer::BufferableEvent;
use prost::Message;

/// Implementation of BufferableEvent for Event
impl BufferableEvent for Event {
    fn estimate_size(&self) -> usize {
        self.encoded_len()
    }

    fn event_type_name(&self) -> String {
        get_protobuf_full_name_from_instance(self)
    }
}
