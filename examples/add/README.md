# Silvana Add Example

This example demonstrates how to build a simple addition program using the Silvana framework. The program showcasing the integration between Sui Move contracts, TypeScript agents, and Mina zkApps.

## Prerequisites

Before starting, ensure you have the following tools installed:

### Required Software

1. **Node.js and npm**

   ```bash
   node --version  # Should be v22 or higher
   npm --version
   ```

2. **Sui CLI**

   Follow the installation instructions at [Sui Documentation](https://docs.sui.io/guides/developer/getting-started/sui-install)

   ```bash
   sui --version
   ```

3. **Docker Desktop**

   - Download from [Docker Desktop](https://www.docker.com/products/docker-desktop/)
   - Ensure Docker is running before proceeding

4. **Silvana CLI**

   ```bash
   # Quick install
   curl -sSL https://raw.githubusercontent.com/SilvanaOne/silvana/main/install.sh | bash

   # Verify installation
   silvana --version
   ```

## Getting Started

### Step 1: Create a New Project

Use the Silvana CLI to create a new project from the template:

```bash
silvana new myprogram
```

This command will:

- Download the project template
- Generate Sui and Mina keypairs for both user and coordinator
- Automatically fund all accounts from testnets
- Fetch devnet configuration
- Create a Silvana registry on-chain
- Store the agent's private key in secure storage
- Generate environment files with all necessary configuration

### Step 2: Navigate to Your Project

```bash
cd myprogram
```

Your project structure will look like:

```

 📁 Project structure:
    myprogram/
    ├── agent/     # TypeScript agent implementation
    |     src/     # Agent source code
    |     .env     # Agent configuration (auto-generated)
    |     .env.app # App-specific configuration (empty initially)
    ├── move/      # Move smart contracts
    └── silvana/   # Silvana coordinator
         .env      # Coordinator configuration (auto-generated)

```

### Step 3: Build and Deploy Move Contract

Navigate to the move directory and deploy your smart contract:

```bash
cd move

# Switch to devnet
sui client switch --env devnet

# Check your addresses
sui client addresses

# Request funds if needed
sui client faucet

# Check balance
sui client balance

# Build the Move contract
sui move build

# Publish the contract
sui client publish
```

After publishing, you'll see output containing the PackageID, like:

```
PackageID: 0xf238cb13361d361c6324e069b99a2ebccbd69c3ac703b25c2f0ee41e7924736b
```

**Important:** Copy this PackageID and add it to your `agent/.env` file:
`APP_PACKAGE_ID=0xf238cb13361d361c6324e069b99a2ebccbd69c3ac703b25c2f0ee41e7924736b`

### Step 4: Build and Compile the Agent

Navigate to the agent directory and set up the TypeScript agent:

```bash
cd agent

# Install dependencies
npm install

# Build the TypeScript code
npm run build

# Compile zkApp circuits (run twice to generate full prover keys)
npm run compile
npm run compile
```

**Note:** The `npm run compile` command needs to be run twice to create a complete set of zk prover keys for the Mina zkApp.

### Step 5: Verify Account Funding

The setup process automatically funded your accounts, but you can verify the balances:

**Check Mina balance:**

```bash
# Get the Mina address from agent/.env (MINA_PUBLIC_KEY)
silvana balance mina --network mina:devnet --address <YOUR_MINA_PUBLIC_KEY>
```

You should see positive balances. If not, you can manually request funds using command
`silvana faucet`

### Step 6: Deploy the Application

Deploy your application to both Mina and Sui blockchains:

```bash
# From the agent directory
npm run deploy
```

This command will:

- Deploy the Mina zkApp contract
- Create the application on the Sui blockchain
- Register the app in the Silvana registry
- Generate the app instance configuration

You should see output similar to:

```
App initialized successfully
✅ App created successfully!
📦 App ID: 0x40deec126c7f08ce85135381646759533e6c7e1c5193674e4e71091052d18822
📦 Registry: 0x5124fbb1d17eccccd42128233fbfbfc0657a5065aa878536f724aa1fe8d6f619
💾 Configuration saved to .env.app
App instance ID: 0xaacf350ac6ae669ebf9804b455b0bc75a71f28a34bdc48d87ca78f1f90ba0f3b
Mina contract address: B62qoRvnY827gKNPy6yTWM9iwQJ9JGXu7MPDU9GCzSUpccELfThGRCE
Mina admin address: B62qoiTzbVwoQorP4ys8YKtP5kYWk46BRzeoxwb6xSnx2kLzZJQrEnG
```

The deployment will automatically save the configuration to `.env.app` file with:

- App ID: The unique identifier for your application
- App instance ID: The specific instance of your deployed app
- Mina contract address: The deployed zkApp address on Mina
- Registry information: Connection to the Silvana registry

### Step 7: Start the Silvana Coordinator

Open a new terminal window (not in VSCode or Cursor - use a regular terminal) and navigate to the silvana folder:

```bash
cd silvana
silvana start
```

**Important:** Use a regular system terminal for this step, not an integrated terminal in your IDE, to ensure proper process management.

The coordinator will:

- Connect to the Sui blockchain
- Monitor for incoming jobs
- Coordinate execution between the agent and blockchain
- Handle settlement and verification

You should see the coordinator starting up and waiting for jobs. Keep this terminal running.
The coordinator performs the following initialization steps:

1. **Configuration Loading**: Fetches devnet configuration from the RPC server and injects environment variables
2. **Sui Connection**: Connects to the Sui blockchain RPC endpoint
3. **Balance Check**: Verifies sufficient SUI balance for operations
4. **Gas Coin Pool**: Initializes a pool of gas coins by splitting large coins for better transaction performance
5. **Service Startup**: Starts multiple background services:
   - **Job Searcher**: Monitors for new jobs on the blockchain
   - **Multicall Processor**: Batches multiple operations for efficiency
   - **Docker Buffer Processor**: Manages Docker container execution
   - **Event Monitor**: Watches blockchain events
   - **gRPC Server**: Provides API for agent communication
   - **Periodic Tasks**: Runs reconciliation, block creation, and proof analysis

Once running, the coordinator will:

- Detect new jobs created on the blockchain
- Launch Docker containers to execute agent code
- Process job results and update blockchain state
- Handle proof generation and settlement

You'll see logs like:

```
📝 JobCreated: seq=4, dev=AddDeveloper, agent=AddAgent/prove, app_method=add
🐳 Starting Docker container for buffered job 1: AddDeveloper/AddAgent/prove
✅ Job 1 started and reserved for Docker session
```

### Step 8: Send Test Transactions

With the coordinator running, open another terminal and navigate to the agent directory to send test transactions:

```bash
cd agent
npm run batch
```

This command runs a batch test that sends multiple transactions to your deployed application. You'll see output like:

```
> add@0.1.0 batch
> npm run test test/batch.test.ts

[2025-09-09T23:02:48.875Z] Batch iteration 1 - Max delay: 13.718s - Total TX: 0 - TPS: 0
wait sui tx: 108.765ms
wait sui tx: 113.928ms
wait sui tx: 3.267s
...
[2025-09-09T23:04:59.263Z] Batch iteration 2 - Max delay: 20.926s - Total TX: 10 - TPS: 0.077
```

The batch test:

- Sends multiple addition requests to the Sui blockchain
- Each transaction creates a job that the coordinator detects
- The coordinator launches Docker containers to process each job
- Results are computed, proven with zkSNARKs, and settled on-chain

## 🎉 Congratulations!

**Your first Silvana app is now running!**

You've successfully:

- ✅ Created a new Silvana project with automated setup
- ✅ Deployed a Move smart contract on Sui
- ✅ Built and compiled a TypeScript agent with zkSNARK circuits
- ✅ Deployed a Mina zkApp for proof verification
- ✅ Started the Silvana coordinator to orchestrate execution
- ✅ Sent transactions that trigger distributed computation

Your application is now processing addition operations across multiple blockchains:

- **Sui** handles job creation, proving orchestration and optimistic state calculations
- **Docker containers** execute the computation securely
- **Mina** verifies zero-knowledge proofs of correct execution
- **Silvana** coordinates the entire workflow

## Running Agent on Silvana Devnet

Instead of running your own coordinator, you can deploy your application to the Silvana devnet, where it will be processed by the shared Silvana infrastructure.

### Option 1: Deploy to Shared Silvana Devnet

To use the shared Silvana devnet registry:

1. **Get the devnet registry ID**:

   ```bash
   silvana config
   ```

   Look for:

   ```
   SILVANA_REGISTRY = 0x916a3b24de8165fb6fb25d060ec82b50683dc9e9cebf0dfae559f943ee32adb2
   SILVANA_REGISTRY_PACKAGE = 0x32f8ad21df94c28401912c8ffebcc3bd186f5bf7da0995057a63755005937025
   ```

2. **Update your agent configuration** to use the shared registry:

   ```bash
   # In agent/.env, update:
   SILVANA_REGISTRY=0x916a3b24de8165fb6fb25d060ec82b50683dc9e9cebf0dfae559f943ee32adb2
   SILVANA_REGISTRY_PACKAGE=0x32f8ad21df94c28401912c8ffebcc3bd186f5bf7da0995057a63755005937025
   ```

3. **Register your app** in the shared registry using silvana registry commands:
   ```bash
   silvana registry --help
   ```

Your jobs will now be processed by the Silvana devnet infrastructure - no need to run your own coordinator!

### Option 2: Join as a Devnet Operator

To contribute computing power to the Silvana devnet:

1. **Configure your coordinator** to use the shared registry:

   ```bash
   # In silvana/.env, set:
   SILVANA_REGISTRY=0x916a3b24de8165fb6fb25d060ec82b50683dc9e9cebf0dfae559f943ee32adb2
   SILVANA_REGISTRY_PACKAGE=0x32f8ad21df94c28401912c8ffebcc3bd186f5bf7da0995057a63755005937025
   ```

2. **Start your coordinator**:
   ```bash
   cd silvana
   silvana start
   ```

Your coordinator will now process jobs from all applications registered in the shared devnet registry.

## Amending the Code

When you want to modify the application logic and deploy your own version:

### Step 1: Modify the Circuit Logic

Edit `agent/src/circuit.ts` to implement your custom computation logic. This file contains the zkSNARK circuit that defines what computation will be proven.

### Step 2: Configure Docker Hub

Before building and pushing your Docker image, configure your Docker Hub credentials in `agent/.env`:

```bash
# Edit agent/.env and add your Docker Hub credentials:
DOCKER_USERNAME=your-dockerhub-username
DOCKER_PASSWORD=your-dockerhub-password
IMAGE_NAME=your-image-name (like 'add')
DOCKER_ACCESS_TOKEN=your-docker-access-token
```

To get a Docker Access Token:

1. Log in to [Docker Hub](https://hub.docker.com)
2. Go to Account Settings → Security
3. Create a New Access Token
4. Copy the token to `DOCKER_ACCESS_TOKEN`

### Step 3: Build and Deploy

After making your changes, rebuild and deploy your agent:

```bash
cd agent

# Build the TypeScript code
npm run build

# Compile the zkApp circuits (run twice for complete prover keys)
npm run compile
npm run compile

# Build and push Docker image with your agent and prover keys
npm run docker
```

This process:

- **Builds** your modified TypeScript code
- **Compiles** the zkSNARK circuits to generate prover/verifier keys
- **Creates** a Docker image containing your agent code and prover keys
- **Pushes** the image to Docker Hub under your account

### Step 4: Update Your App Configuration

After pushing your new Docker image, update your app to use it:

1. Note your new Docker image URL: `docker.io/your-username/your-image-name:latest`
2. Update your app configuration to reference this new image
3. Redeploy your app with `npm run deploy`

### Important Notes

- **Prover Keys**: The compiled prover keys are large (can be several GB) and are included in the Docker image
- **Image Size**: Docker images with prover keys can be large
- **Public Repositories**: You should use public Docker repository, to ensure that coordinator will be able to pull the image

## Troubleshooting

If you encounter issues:

- **Coordinator not detecting jobs**: Ensure the coordinator is running and connected to the correct SILVANA_REGISTRY_PACKAGE and SILVANA_REGISTRY on sui devnet
- **Docker errors**: Verify Docker Desktop is running and you're logged in to Docker Hub
- **Insufficient balance**: Run `silvana faucet` commands to request more tokens
- **Transaction failures**: Check the explorer for detailed error messages and look at the coordinator logs in the terminal
