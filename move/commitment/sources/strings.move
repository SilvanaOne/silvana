module commitment::string;

use commitment::commitment::get_mina_prime;
use std::string::{String, utf8, as_bytes};

const EInvalidText: u64 = 1;
const EInvalidField: u64 = 2;

// Follows the method described in the Mina documentation: Encoding.stringToFields
public fun string_to_field(text: String): u256 {
    assert!(text.length() <= 30, EInvalidText);
    let text_bytes = text.as_bytes();
    let mut field: u256 = 0;
    let mut bit_position = 0;
    let mut i = 0;

    // Pack bytes in little-endian order
    while (i < text_bytes.length()) {
        let byte_val = (*text_bytes.borrow(i) as u256);
        field = field | (byte_val << bit_position);
        bit_position = bit_position + 8;
        i = i + 1;
    };

    // Add STOP byte (0x01) at the end for invertibility
    field = field | (1u256 << bit_position);

    assert!(field < get_mina_prime(), EInvalidField);

    field
}

public fun field_to_string(field: u256): String {
    let mut bytes = vector::empty<u8>();
    let mut temp_field = field;
    let mut i = 0;

    // Extract bytes in little-endian order until we hit the STOP byte (0x01)
    while (i < 31 && temp_field > 0) {
        let byte = ((temp_field & 0xFF) as u8);
        if (byte == 1u8) {
            // Found STOP byte, stop processing
            break
        };
        bytes.push_back(byte);
        temp_field = temp_field >> 8;
        i = i + 1;
    };

    utf8(bytes)
}

#[test]
fun test_string_to_field_hello_world() {
    let text = utf8(b"Hello, world!");
    let field = string_to_field(text);

    // Expected value from TypeScript test: 22928018571998998000425702810952n
    assert!(field == 22928018571998998000425702810952, 0);
}

#[test]
fun test_field_to_string_hello_world() {
    let field: u256 = 22928018571998998000425702810952;
    let text = field_to_string(field);
    let expected = utf8(b"Hello, world!");
    assert!(text == expected, 0);
}

#[test]
fun test_string_roundtrip_hello_world() {
    let original = utf8(b"Hello, world!");
    let field = string_to_field(original);
    let recovered = field_to_string(field);
    assert!(original == recovered, 0);
}
