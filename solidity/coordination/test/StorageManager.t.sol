// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "forge-std/Test.sol";
import "../src/StorageManager.sol";
import "../src/libraries/DataTypes.sol";

contract StorageManagerTest is Test {
    StorageManager public storageManager;

    address admin = address(0x1);
    address coordinator = address(0x2);

    string constant APP_INSTANCE = "test-app-instance";
    string constant KEY1 = "key1";
    string constant KEY2 = "key2";
    string constant VALUE1 = "value1";
    string constant VALUE2 = "value2";
    bytes constant BINARY_KEY1 = "binary_key_1";
    bytes constant BINARY_KEY2 = "binary_key_2";
    bytes constant BINARY_VALUE1 = "binary_data_1";
    bytes constant BINARY_VALUE2 = "binary_data_2";

    function setUp() public {
        vm.startPrank(admin);
        storageManager = new StorageManager();
        storageManager.grantRole(storageManager.COORDINATOR_ROLE(), coordinator);
        vm.stopPrank();
    }

    // ============ String KV Storage Tests ============

    function testSetAndGetKvString() public {
        vm.prank(coordinator);
        storageManager.setKvString(APP_INSTANCE, KEY1, VALUE1);

        (string memory value, bool exists) = storageManager.getKvString(APP_INSTANCE, KEY1);
        assertTrue(exists);
        assertEq(value, VALUE1);
    }

    function testGetNonExistentKvString() public view {
        (string memory value, bool exists) = storageManager.getKvString(APP_INSTANCE, "nonexistent");
        assertFalse(exists);
        assertEq(bytes(value).length, 0);
    }

    function testUpdateKvString() public {
        vm.startPrank(coordinator);
        storageManager.setKvString(APP_INSTANCE, KEY1, VALUE1);
        storageManager.setKvString(APP_INSTANCE, KEY1, VALUE2);
        vm.stopPrank();

        (string memory value, bool exists) = storageManager.getKvString(APP_INSTANCE, KEY1);
        assertTrue(exists);
        assertEq(value, VALUE2);
    }

    function testDeleteKvString() public {
        vm.startPrank(coordinator);
        storageManager.setKvString(APP_INSTANCE, KEY1, VALUE1);
        storageManager.deleteKvString(APP_INSTANCE, KEY1);
        vm.stopPrank();

        (string memory value, bool exists) = storageManager.getKvString(APP_INSTANCE, KEY1);
        assertFalse(exists);
        assertEq(bytes(value).length, 0);
    }

    function testDeleteNonExistentKvString() public {
        vm.prank(coordinator);
        vm.expectRevert("StorageManager: Key not found");
        storageManager.deleteKvString(APP_INSTANCE, "nonexistent");
    }

    function testListKvStringKeys() public {
        vm.startPrank(coordinator);
        storageManager.setKvString(APP_INSTANCE, KEY1, VALUE1);
        storageManager.setKvString(APP_INSTANCE, KEY2, VALUE2);
        vm.stopPrank();

        string[] memory keys = storageManager.listKvStringKeys(APP_INSTANCE);
        assertEq(keys.length, 2);
        assertEq(keys[0], KEY1);
        assertEq(keys[1], KEY2);
    }

    function testGetAllKvString() public {
        vm.startPrank(coordinator);
        storageManager.setKvString(APP_INSTANCE, KEY1, VALUE1);
        storageManager.setKvString(APP_INSTANCE, KEY2, VALUE2);
        vm.stopPrank();

        (string[] memory keys, string[] memory values) = storageManager.getAllKvString(APP_INSTANCE);
        assertEq(keys.length, 2);
        assertEq(values.length, 2);
        assertEq(keys[0], KEY1);
        assertEq(values[0], VALUE1);
        assertEq(keys[1], KEY2);
        assertEq(values[1], VALUE2);
    }

    function testSetKvStringEmitsEvent() public {
        vm.prank(coordinator);
        vm.expectEmit(true, true, true, true);
        emit DataTypes.KvStringSetEvent(APP_INSTANCE, KEY1, VALUE1, block.timestamp);
        storageManager.setKvString(APP_INSTANCE, KEY1, VALUE1);
    }

    function testDeleteKvStringEmitsEvent() public {
        vm.startPrank(coordinator);
        storageManager.setKvString(APP_INSTANCE, KEY1, VALUE1);

        vm.expectEmit(true, true, true, true);
        emit DataTypes.KvStringDeletedEvent(APP_INSTANCE, KEY1, block.timestamp);
        storageManager.deleteKvString(APP_INSTANCE, KEY1);
        vm.stopPrank();
    }

    function testKeyTooLong() public {
        // Create a key longer than MAX_KEY_LENGTH (256)
        string memory longKey = new string(257);

        vm.prank(coordinator);
        vm.expectRevert("StorageManager: Invalid key length");
        storageManager.setKvString(APP_INSTANCE, longKey, VALUE1);
    }

    // ============ Binary KV Storage Tests ============

    function testSetAndGetKvBinary() public {
        vm.prank(coordinator);
        storageManager.setKvBinary(APP_INSTANCE, BINARY_KEY1, BINARY_VALUE1);

        (bytes memory value, bool exists) = storageManager.getKvBinary(APP_INSTANCE, BINARY_KEY1);
        assertTrue(exists);
        assertEq(value, BINARY_VALUE1);
    }

    function testGetNonExistentKvBinary() public view {
        (bytes memory value, bool exists) = storageManager.getKvBinary(APP_INSTANCE, "nonexistent");
        assertFalse(exists);
        assertEq(value.length, 0);
    }

    function testDeleteKvBinary() public {
        vm.startPrank(coordinator);
        storageManager.setKvBinary(APP_INSTANCE, BINARY_KEY1, BINARY_VALUE1);
        storageManager.deleteKvBinary(APP_INSTANCE, BINARY_KEY1);
        vm.stopPrank();

        (bytes memory value, bool exists) = storageManager.getKvBinary(APP_INSTANCE, BINARY_KEY1);
        assertFalse(exists);
        assertEq(value.length, 0);
    }

    function testListKvBinaryKeys() public {
        vm.startPrank(coordinator);
        storageManager.setKvBinary(APP_INSTANCE, BINARY_KEY1, BINARY_VALUE1);
        storageManager.setKvBinary(APP_INSTANCE, BINARY_KEY2, BINARY_VALUE2);
        vm.stopPrank();

        bytes[] memory keys = storageManager.listKvBinaryKeys(APP_INSTANCE);
        assertEq(keys.length, 2);
        assertEq(keys[0], BINARY_KEY1);
        assertEq(keys[1], BINARY_KEY2);
    }

    function testSetKvBinaryEmitsEvent() public {
        vm.prank(coordinator);
        vm.expectEmit(true, true, true, true);
        emit DataTypes.KvBinarySetEvent(APP_INSTANCE, BINARY_KEY1, BINARY_VALUE1, block.timestamp);
        storageManager.setKvBinary(APP_INSTANCE, BINARY_KEY1, BINARY_VALUE1);
    }

    function testDeleteKvBinaryEmitsEvent() public {
        vm.startPrank(coordinator);
        storageManager.setKvBinary(APP_INSTANCE, BINARY_KEY1, BINARY_VALUE1);

        vm.expectEmit(true, true, true, true);
        emit DataTypes.KvBinaryDeletedEvent(APP_INSTANCE, BINARY_KEY1, block.timestamp);
        storageManager.deleteKvBinary(APP_INSTANCE, BINARY_KEY1);
        vm.stopPrank();
    }

    // ============ Metadata Tests ============

    function testSetAndGetMetadata() public {
        vm.prank(coordinator);
        storageManager.setMetadata(APP_INSTANCE, KEY1, VALUE1);

        (string memory value, bool exists) = storageManager.getMetadata(APP_INSTANCE, KEY1);
        assertTrue(exists);
        assertEq(value, VALUE1);
    }

    function testGetNonExistentMetadata() public view {
        (string memory value, bool exists) = storageManager.getMetadata(APP_INSTANCE, "nonexistent");
        assertFalse(exists);
        assertEq(bytes(value).length, 0);
    }

    function testUpdateMetadata() public {
        vm.startPrank(coordinator);
        storageManager.setMetadata(APP_INSTANCE, KEY1, VALUE1);
        storageManager.setMetadata(APP_INSTANCE, KEY1, VALUE2);
        vm.stopPrank();

        (string memory value, bool exists) = storageManager.getMetadata(APP_INSTANCE, KEY1);
        assertTrue(exists);
        assertEq(value, VALUE2);
    }

    function testDeleteMetadata() public {
        vm.startPrank(coordinator);
        storageManager.setMetadata(APP_INSTANCE, KEY1, VALUE1);
        storageManager.deleteMetadata(APP_INSTANCE, KEY1);
        vm.stopPrank();

        (string memory value, bool exists) = storageManager.getMetadata(APP_INSTANCE, KEY1);
        assertFalse(exists);
        assertEq(bytes(value).length, 0);
    }

    function testDeleteNonExistentMetadata() public {
        vm.prank(coordinator);
        vm.expectRevert("StorageManager: Key not found");
        storageManager.deleteMetadata(APP_INSTANCE, "nonexistent");
    }

    // ============ Access Control Tests ============

    function testSetKvStringRequiresCoordinator() public {
        vm.prank(address(0x999));
        vm.expectRevert();
        storageManager.setKvString(APP_INSTANCE, KEY1, VALUE1);
    }

    function testSetKvBinaryRequiresCoordinator() public {
        vm.prank(address(0x999));
        vm.expectRevert();
        storageManager.setKvBinary(APP_INSTANCE, BINARY_KEY1, BINARY_VALUE1);
    }

    function testSetMetadataRequiresCoordinator() public {
        vm.prank(address(0x999));
        vm.expectRevert();
        storageManager.setMetadata(APP_INSTANCE, KEY1, VALUE1);
    }

    // ============ Mixed Storage Test ============

    function testMixedStorageIsolation() public {
        vm.startPrank(coordinator);

        // Set data in all three storage systems with same key
        storageManager.setKvString(APP_INSTANCE, KEY1, VALUE1);
        storageManager.setKvBinary(APP_INSTANCE, BINARY_KEY1, BINARY_VALUE1);
        storageManager.setMetadata(APP_INSTANCE, KEY1, "metadata_value");

        vm.stopPrank();

        // Verify all three are independent
        (string memory strValue, bool strExists) = storageManager.getKvString(APP_INSTANCE, KEY1);
        (bytes memory binValue, bool binExists) = storageManager.getKvBinary(APP_INSTANCE, BINARY_KEY1);
        (string memory metaValue, bool metaExists) = storageManager.getMetadata(APP_INSTANCE, KEY1);

        assertTrue(strExists);
        assertTrue(binExists);
        assertTrue(metaExists);
        assertEq(strValue, VALUE1);
        assertEq(binValue, BINARY_VALUE1);
        assertEq(metaValue, "metadata_value");
    }
}
