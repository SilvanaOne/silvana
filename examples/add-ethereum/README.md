cargo run --release -p coordinator -- registry add-app add_ethereum_app
cargo run --release -p coordinator -- registry add-developer AddEthereumDeveloper
cargo run --release -p coordinator -- registry add-agent AddEthereumDeveloper AddEthereumAgent
cargo run --release -p coordinator -- registry add-method AddEthereumDeveloper AddEthereumAgent prove docker.io/dfstio/add-ethereum-devnet:latest --min-memory-gb 1 --min-cpu-cores 8
