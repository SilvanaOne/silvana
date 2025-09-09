use alloy::sol;

// Define the contract interfaces using sol! macro
sol! {
    #[sol(rpc)]
    interface IDelegationManager {
        function registerAsOperator(
            address initDelegationApprover,
            uint32 allocationDelay,
            string metadataURI
        ) external;
        
        function isOperator(address operator) external view returns (bool);
    }
    
    #[sol(rpc)]
    interface IAVSDirectory {
        function calculateOperatorAVSRegistrationDigestHash(
            address operator,
            address avs,
            bytes32 salt,
            uint256 expiry
        ) external view returns (bytes32);
    }
    
    #[sol(rpc)]
    interface IECDSAStakeRegistry {
        struct SignatureWithSaltAndExpiry {
            bytes signature;
            bytes32 salt;
            uint256 expiry;
        }
        
        function registerOperatorWithSignature(
            SignatureWithSaltAndExpiry memory operatorSignature,
            address operatorToRegister
        ) external;
    }
    
    #[sol(rpc)]
    interface ISilvanaServiceManager {
        struct Task {
            string name;
            uint32 taskCreatedBlock;
        }
        
        event NewTaskCreated(uint32 indexed taskIndex, Task task);
        
        function createNewTask(string memory taskName) external;
        
        function respondToTask(
            Task calldata task,
            uint32 taskIndex,
            bytes calldata signature
        ) external;
    }
}