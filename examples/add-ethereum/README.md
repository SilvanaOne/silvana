cargo run --release -p coordinator -- registry add-app add_ethereum_app --registry 0xdf768747cb016e015ac9d60dd6420e9d9e6640876d17b3e1e9d61ab5e057262a

cargo run --release -p coordinator -- registry add-app add_ethereum_app

cargo run --release -p coordinator -- registry add-developer AddEthereumDeveloper

cargo run --release -p coordinator -- registry add-agent AddEthereumDeveloper AddEthereumAgent

cargo run --release -p coordinator -- registry add-method AddEthereumDeveloper AddEthereumAgent prove docker.io/dfstio/add-ethereum-devnet:latest --min-memory-gb 10 --min-cpu-cores 8
