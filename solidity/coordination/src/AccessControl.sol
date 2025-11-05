// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "./interfaces/IAccessControl.sol";

/**
 * @title AccessControl
 * @notice Implements role-based access control
 * @dev Provides role management and access control modifiers
 */
abstract contract AccessControl is IAccessControl {
    // ============ Storage ============

    struct RoleData {
        mapping(address => bool) members;
        bytes32 adminRole;
    }

    mapping(bytes32 => RoleData) private _roles;
    address private _admin;

    // ============ Role Constants ============

    bytes32 public constant override ADMIN_ROLE = 0x00;
    bytes32 public constant override OPERATOR_ROLE = keccak256("OPERATOR");
    bytes32 public constant override COORDINATOR_ROLE = keccak256("COORDINATOR");
    bytes32 public constant override PAUSER_ROLE = keccak256("PAUSER");
    bytes32 public constant override UPGRADER_ROLE = keccak256("UPGRADER");

    // ============ Modifiers ============

    /**
     * @dev Modifier to check if caller has a specific role
     */
    modifier onlyRole(bytes32 role) {
        _checkRole(role, msg.sender);
        _;
    }

    /**
     * @dev Modifier to check if caller is admin
     */
    modifier onlyAdmin() {
        _checkRole(ADMIN_ROLE, msg.sender);
        _;
    }

    // ============ Constructor ============

    /**
     * @dev Sets the initial admin
     */
    constructor() {
        _grantRole(ADMIN_ROLE, msg.sender);
        _admin = msg.sender;
    }

    // ============ External Functions ============

    /**
     * @inheritdoc IAccessControl
     */
    function grantRole(bytes32 role, address account) external override onlyRole(getRoleAdmin(role)) {
        _grantRole(role, account);
    }

    /**
     * @inheritdoc IAccessControl
     */
    function revokeRole(bytes32 role, address account) external override onlyRole(getRoleAdmin(role)) {
        _revokeRole(role, account);
    }

    /**
     * @inheritdoc IAccessControl
     */
    function renounceRole(bytes32 role) external override {
        _revokeRole(role, msg.sender);
    }

    /**
     * @inheritdoc IAccessControl
     */
    function transferAdmin(address newAdmin) external override onlyAdmin {
        require(newAdmin != address(0), "AccessControl: new admin is zero address");
        address previousAdmin = _admin;
        _revokeRole(ADMIN_ROLE, previousAdmin);
        _grantRole(ADMIN_ROLE, newAdmin);
        _admin = newAdmin;
        emit AdminTransferred(previousAdmin, newAdmin);
    }

    // ============ Public View Functions ============

    /**
     * @inheritdoc IAccessControl
     */
    function hasRole(bytes32 role, address account) public view override returns (bool) {
        return _roles[role].members[account];
    }

    /**
     * @inheritdoc IAccessControl
     */
    function getAdmin() public view override returns (address) {
        return _admin;
    }

    /**
     * @inheritdoc IAccessControl
     */
    function getRoleAdmin(bytes32 role) public pure override returns (bytes32) {
        return ADMIN_ROLE;
    }

    // ============ Internal Functions ============

    /**
     * @dev Grants a role to an account
     */
    function _grantRole(bytes32 role, address account) internal {
        if (!hasRole(role, account)) {
            _roles[role].members[account] = true;
            emit RoleGranted(role, account, msg.sender);
        }
    }

    /**
     * @dev Revokes a role from an account
     */
    function _revokeRole(bytes32 role, address account) internal {
        if (hasRole(role, account)) {
            _roles[role].members[account] = false;
            emit RoleRevoked(role, account, msg.sender);
        }
    }

    /**
     * @dev Checks if an account has a role, reverts if not
     */
    function _checkRole(bytes32 role, address account) internal view {
        if (!hasRole(role, account)) {
            revert(
                string(
                    abi.encodePacked(
                        "AccessControl: account ",
                        _toHexString(account),
                        " is missing role ",
                        _toHexString(uint256(role), 32)
                    )
                )
            );
        }
    }

    /**
     * @dev Converts an address to its ASCII string hexadecimal representation
     */
    function _toHexString(address addr) internal pure returns (string memory) {
        return _toHexString(uint256(uint160(addr)), 20);
    }

    /**
     * @dev Converts a uint256 to its ASCII string hexadecimal representation with fixed length
     */
    function _toHexString(uint256 value, uint256 length) internal pure returns (string memory) {
        bytes memory buffer = new bytes(2 * length + 2);
        buffer[0] = "0";
        buffer[1] = "x";
        for (uint256 i = 2 * length + 1; i > 1; --i) {
            uint8 digit = uint8(value & 0xf);
            buffer[i] = bytes1(digit < 10 ? 48 + digit : 87 + digit);
            value >>= 4;
        }
        require(value == 0, "AccessControl: hex length insufficient");
        return string(buffer);
    }

    /**
     * @dev Sets up a role's admin role
     */
    function _setRoleAdmin(bytes32 role, bytes32 adminRole) internal {
        _roles[role].adminRole = adminRole;
    }
}