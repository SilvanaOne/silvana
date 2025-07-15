# Silvana Coordination Contracts

## Architecture & Code Flow Presentation

---

## 1: System Overview

The Silvana coordination system is a decentralized blockchain platform for managing AI agents, applications, and their execution with integrated proof calculation and settlement mechanisms, built on the Sui blockchain using Move language.

**Key Components:**

- **Registry**: Central orchestrator managing developers, agents, and Silvana applications
- **Developers**: Agent and application creators who publish AI solutions
- **Agents**: AI applications with execution methods and configurations
- **Silvana Apps**: Main application containers with methods and instances
- **App Instances**: Individual running instances with state management and block production
- **Blocks**: Blockchain blocks with commitments and settlement tracking
- **Provers**: Proof calculation system with distributed prover network
- **Settlement**: Cross-chain settlement integration (planned)

```mermaid
graph TB
    subgraph "Core Registry Layer"
        R[SilvanaRegistry]
        A[Admin]
    end

    subgraph "Developer & Agent Layer"
        D1[Developer 1]
        D2[Developer 2]
        AG1[Agent A]
        AG2[Agent B]
    end

    subgraph "Application Layer"
        SA1[SilvanaApp 1]
        SA2[SilvanaApp 2]
        AI1[AppInstance 1]
        AI2[AppInstance 2]
        AI3[AppInstance 3]
    end

    subgraph "Blockchain Layer"
        B1[Block 1]
        B2[Block 2]
        B3[Block 3]
        PC1[ProofCalculation 1]
        PC2[ProofCalculation 2]
    end

    subgraph "Settlement Layer"
        S[Settlement]
        ST[Settlement Tx]
    end

    R --> D1
    R --> D2
    R --> SA1
    R --> SA2
    D1 --> AG1
    D2 --> AG2

    SA1 --> AI1
    SA1 --> AI2
    SA2 --> AI3

    AI1 --> B1
    AI2 --> B2
    AI3 --> B3

    B1 --> PC1
    B2 --> PC2

    B1 --> ST
    B2 --> ST
    ST --> S

    style R fill:#ff6b6b
    style SA1 fill:#4ecdc4
    style AI1 fill:#45b7d1
    style B1 fill:#96ceb4
    style PC1 fill:#ffeaa7
```

---

## 2: Enhanced Registry Structure

### SilvanaRegistry Structure

```rust
pub struct SilvanaRegistry {
    id: UID,
    name: String,
    version: u32,
    admin: address,
    developers: ObjectTable<String, Developer>,
    developers_index: ObjectTable<address, DeveloperNames>,
    apps: ObjectTable<String, SilvanaApp>,           // New: App management
    apps_index: ObjectTable<address, AppNames>,      // New: App indexing
}
```

### Developer Structure (Enhanced)

```rust
pub struct Developer {
    id: UID,
    name: String,
    github: String,
    image: Option<String>,
    description: Option<String>,
    site: Option<String>,
    agents: ObjectTable<String, Agent>,
    owner: address,
    created_at: u64,
    updated_at: u64,
    version: u64,
}
```

### Admin Structure

```rust
pub struct Admin {
    id: UID,
    address: address,
}
```

---

## 3: Application Architecture

### SilvanaApp Structure

```rust
pub struct SilvanaApp {
    id: UID,
    name: String,
    description: Option<String>,
    methods: VecMap<String, AppMethod>,
    owner: address,
    created_at: u64,
    updated_at: u64,
    version: u64,
    instances: VecSet<address>,                      // Track app instances
}
```

### AppMethod Structure

```rust
pub struct AppMethod {
    description: Option<String>,
    developer: String,
    agent: String,
    agent_method: String,
}
```

### AppInstance Structure

```rust
pub struct AppInstance {
    id: UID,
    silvana_app_name: String,
    description: Option<String>,
    metadata: Option<String>,
    methods: VecMap<String, AppMethod>,
    state: AppState,                                 // Commitment state
    blocks: ObjectTable<u64, Block>,                 // Block storage
    proof_calculations: ObjectTable<u64, ProofCalculation>,
    sequence: u64,
    admin: address,
    block_number: u64,
    previous_block_timestamp: u64,
    previous_block_last_sequence: u64,
    previous_block_actions_state: Element<Scalar>,
    last_proved_block_number: u64,
    last_proved_sequence: u64,
    isPaused: bool,
    created_at: u64,
    updated_at: u64,
}
```

---

## 4: Blockchain & Proof System

### Block Structure

```rust
pub struct Block {
    id: UID,
    name: String,
    block_number: u64,
    start_sequence: u64,
    end_sequence: u64,
    actions_commitment: Element<Scalar>,
    state_commitment: Element<Scalar>,
    time_since_last_block: u64,
    number_of_transactions: u64,
    start_actions_commitment: Element<Scalar>,
    end_actions_commitment: Element<Scalar>,
    state_data_availability: Option<String>,
    proof_data_availability: Option<String>,
    settlement_tx_hash: Option<String>,
    settlement_tx_included_in_block: bool,
    created_at: u64,
    state_calculated_at: Option<u64>,
    proved_at: Option<u64>,
    sent_to_settlement_at: Option<u64>,
    settled_at: Option<u64>,
}
```

### ProofCalculation Structure

```rust
pub struct ProofCalculation {
    id: UID,
    block_number: u64,
    start_sequence: u64,
    end_sequence: Option<u64>,
    proofs: VecMap<vector<u64>, Proof>,
    block_proof: Option<String>,
    is_finished: bool,
}
```

### Proof Structure

```rust
pub struct Proof {
    status: u8,                    // STARTED, CALCULATED, REJECTED, RESERVED, USED
    da_hash: Option<String>,
    sequence1: Option<vector<u64>>,
    sequence2: Option<vector<u64>>,
    rejected_count: u16,
    timestamp: u64,
    prover: address,
    user: Option<address>,
    job_id: String,
}
```

---

## 5: Complete System Architecture

```mermaid
graph TB
    subgraph "Registry Layer"
        SR[SilvanaRegistry]
        ADM[Admin]
        DI[Developer Index]
        AI[App Index]
    end

    subgraph "Development Layer"
        D1[Developer 1]
        D2[Developer 2]
        AG1[Agent A]
        AG2[Agent B]
        AM1[Agent Methods]
        AM2[Agent Methods]
    end

    subgraph "Application Layer"
        SA1[SilvanaApp 1]
        SA2[SilvanaApp 2]
        APM1[App Methods]
        APM2[App Methods]
    end

    subgraph "Instance Layer"
        AI1[AppInstance 1]
        AI2[AppInstance 2]
        AI3[AppInstance 3]
        AS1[App State]
        AS2[App State]
        AS3[App State]
    end

    subgraph "Blockchain Layer"
        B1[Block 1]
        B2[Block 2]
        B3[Block 3]
        BC1[Block Commitment]
        BC2[Block Commitment]
    end

    subgraph "Proof Layer"
        PC1[ProofCalculation 1]
        PC2[ProofCalculation 2]
        P1[Proof 1]
        P2[Proof 2]
        P3[Proof 3]
        PV[Prover Network]
    end

    subgraph "Settlement Layer"
        SET[Settlement]
        STX[Settlement Tx]
        DA[Data Availability]
    end

    SR --> D1
    SR --> D2
    SR --> SA1
    SR --> SA2
    SR --> DI
    SR --> AI
    ADM --> SR

    D1 --> AG1
    D2 --> AG2
    AG1 --> AM1
    AG2 --> AM2

    SA1 --> APM1
    SA2 --> APM2
    SA1 --> AI1
    SA1 --> AI2
    SA2 --> AI3

    AI1 --> AS1
    AI2 --> AS2
    AI3 --> AS3
    AI1 --> B1
    AI2 --> B2
    AI3 --> B3

    B1 --> BC1
    B2 --> BC2
    B1 --> PC1
    B2 --> PC2

    PC1 --> P1
    PC1 --> P2
    PC2 --> P3
    PV --> P1
    PV --> P2
    PV --> P3

    B1 --> STX
    B2 --> STX
    STX --> SET
    B1 --> DA
    B2 --> DA

    style SR fill:#ff6b6b
    style SA1 fill:#4ecdc4
    style AI1 fill:#45b7d1
    style B1 fill:#96ceb4
    style PC1 fill:#ffeaa7
    style SET fill:#dda0dd
```

---

## 6: Application Instance Creation Flow

```mermaid
sequenceDiagram
    participant U as User
    participant SA as SilvanaApp
    participant AI as AppInstance
    participant AS as AppState
    participant B as Block
    participant PC as ProofCalculation

    U->>SA: Create Instance Request
    SA->>AI: create_app_instance()
    AI->>AS: create_app_state()
    AS->>AS: Initialize commitment state
    AI->>B: Create genesis block
    B->>B: Set initial commitments
    AI->>PC: create_block_proof_calculation()
    PC->>PC: Initialize proof system
    AI->>AI: Set initial sequence & timestamps
    AI-->>U: AppInstance created

    Note over U,PC: Instance ready for transaction processing
```

**Key Steps:**

1. User requests instance creation from SilvanaApp
2. AppInstance is created with initial state and metadata
3. AppState is initialized with commitment tracking
4. Genesis block is created with initial commitments
5. ProofCalculation is set up for the first block
6. Instance is configured with sequence numbers and timestamps
7. Events are emitted for external systems

---

## 7: Block Production & Proof Flow

```mermaid
sequenceDiagram
    participant AI as AppInstance
    participant B as Block
    participant PC as ProofCalculation
    participant P as Prover
    participant S as Settlement

    AI->>B: Create new block
    B->>B: Set commitments & sequences
    B->>PC: Initialize proof calculation
    PC->>P: Request proof generation
    P->>P: Calculate proofs
    P->>PC: Submit proof
    PC->>PC: Validate & store proof
    PC->>B: Mark as proved
    B->>S: Send to settlement
    S->>S: Process settlement
    S->>B: Confirm settlement
    B->>B: Mark as settled

    Note over AI,S: Block lifecycle complete
```

**Block Lifecycle:**

1. **Creation**: Block created with commitments and sequence ranges
2. **Proof Request**: ProofCalculation initialized for the block
3. **Proof Generation**: Distributed prover network generates proofs
4. **Proof Submission**: Provers submit proofs with verification data
5. **Validation**: Proof validation and status tracking
6. **Settlement**: Block data sent to settlement layer
7. **Confirmation**: Settlement confirmation and finalization

---

## 8: Proof Calculation System

```mermaid
graph TB
    subgraph "Proof Status Flow"
        PS1[STARTED] --> PS2[CALCULATED]
        PS2 --> PS3[RESERVED]
        PS3 --> PS4[USED]
        PS2 --> PS5[REJECTED]
        PS5 --> PS1
    end

    subgraph "Prover Network"
        PR1[Prover 1]
        PR2[Prover 2]
        PR3[Prover N]
    end

    subgraph "Proof Data"
        PD1[DA Hash]
        PD2[Sequences]
        PD3[Job ID]
        PD4[Timestamp]
        PD5[CPU Time]
        PD6[Memory Usage]
    end

    PR1 --> PS1
    PR2 --> PS1
    PR3 --> PS1

    PS2 --> PD1
    PS2 --> PD2
    PS2 --> PD3
    PS2 --> PD4
    PS2 --> PD5
    PS2 --> PD6

    style PS1 fill:#fff3e0
    style PS2 fill:#e8f5e8
    style PS4 fill:#e1f5fe
    style PS5 fill:#ffebee
```

**Proof Status Types:**

- **STARTED**: Proof calculation initiated by prover
- **CALCULATED**: Proof completed and submitted
- **REJECTED**: Proof rejected by validators
- **RESERVED**: Proof reserved for specific user
- **USED**: Proof utilized in block finalization

---

## 9: Enhanced Event System

### Registry Events

- `RegistryCreatedEvent` - New registry initialization
- `AdminCreateEvent` - Admin account creation

### Application Events

- `AppCreatedEvent` - New Silvana app creation
- `AppUpdatedEvent` - App modifications
- `AppDeletedEvent` - App removal
- `AppInstanceCreatedEvent` - Instance creation
- `AppMethodAddedEvent` - Method addition to app

### Blockchain Events

- `BlockEvent` - Block creation and commitment
- `DataAvailabilityEvent` - DA hash updates
- `SettlementTransactionEvent` - Settlement status

### Proof Events

- `ProofStartedEvent` - Proof calculation initiated
- `ProofSubmittedEvent` - Proof calculation completed
- `ProofUsedEvent` - Proof utilized
- `ProofReservedEvent` - Proof reserved
- `ProofRejectedEvent` - Proof rejected
- `BlockProofEvent` - Block proof finalization

```mermaid
graph TB
    subgraph "Event Categories"
        E1[Registry Events]
        E2[Application Events]
        E3[Blockchain Events]
        E4[Proof Events]
    end

    subgraph "Event Consumers"
        UI[Frontend UI]
        API[API Gateway]
        IDX[Indexing Service]
        MON[Monitoring]
        SET[Settlement Service]
    end

    E1 --> UI
    E1 --> IDX
    E2 --> UI
    E2 --> API
    E3 --> IDX
    E3 --> MON
    E3 --> SET
    E4 --> MON
    E4 --> API

    style E1 fill:#ff6b6b
    style E2 fill:#4ecdc4
    style E3 fill:#45b7d1
    style E4 fill:#96ceb4
```

---

## 10: Settlement Integration

### Settlement Components

- **Block Settlement**: Cross-chain settlement transaction management
- **Data Availability**: State and proof data availability tracking
- **Transaction Hashing**: Settlement transaction hash tracking
- **Settlement Confirmation**: Block settlement status confirmation

```mermaid
graph TB
    subgraph "Settlement Flow"
        B[Block]
        DA[Data Availability]
        ST[Settlement Tx]
        SC[Settlement Confirmation]
    end

    subgraph "Settlement Data"
        SD1[State DA Hash]
        SD2[Proof DA Hash]
        SD3[Settlement Tx Hash]
        SD4[Settlement Status]
    end

    subgraph "External Systems"
        ES1[External Chain]
        ES2[DA Layer]
        ES3[Settlement Network]
    end

    B --> DA
    DA --> SD1
    DA --> SD2
    DA --> ES2

    B --> ST
    ST --> SD3
    ST --> ES1
    ST --> ES3

    ES1 --> SC
    ES3 --> SC
    SC --> SD4
    SC --> B

    style B fill:#45b7d1
    style DA fill:#96ceb4
    style ST fill:#ffeaa7
    style SC fill:#dda0dd
```

---

## 11: Key Functions & API Surface

### Registry Functions

- `create_registry()` - Initialize coordination system
- `add_developer()` - Register developer in system
- `add_agent()` - Create agent under developer
- `add_app()` - Create new Silvana application
- `get_app()` - Retrieve application information

### Application Functions

- `create_app()` - Initialize Silvana application
- `update_app()` - Modify application metadata
- `add_method()` - Add execution method to app
- `create_app_instance()` - Create app instance
- `delete_app()` - Remove application

### App Instance Functions

- `create_app_instance()` - Initialize app instance with state
- `add_method()` - Add method to instance
- `pause_instance()` - Pause instance execution
- `resume_instance()` - Resume instance execution
- `process_block()` - Process new block

### Block Functions

- `create_block()` - Create new block with commitments
- `set_state_calculated()` - Mark state as calculated
- `set_proved()` - Mark block as proved
- `set_settlement_data()` - Set settlement transaction data
- `set_settled()` - Mark block as settled

### Proof Functions

- `create_block_proof_calculation()` - Initialize proof calculation
- `start_proof()` - Start proof calculation
- `submit_proof()` - Submit calculated proof
- `reserve_proof()` - Reserve proof for user
- `use_proof()` - Utilize proof in block
- `reject_proof()` - Reject invalid proof

### Settlement Functions

- `send_to_settlement()` - Send block to settlement layer
- `confirm_settlement()` - Confirm settlement transaction
- `update_settlement_status()` - Update settlement status

---

## 12: Data Flow & State Management

```mermaid
graph TB
    subgraph "State Management"
        SM1[App State]
        SM2[Block State]
        SM3[Proof State]
        SM4[Settlement State]
    end

    subgraph "State Transitions"
        ST1[Instance Creation]
        ST2[Block Production]
        ST3[Proof Calculation]
        ST4[Settlement Processing]
    end

    subgraph "State Persistence"
        SP1[Sui Blockchain]
        SP2[Object Storage]
        SP3[Event Log]
    end

    ST1 --> SM1
    ST2 --> SM2
    ST3 --> SM3
    ST4 --> SM4

    SM1 --> SP1
    SM2 --> SP2
    SM3 --> SP2
    SM4 --> SP3

    style SM1 fill:#e8f5e8
    style ST1 fill:#fff3e0
    style SP1 fill:#f3e5f5
```

**State Management Features:**

- **Commitment Tracking**: BLS12-381 scalar commitments for state and actions
- **Version Control**: Incremental version tracking across all components
- **Sequence Management**: Sequential transaction and block numbering
- **Timestamp Tracking**: Creation, update, and lifecycle timestamps
- **Status Monitoring**: Comprehensive status tracking across all components

---
