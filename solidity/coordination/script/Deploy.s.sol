// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "forge-std/Script.sol";
import "../src/SilvanaCoordination.sol";
import "../src/JobManager.sol";
import "../src/AppInstanceManager.sol";
import "../src/ProofManager.sol";
import "../src/SettlementManager.sol";
import "../src/StorageManager.sol";

/**
 * @title DeployCoordination
 * @notice Deployment script for the Silvana Coordination Layer
 * @dev Deploys all core contracts and initializes them
 */
contract DeployCoordination is Script {
    function run() external {
        // Get deployment parameters from environment
        address admin = vm.envOr("ADMIN_ADDRESS", address(0));
        if (admin == address(0)) {
            admin = msg.sender;
        }

        // Start broadcast
        vm.startBroadcast();

        // Deploy JobManager
        JobManager jobManager = new JobManager();
        console.log("JobManager deployed at:", address(jobManager));

        // Deploy AppInstanceManager
        AppInstanceManager appInstanceManager = new AppInstanceManager();
        console.log("AppInstanceManager deployed at:", address(appInstanceManager));

        // Deploy ProofManager
        ProofManager proofManager = new ProofManager();
        console.log("ProofManager deployed at:", address(proofManager));

        // Deploy SettlementManager
        SettlementManager settlementManager = new SettlementManager();
        console.log("SettlementManager deployed at:", address(settlementManager));

        // Deploy StorageManager
        StorageManager storageManager = new StorageManager();
        console.log("StorageManager deployed at:", address(storageManager));

        // Deploy SilvanaCoordination
        SilvanaCoordination coordination = new SilvanaCoordination();
        console.log("SilvanaCoordination deployed at:", address(coordination));

        // Initialize SilvanaCoordination with all managers
        coordination.initialize(admin, address(jobManager), address(appInstanceManager), address(proofManager), address(settlementManager), address(storageManager));
        console.log("SilvanaCoordination initialized");

        // Set ProofManager in AppInstanceManager
        appInstanceManager.setProofManager(address(proofManager));
        console.log("ProofManager set in AppInstanceManager");

        // Grant roles in component contracts
        jobManager.grantRole(jobManager.COORDINATOR_ROLE(), address(coordination));
        appInstanceManager.grantRole(appInstanceManager.COORDINATOR_ROLE(), address(coordination));
        proofManager.grantRole(proofManager.COORDINATOR_ROLE(), address(coordination));
        settlementManager.grantRole(settlementManager.COORDINATOR_ROLE(), address(coordination));
        storageManager.grantRole(storageManager.COORDINATOR_ROLE(), address(coordination));

        // AppInstanceManager needs to call ProofManager.createProofCalculation
        proofManager.grantRole(proofManager.COORDINATOR_ROLE(), address(appInstanceManager));
        console.log("Roles granted to coordination contract and app instance manager");

        // Transfer admin roles if needed
        if (admin != msg.sender) {
            jobManager.transferAdmin(admin);
            appInstanceManager.transferAdmin(admin);
            proofManager.transferAdmin(admin);
            settlementManager.transferAdmin(admin);
            storageManager.transferAdmin(admin);
            coordination.transferAdmin(admin);
            console.log("Admin roles transferred to:", admin);
        }

        vm.stopBroadcast();

        // Log deployment summary
        console.log("\n=== Deployment Summary ===");
        console.log("Admin:", admin);
        console.log("SilvanaCoordination:", address(coordination));
        console.log("JobManager:", address(jobManager));
        console.log("AppInstanceManager:", address(appInstanceManager));
        console.log("ProofManager:", address(proofManager));
        console.log("SettlementManager:", address(settlementManager));
        console.log("StorageManager:", address(storageManager));
        console.log("==========================\n");
    }
}

/**
 * @title DeployLocal
 * @notice Local deployment script with test accounts
 * @dev Deploys with additional test setup for local testing
 */
contract DeployLocal is Script {
    function run() external {
        // Use first anvil account as admin
        address admin = address(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266);

        // Additional test accounts
        address coordinator1 = address(0x70997970C51812dc3A010C7d01b50e0d17dc79C8);
        address coordinator2 = address(0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC);
        address operator = address(0x90F79bf6EB2c4f870365E785982E1f101E93b906);

        vm.startBroadcast();

        // Deploy contracts
        JobManager jobManager = new JobManager();
        AppInstanceManager appInstanceManager = new AppInstanceManager();
        ProofManager proofManager = new ProofManager();
        SettlementManager settlementManager = new SettlementManager();
        StorageManager storageManager = new StorageManager();
        SilvanaCoordination coordination = new SilvanaCoordination();

        // Initialize with all managers
        coordination.initialize(admin, address(jobManager), address(appInstanceManager), address(proofManager), address(settlementManager), address(storageManager));

        // Set ProofManager in AppInstanceManager
        appInstanceManager.setProofManager(address(proofManager));

        // Grant roles to component contracts
        jobManager.grantRole(jobManager.COORDINATOR_ROLE(), address(coordination));
        appInstanceManager.grantRole(appInstanceManager.COORDINATOR_ROLE(), address(coordination));
        proofManager.grantRole(proofManager.COORDINATOR_ROLE(), address(coordination));
        settlementManager.grantRole(settlementManager.COORDINATOR_ROLE(), address(coordination));
        storageManager.grantRole(storageManager.COORDINATOR_ROLE(), address(coordination));
        proofManager.grantRole(proofManager.COORDINATOR_ROLE(), address(appInstanceManager));

        // Grant roles to coordinator and operator accounts
        coordination.grantRole(coordination.COORDINATOR_ROLE(), coordinator1);
        coordination.grantRole(coordination.COORDINATOR_ROLE(), coordinator2);
        coordination.grantRole(coordination.OPERATOR_ROLE(), operator);
        coordination.grantRole(coordination.PAUSER_ROLE(), operator);

        jobManager.grantRole(jobManager.COORDINATOR_ROLE(), coordinator1);
        jobManager.grantRole(jobManager.COORDINATOR_ROLE(), coordinator2);
        jobManager.grantRole(jobManager.OPERATOR_ROLE(), operator);

        appInstanceManager.grantRole(appInstanceManager.COORDINATOR_ROLE(), coordinator1);
        appInstanceManager.grantRole(appInstanceManager.COORDINATOR_ROLE(), coordinator2);
        appInstanceManager.grantRole(appInstanceManager.OPERATOR_ROLE(), operator);

        proofManager.grantRole(proofManager.COORDINATOR_ROLE(), coordinator1);
        proofManager.grantRole(proofManager.COORDINATOR_ROLE(), coordinator2);
        proofManager.grantRole(proofManager.OPERATOR_ROLE(), operator);

        settlementManager.grantRole(settlementManager.COORDINATOR_ROLE(), coordinator1);
        settlementManager.grantRole(settlementManager.COORDINATOR_ROLE(), coordinator2);
        settlementManager.grantRole(settlementManager.OPERATOR_ROLE(), operator);

        storageManager.grantRole(storageManager.COORDINATOR_ROLE(), coordinator1);
        storageManager.grantRole(storageManager.COORDINATOR_ROLE(), coordinator2);
        storageManager.grantRole(storageManager.OPERATOR_ROLE(), operator);

        vm.stopBroadcast();

        console.log("\n=== Local Deployment ===");
        console.log("Admin:", admin);
        console.log("Coordinator 1:", coordinator1);
        console.log("Coordinator 2:", coordinator2);
        console.log("Operator:", operator);
        console.log("\nContracts:");
        console.log("SilvanaCoordination:", address(coordination));
        console.log("JobManager:", address(jobManager));
        console.log("AppInstanceManager:", address(appInstanceManager));
        console.log("ProofManager:", address(proofManager));
        console.log("SettlementManager:", address(settlementManager));
        console.log("StorageManager:", address(storageManager));
        console.log("========================\n");
    }
}