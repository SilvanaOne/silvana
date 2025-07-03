pub mod parser;
pub mod schema_validator;

pub use parser::{
    parse_proto_file, proto_type_to_mysql, proto_type_to_rust, ProtoField, ProtoMessage,
};
pub use schema_validator::SchemaValidator;
