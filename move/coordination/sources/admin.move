module coordination::admin;

use sui::display;
use sui::event;
use sui::package;

public struct Admin has key {
    id: UID,
    address: address,
}

public struct AdminCreateEvent has copy, drop {
    address: address,
    admin: address,
}

public struct ADMIN has drop {}

fun init(otw: ADMIN, ctx: &mut TxContext) {
    let admin = Admin {
        id: object::new(ctx),
        address: ctx.sender(),
    };
    event::emit(AdminCreateEvent {
        address: admin.id.to_address(),
        admin: ctx.sender(),
    });
    transfer::transfer(admin, ctx.sender());

    let publisher = package::claim(otw, ctx);

    let admin_keys = vector[
        b"name".to_string(),
        b"project_url".to_string(),
        b"creator".to_string(),
    ];

    let admin_values = vector[
        b"Silvana Registry Admin".to_string(),
        b"https://app.silvana.one".to_string(),
        b"DFST".to_string(),
    ];

    let mut display_admin = display::new_with_fields<Admin>(
        &publisher,
        admin_keys,
        admin_values,
        ctx,
    );

    display_admin.update_version();
    transfer::public_transfer(publisher, ctx.sender());
    transfer::public_transfer(display_admin, ctx.sender());
}

public fun get_admin_address(admin: &Admin): address {
    admin.address
}
