// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

/**
 * @title IAccessControl
 * @notice Interface for access control management
 * @dev Defines role-based access control interface
 */
interface IAccessControl {
    // ============ Events ============

    /**
     * @notice Emitted when a role is granted to an account
     */
    event RoleGranted(bytes32 indexed role, address indexed account, address indexed sender);

    /**
     * @notice Emitted when a role is revoked from an account
     */
    event RoleRevoked(bytes32 indexed role, address indexed account, address indexed sender);

    /**
     * @notice Emitted when admin role is transferred
     */
    event AdminTransferred(address indexed previousAdmin, address indexed newAdmin);

    // ============ Role Management ============

    /**
     * @notice Grant a role to an account
     * @param role The role to grant
     * @param account The account to grant the role to
     */
    function grantRole(bytes32 role, address account) external;

    /**
     * @notice Revoke a role from an account
     * @param role The role to revoke
     * @param account The account to revoke the role from
     */
    function revokeRole(bytes32 role, address account) external;

    /**
     * @notice Renounce a role
     * @dev Caller must have the role
     * @param role The role to renounce
     */
    function renounceRole(bytes32 role) external;

    /**
     * @notice Transfer admin role to a new account
     * @param newAdmin The new admin address
     */
    function transferAdmin(address newAdmin) external;

    // ============ Role Queries ============

    /**
     * @notice Check if an account has a specific role
     * @param role The role to check
     * @param account The account to check
     * @return hasRole True if the account has the role
     */
    function hasRole(bytes32 role, address account) external view returns (bool hasRole);

    /**
     * @notice Get the admin address
     * @return admin The admin address
     */
    function getAdmin() external view returns (address admin);

    /**
     * @notice Get the role admin for a specific role
     * @param role The role to query
     * @return adminRole The admin role for the given role
     */
    function getRoleAdmin(bytes32 role) external view returns (bytes32 adminRole);

    // ============ Role Constants ============

    /**
     * @notice Get the admin role identifier
     * @return role The admin role bytes32
     */
    function ADMIN_ROLE() external pure returns (bytes32 role);

    /**
     * @notice Get the operator role identifier
     * @return role The operator role bytes32
     */
    function OPERATOR_ROLE() external pure returns (bytes32 role);

    /**
     * @notice Get the coordinator role identifier
     * @return role The coordinator role bytes32
     */
    function COORDINATOR_ROLE() external pure returns (bytes32 role);

    /**
     * @notice Get the pauser role identifier
     * @return role The pauser role bytes32
     */
    function PAUSER_ROLE() external pure returns (bytes32 role);

    /**
     * @notice Get the upgrader role identifier
     * @return role The upgrader role bytes32
     */
    function UPGRADER_ROLE() external pure returns (bytes32 role);
}