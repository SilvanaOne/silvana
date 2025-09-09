silvana new myprogram
cd myprogram
cd move
sui client switch --env devnet
sui client addresses
sui client faucet
sui client balance
sui move build
sui client publish
get
PackageID: 0xf238cb13361d361c6324e069b99a2ebccbd69c3ac703b25c2f0ee41e7924736b
write to APP_PACKAGE_ID in agent

cd ../agent
npm install
npm run build
npm run compile
npm run compile

wait for topup on Mina for MINA_PUBLIC_KEY from agent/.env
silvana balance mina --network 'mina:devnet' --address B62qk11BSgji2ygYeYg7xFU1NdTz5MAxqviyMhyfqoFwhu5ksVghPBS`
📊 Checking balance for B62qk11BSgji2ygYeYg7xFU1NdTz5MAxqviyMhyfqoFwhu5ksVghPBS on mina:devnet

💰 Balance: 299 MINA
Nonce: 0
Token Symbol:

🔍 View on explorer:
https://minascan.io/devnet/account/B62qk11BSgji2ygYeYg7xFU1NdTz5MAxqviyMhyfqoFwhu5ksVghPBS

npm run deploy

generate keys
topup keys
npm install
npm run build
env vars
sui client switch --env devnet
sui client addresses
sui client faucet
sui client balance
sui move build
sui client publish
record PackageID: 0xe1d202a644283aa739f35f3992f6684a32f81cf624771d53526cae494170071e
APP_PACKAGE_ID=0x0d296aaacbf84266307f087759760ac55194b59540839a82b701eedc896d34de
.env agent
npm run compile 3 times
env docker
start Docker
npm run docker
save admin private key to secrets
amend docker name dockerImage: "docker.io/dfstio/add:latest", in create.ts
check that Mina balance is positive by checking the links provided by silvana

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

```
2025-09-09T22:25:20.786997Z  INFO silvana: 🚀 Creating new Silvana project: myprogram
2025-09-09T22:25:20.787266Z  INFO silvana: 📥 Downloading project template...
2025-09-09T22:25:21.310632Z  INFO silvana: ✅ Template downloaded successfully!
2025-09-09T22:25:21.310708Z  INFO silvana::example: 🔑 Setting up project credentials...
2025-09-09T22:25:21.310736Z  INFO silvana::example: 📍 Generating Sui keypairs...
2025-09-09T22:25:21.311021Z  INFO silvana::example:    • Generating Sui user keypair...
2025-09-09T22:25:21.312435Z  INFO silvana::example:      ✓ Sui user address: 0x3f176926a223d730fea3998da1791f4c7517e73bf3472e233a316d8672275683
2025-09-09T22:25:21.312490Z  INFO silvana::example:    • Generating Sui coordinator keypair...
2025-09-09T22:25:21.312523Z  INFO silvana::example:      ✓ Sui coordinator address: 0x82d751f1182343a402e83868b6573856fbe64c87370471afab21f429490c9559
2025-09-09T22:25:21.312530Z  INFO silvana::example: 📍 Generating Mina keypairs...
2025-09-09T22:25:21.312533Z  INFO silvana::example:    • Generating Mina app-admin keypair...
2025-09-09T22:25:21.312803Z  INFO silvana::example:      ✓ Mina app-admin address: B62qoiTzbVwoQorP4ys8YKtP5kYWk46BRzeoxwb6xSnx2kLzZJQrEnG
2025-09-09T22:25:21.312807Z  INFO silvana::example:    • Generating Mina user keypair...
2025-09-09T22:25:21.312995Z  INFO silvana::example:      ✓ Mina user address: B62qk11BSgji2ygYeYg7xFU1NdTz5MAxqviyMhyfqoFwhu5ksVghPBS
2025-09-09T22:25:21.312999Z  INFO silvana::example: 💰 Funding Sui accounts from devnet faucet...
2025-09-09T22:25:21.313002Z  INFO silvana::example: Funding Sui user account: 0x3f176926a223d730fea3998da1791f4c7517e73bf3472e233a316d8672275683
2025-09-09T22:25:21.313333Z  INFO sui::faucet: Requesting tokens from devnet faucet at https://faucet.devnet.sui.io/v2/gas for address 0x3f176926a223d730fea3998da1791f4c7517e73bf3472e233a316d8672275683
2025-09-09T22:25:22.223757Z  INFO sui::faucet: ✅ Devnet faucet request successful for 0x3f176926a223d730fea3998da1791f4c7517e73bf3472e233a316d8672275683. Transaction: GGRwF1ybif9nRsjiJEneBguQppzKLkWVzxSZmToj1mLH
2025-09-09T22:25:22.224167Z  INFO silvana::example: ✓ Faucet request sent
2025-09-09T22:25:22.224184Z  INFO silvana::example: 📄 Transaction: GGRwF1ybif9nRsjiJEneBguQppzKLkWVzxSZmToj1mLH
2025-09-09T22:25:22.224188Z  INFO silvana::example: 🔗 Explorer: https://suiscan.xyz/devnet/tx/GGRwF1ybif9nRsjiJEneBguQppzKLkWVzxSZmToj1mLH
2025-09-09T22:25:24.225869Z  INFO silvana::example: Funding Sui coordinator account: 0x82d751f1182343a402e83868b6573856fbe64c87370471afab21f429490c9559
2025-09-09T22:25:24.225947Z  INFO sui::faucet: Requesting tokens from devnet faucet at https://faucet.devnet.sui.io/v2/gas for address 0x82d751f1182343a402e83868b6573856fbe64c87370471afab21f429490c9559
2025-09-09T22:25:24.831360Z  INFO sui::faucet: ✅ Devnet faucet request successful for 0x82d751f1182343a402e83868b6573856fbe64c87370471afab21f429490c9559. Transaction: 81owAbnYmFEfm89L9DwR141yptQU7iR1BP2EnuapF9MK
2025-09-09T22:25:24.831400Z  INFO silvana::example: ✓ Faucet request sent
2025-09-09T22:25:24.831405Z  INFO silvana::example: 📄 Transaction: 81owAbnYmFEfm89L9DwR141yptQU7iR1BP2EnuapF9MK
2025-09-09T22:25:24.831408Z  INFO silvana::example: 🔗 Explorer: https://suiscan.xyz/devnet/tx/81owAbnYmFEfm89L9DwR141yptQU7iR1BP2EnuapF9MK
2025-09-09T22:25:24.831413Z  INFO silvana::example: 💰 Funding Mina accounts from devnet faucet...
2025-09-09T22:25:24.831417Z  INFO silvana::example: Funding Mina app-admin account: B62qoiTz...rEnG
2025-09-09T22:25:24.831423Z  INFO mina::faucet: 🚰 Requesting MINA from Devnet faucet for address B62qoiTzbVwoQorP4ys8YKtP5kYWk46BRzeoxwb6xSnx2kLzZJQrEnG
2025-09-09T22:25:26.466052Z  INFO silvana::example: ✓ Faucet request sent successfully
2025-09-09T22:25:26.466089Z  INFO silvana::example: 📄 Response: {"status":"success","message":{"paymentID":"5JtvA2fZrLqMyaisJwPW4QX3oYEJFebvKi6UJzTD6142iXSJ7qtd"}}
2025-09-09T22:25:26.466098Z  INFO silvana::example: 🔗 Explorer: https://minascan.io/devnet/account/B62qoiTzbVwoQorP4ys8YKtP5kYWk46BRzeoxwb6xSnx2kLzZJQrEnG
2025-09-09T22:25:28.467615Z  INFO silvana::example: Funding Mina user account: B62qk11B...hPBS
2025-09-09T22:25:28.467650Z  INFO mina::faucet: 🚰 Requesting MINA from Devnet faucet for address B62qk11BSgji2ygYeYg7xFU1NdTz5MAxqviyMhyfqoFwhu5ksVghPBS
2025-09-09T22:25:29.080061Z  INFO silvana::example: ✓ Faucet request sent successfully
2025-09-09T22:25:29.080120Z  INFO silvana::example: 📄 Response: {"status":"success","message":{"paymentID":"5Ju9sfyp8afmA2kHXNCkfXBX3PQgtE7rBpNsXLAiBaieLaS2Baqt"}}
2025-09-09T22:25:29.080129Z  INFO silvana::example: 🔗 Explorer: https://minascan.io/devnet/account/B62qk11BSgji2ygYeYg7xFU1NdTz5MAxqviyMhyfqoFwhu5ksVghPBS
2025-09-09T22:25:29.080142Z  INFO silvana::example: 📥 Fetching devnet configuration...
2025-09-09T22:25:29.080570Z  INFO silvana::config: Fetching configuration for chain devnet from https://rpc.silvana.dev
2025-09-09T22:25:29.080963Z  INFO rpc_client: Connecting to Silvana RPC service at: https://rpc.silvana.dev
2025-09-09T22:25:29.404632Z  INFO rpc_client: Successfully connected to Silvana RPC service
2025-09-09T22:25:29.501012Z  INFO silvana::config: Successfully fetched 16 configuration items for chain devnet
2025-09-09T22:25:29.501067Z  INFO silvana::example: ✓ Fetched configuration successfully
2025-09-09T22:25:29.501080Z  INFO silvana::example: 📝 Creating Silvana registry...
2025-09-09T22:25:29.501178Z  INFO sui::state: Initializing SharedSuiState with RPC URL: https://rpc-devnet-grpc.suiscan.xyz:443
2025-09-09T22:25:29.502565Z  INFO sui::state: Initialized SharedSuiState with address: 0x3f176926a223d730fea3998da1791f4c7517e73bf3472e233a316d8672275683
2025-09-09T22:25:29.502613Z  INFO sui::registry: Creating registry 'myprogram'
2025-09-09T22:25:31.021327Z  INFO sui::registry: Found registry ID from RegistryCreatedEvent: 0x5124fbb1d17eccccd42128233fbfbfc0657a5065aa878536f724aa1fe8d6f619
2025-09-09T22:25:31.143771Z  INFO sui::registry: Registry created with ID: 0x5124fbb1d17eccccd42128233fbfbfc0657a5065aa878536f724aa1fe8d6f619 (tx: DpYKyzZFW9H2NaS56eH7byTAVgh11KWcvoaf7zaDYFJ)
2025-09-09T22:25:31.143840Z  INFO sui::interface: Successfully created registry 'myprogram' with ID: 0x5124fbb1d17eccccd42128233fbfbfc0657a5065aa878536f724aa1fe8d6f619 (tx: DpYKyzZFW9H2NaS56eH7byTAVgh11KWcvoaf7zaDYFJ)
2025-09-09T22:25:31.143856Z  INFO silvana::example:    ✓ Registry created successfully
2025-09-09T22:25:31.143865Z  INFO silvana::example:    📄 Registry ID: 0x5124fbb1d17eccccd42128233fbfbfc0657a5065aa878536f724aa1fe8d6f619
2025-09-09T22:25:31.143873Z  INFO silvana::example:    📄 Transaction: DpYKyzZFW9H2NaS56eH7byTAVgh11KWcvoaf7zaDYFJ
2025-09-09T22:25:31.143881Z  INFO silvana::example:    🔗 Explorer: https://suiscan.xyz/devnet/tx/DpYKyzZFW9H2NaS56eH7byTAVgh11KWcvoaf7zaDYFJ
2025-09-09T22:25:31.143983Z  INFO silvana::example: 🔐 Storing agent private key in secure storage...
2025-09-09T22:25:31.144017Z  INFO rpc_client: Connecting to Silvana RPC service at: https://rpc.silvana.dev
2025-09-09T22:25:31.286128Z  INFO rpc_client: Successfully connected to Silvana RPC service
2025-09-09T22:25:31.406582Z  INFO secrets_client: Successfully stored secret for AddDeveloper:AddAgent
2025-09-09T22:25:31.406649Z  INFO silvana::example:    ✓ Agent private key stored securely
2025-09-09T22:25:31.406673Z  INFO silvana::example:    📄 Secret name: sk_B62qoiTzbVwoQorP4ys8YKtP5kYWk46BRzeoxwb6xSnx2kLzZJQrEnG
2025-09-09T22:25:31.406731Z  INFO silvana::example:    📄 Developer: AddDeveloper
2025-09-09T22:25:31.406760Z  INFO silvana::example:    📄 Agent: AddAgent
2025-09-09T22:25:31.409045Z  INFO silvana::example: ✅ Project setup complete!
2025-09-09T22:25:31.409074Z  INFO silvana::example: 📋 Generated Credentials:
2025-09-09T22:25:31.409089Z  INFO silvana::example:    ├── Sui:
2025-09-09T22:25:31.409104Z  INFO silvana::example:    │   ├── User: 0x3f176926a223d730fea3998da1791f4c7517e73bf3472e233a316d8672275683
2025-09-09T22:25:31.409168Z  INFO silvana::example:    │   └── Coordinator: 0x82d751f1182343a402e83868b6573856fbe64c87370471afab21f429490c9559
2025-09-09T22:25:31.409201Z  INFO silvana::example:    ├── Mina:
2025-09-09T22:25:31.409217Z  INFO silvana::example:    │   ├── App Admin: B62qoiTzbVwoQorP4ys8YKtP5kYWk46BRzeoxwb6xSnx2kLzZJQrEnG
2025-09-09T22:25:31.409241Z  INFO silvana::example:    │   └── User: B62qk11BSgji2ygYeYg7xFU1NdTz5MAxqviyMhyfqoFwhu5ksVghPBS
2025-09-09T22:25:31.409256Z  INFO silvana::example:    └── Registry: 0x5124fb...f619
2025-09-09T22:25:31.409270Z  INFO silvana::example: ⚙️  Environment files created:
2025-09-09T22:25:31.409286Z  INFO silvana::example:    • agent/.env (from env.example template)
2025-09-09T22:25:31.409302Z  INFO silvana::example:    • agent/.env.app (empty, for app-specific configuration)
2025-09-09T22:25:31.409319Z  INFO silvana::example:    • silvana/.env (coordinator configuration)
2025-09-09T22:25:31.409341Z  INFO silvana:
2025-09-09T22:25:31.409351Z  INFO silvana: 🎉 Project 'myprogram' is ready!
2025-09-09T22:25:31.409364Z  INFO silvana:
2025-09-09T22:25:31.409375Z  INFO silvana: 📁 Project structure:
2025-09-09T22:25:31.409426Z  INFO silvana:    myprogram/
2025-09-09T22:25:31.409439Z  INFO silvana:    ├── agent/     # TypeScript agent implementation
2025-09-09T22:25:31.409452Z  INFO silvana:    ├── move/      # Move smart contracts
2025-09-09T22:25:31.409473Z  INFO silvana:    └── silvana/   # Silvana coordinator configuration
2025-09-09T22:25:31.409490Z  INFO silvana:
2025-09-09T22:25:31.409501Z  INFO silvana: 🚀 Next steps:
2025-09-09T22:25:31.409514Z  INFO silvana:    cd myprogram
2025-09-09T22:25:31.409524Z  INFO silvana:    cd agent && npm install    # Install agent dependencies
2025-09-09T22:25:31.409540Z  INFO silvana:    cd move && sui move build  # Build Move contracts
2025-09-09T22:25:31.409556Z  INFO silvana:
2025-09-09T22:25:31.409566Z  INFO silvana: 📖 Check the README file for more information.
```
