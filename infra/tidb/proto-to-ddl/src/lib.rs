pub mod parser;
pub mod schema_validator;
pub mod fulltext_indexes;

pub use parser::{
    ProtoField, ProtoMessage, parse_proto_file, proto_type_to_mysql_with_options,
    proto_type_to_rust,
};
pub use schema_validator::SchemaValidator;
pub use fulltext_indexes::{generate_fulltext_index_metadata, generate_fulltext_index_list};
