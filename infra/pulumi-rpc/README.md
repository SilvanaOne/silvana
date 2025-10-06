# AWS TypeScript Pulumi Template

## Prerequisites

- Pulumi CLI (>= v3): https://www.pulumi.com/docs/get-started/install/
- Node.js (>= 22): https://nodejs.org/
- AWS credentials configured (e.g., via `aws configure` or environment variables)

## Deploying

### Preview and deploy your infrastructure:

```bash
pulumi login --local
pulumi preview
pulumi up
```

### Connect and interact with Silvana RPC server

```sh
ssh -i "RPC.pem" ec2-user@18.194.39.156
sudo less /var/log/cloud-init-output.log
grpcurl rpc-devnet.silvana.dev:443 list
grpcurl rpc-devnet.silvana.dev:443 describe silvana.events.v1.SilvanaEventsService
grpcurl rpc-devnet.silvana.dev:443 describe silvana.events.v1.Event
```

Logs are at rpc/logs:

```sh
ls rpc/logs
less rpc/logs/silvana.2025-07-06
```

### When you're finished, tear down your stack:

    ```bash
    pulumi destroy
    pulumi stack rm
    ```

## Project Layout

- `Pulumi.yaml` — Pulumi project and template metadata
- `index.ts` — Main Pulumi script
- `user-data.sh` - EC2 init script
- [`docker/rpc/build/start.sh`](docker/rpc/build/start.sh) - Silvana RPC installation script

## Configuration

aws:region: eu-central-1 (TiDB serverless with fulltext search region)
