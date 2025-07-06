# AWS TypeScript Pulumi Template

````
ssh -i "RPC2.pem" ec2-user@18.194.39.156
sudo less /var/log/cloud-init-output.log
grpcurl rpc.silvana.dev:443 list
grpcurl rpc.silvana.dev:443 describe silvana.events.v1.SilvanaEventsService
grpcurl rpc.silvana.dev:443 describe silvana.events.v1.Event

```sh
git clone https://github.com/SilvanaOne/zk-tests
cd zk-tests/packages/avs/rpc
cargo build --release
sudo setcap 'cap_net_bind_service=+ep' target/release/rpc
target/release/rpc
````

🔒 NATS (TLS): nats://rpc.silvana.dev:4222
🔒 NATS-WS (TLS): wss://rpc.silvana.dev:8080/ws
📊 NATS monitoring: http://rpc.silvana.dev:8222

copy image to s3 and back:

```sh
tar -czvf tee-arm-v5.tar.gz out
aws s3 cp tee-arm-v5.tar.gz s3://silvana-tee-images/tee-arm-v5.tar.gz
aws s3 cp time-server-v1.tar.gz s3://silvana-tee-images/time-server-v1.tar.gz
aws s3 cp s3://silvana-tee-images/tee-arm-v5.tar.gz tee-arm-v5.tar.gz
tar -xzvf tee-arm-v5.tar.gz
aws s3 cp s3://silvana-tee-images/time-server-v1.tar.gz time-server-v1.tar.gz
tar -xzvf time-server-v1.tar.gz
```

````

A minimal Pulumi template for provisioning AWS infrastructure using TypeScript. This template creates an Amazon S3 bucket and exports its name.

## Prerequisites

- Pulumi CLI (>= v3): https://www.pulumi.com/docs/get-started/install/
- Node.js (>= 14): https://nodejs.org/
- AWS credentials configured (e.g., via `aws configure` or environment variables)

## Getting Started

1.  Initialize a new Pulumi project:

    ```bash
    pulumi new aws-typescript
    ```

    Follow the prompts to set your:

    - Project name
    - Project description
    - AWS region (defaults to `us-east-1`)

2.  Preview and deploy your infrastructure:

    ```bash
    pulumi preview
    pulumi up
    ```

3.  When you're finished, tear down your stack:

    ```bash
    pulumi destroy
    pulumi stack rm
    ```

## Project Layout

- `Pulumi.yaml` — Pulumi project and template metadata
- `index.ts` — Main Pulumi program (creates an S3 bucket)
- `package.json` — Node.js dependencies
- `tsconfig.json` — TypeScript compiler options

## Configuration

| Key          | Description                             | Default     |
| ------------ | --------------------------------------- | ----------- |
| `aws:region` | The AWS region to deploy resources into | `us-east-1` |

Use `pulumi config set <key> <value>` to customize configuration.

## Next Steps

- Extend `index.ts` to provision additional resources (e.g., VPCs, Lambda functions, DynamoDB tables).
- Explore [Pulumi AWSX](https://www.pulumi.com/docs/reference/pkg/awsx/) for higher-level AWS components.
- Consult the [Pulumi documentation](https://www.pulumi.com/docs/) for more examples and best practices.

## Getting Help

If you encounter any issues or have suggestions, please open an issue in this repository.
````
