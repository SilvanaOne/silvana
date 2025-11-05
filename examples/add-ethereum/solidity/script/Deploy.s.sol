// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "forge-std/Script.sol";
import "../src/AddApp.sol";
import "coordination/interfaces/ICoordination.sol";
import "coordination/interfaces/IAppInstanceManager.sol";

/**
 * @title DeployAddApp
 * @notice Deployment script for the AddApp example
 *
 * Usage:
 *   # Deploy to local Anvil (requires deployed coordination contracts)
 *   forge script script/Deploy.s.sol:DeployAddApp --rpc-url localhost --broadcast --legacy
 *
 *   # Deploy to testnet
 *   forge script script/Deploy.s.sol:DeployAddApp --rpc-url $RPC_URL --broadcast --verify --legacy
 */
contract DeployAddApp is Script {
    function run() external {
        // Load deployment info for coordination contract address
        // On local Anvil, this should point to the deployed SilvanaCoordination
        address coordinationAddress = vm.envAddress("SILVANA_COORDINATION_CONTRACT");

        // Get deployer private key
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        address deployer = vm.addr(deployerPrivateKey);

        console.log("================================================");
        console.log("Deploying AddApp Example");
        console.log("================================================");
        console.log("Deployer:", deployer);
        console.log("Coordination:", coordinationAddress);
        console.log("");

        vm.startBroadcast(deployerPrivateKey);

        // 1. Deploy AddApp contract
        AddApp addApp = new AddApp(coordinationAddress);
        console.log("AddApp deployed at:", address(addApp));

        // Generate Ed25519 public key as app instance ID
        string[] memory inputs = new string[](1);
        inputs[0] = "script/generate_ed25519_id.sh";
        bytes memory result = vm.ffi(inputs);
        // Convert bytes to 0x-prefixed hex string
        string memory appInstanceId = vm.toString(result);
        console.log("Generated app instance ID:", appInstanceId);

        // 2. Create app instance in coordination layer
        ICoordination coordination = ICoordination(coordinationAddress);
        IAppInstanceManager appManager = coordination.appInstanceManager();

        // Get app metadata from environment variables
        string memory appName = vm.envOr("APP_NAME", string("add-app-demo"));
        string memory agentName = vm.envOr("AGENT_NAME", string("add-agent"));
        string memory developerName = vm.envOr("DEVELOPER_NAME", string("silvana"));

        // Create app instance with the generated ID
        string memory createdId = appManager.createAppInstance(
            appInstanceId,
            appName,
            agentName,
            developerName
        );
        console.log("App instance created:", createdId);

        // 3. Initialize AddApp with the instance ID
        addApp.initialize(appInstanceId);
        console.log("AddApp initialized with instance ID");

        vm.stopBroadcast();

        console.log("");
        console.log("================================================");
        console.log("Deployment Complete!");
        console.log("================================================");
        console.log("AddApp:", address(addApp));
        console.log("App Instance ID:", appInstanceId);
        console.log("");
        console.log("Next steps:");
        console.log("1. Call addApp.add(1, 50) to add value 50 at index 1");
        console.log("2. Call addApp.getSum() to see the total");
        console.log("3. Check coordination.jobManager().getPendingJobs() for created jobs");
        console.log("");
    }
}

/**
 * @title DeployAddAppLocal
 * @notice Deployment script specifically for local Anvil testing
 * @dev Reads coordination address from latest deployment JSON
 */
contract DeployAddAppLocal is Script {
    function run() external {
        // For local Anvil, read from environment or use default
        address coordinationAddress = vm.envOr(
            "SILVANA_COORDINATION_CONTRACT",
            address(0x5eb3Bc0a489C5A8288765d2336659EbCA68FCd00) // Default from recent deployment
        );

        // Use Anvil's first account
        uint256 deployerPrivateKey = 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80;
        address deployer = vm.addr(deployerPrivateKey);

        console.log("================================================");
        console.log("Deploying AddApp to Local Anvil");
        console.log("================================================");
        console.log("Deployer:", deployer);
        console.log("Coordination:", coordinationAddress);
        console.log("");

        vm.startBroadcast(deployerPrivateKey);

        // Deploy AddApp
        AddApp addApp = new AddApp(coordinationAddress);
        console.log("AddApp deployed at:", address(addApp));

        // Generate Ed25519 public key as app instance ID
        string[] memory inputs = new string[](1);
        inputs[0] = "script/generate_ed25519_id.sh";
        bytes memory result = vm.ffi(inputs);
        // Convert bytes to 0x-prefixed hex string
        string memory appInstanceId = vm.toString(result);
        console.log("Generated app instance ID:", appInstanceId);

        // Create app instance
        ICoordination coordination = ICoordination(coordinationAddress);
        IAppInstanceManager appManager = coordination.appInstanceManager();

        // Get app metadata from environment variables
        string memory appName = vm.envOr("APP_NAME", string("add-app-demo"));
        string memory agentName = vm.envOr("AGENT_NAME", string("add-agent"));
        string memory developerName = vm.envOr("DEVELOPER_NAME", string("silvana"));

        // Create app instance with the generated ID
        string memory createdId = appManager.createAppInstance(
            appInstanceId,
            appName,
            agentName,
            developerName
        );
        console.log("App instance created:", createdId);

        // Initialize
        addApp.initialize(appInstanceId);
        console.log("AddApp initialized");

        vm.stopBroadcast();

        console.log("");
        console.log("================================================");
        console.log("Local Deployment Complete!");
        console.log("================================================");
        console.log("AddApp address:", address(addApp));
        console.log("App Instance ID:", appInstanceId);
        console.log("");
        console.log("Try these commands:");
        console.log("  # Add value 50 at index 1");
        console.log("  cast send", vm.toString(address(addApp)), "\"add(uint32,uint256)\" 1 50 --rpc-url localhost --private-key $PRIVATE_KEY");
        console.log("");
        console.log("  # Check sum");
        console.log("  cast call", vm.toString(address(addApp)), "\"getSum()(uint256)\" --rpc-url localhost");
        console.log("");
    }
}
