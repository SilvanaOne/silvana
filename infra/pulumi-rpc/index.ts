import * as pulumi from "@pulumi/pulumi";
import * as aws from "@pulumi/aws";
import * as awsx from "@pulumi/awsx";
import * as fs from "fs";
import * as path from "path";

export = async () => {
  const stack = pulumi.getStack();
  // Get ARM64 AMI for eu-central-1 region
  const amiIdArm64 = (
    await aws.ssm.getParameter({
      name: "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-arm64",
    })
  ).value;

  const keyPairName = "RPC2"; // Using RPC keypair
  const region = "eu-central-1"; // Using eu-central-1 region

  // -------------------------
  // IAM Role and Policies for EC2 Instance
  // -------------------------

  // Create IAM role for EC2 instance
  const ec2Role = new aws.iam.Role("silvana-rpc-ec2-role", {
    assumeRolePolicy: JSON.stringify({
      Version: "2012-10-17",
      Statement: [
        {
          Action: "sts:AssumeRole",
          Effect: "Allow",
          Principal: {
            Service: "ec2.amazonaws.com",
          },
        },
      ],
    }),
    tags: {
      Name: "silvana-rpc-ec2-role",
      Project: "silvana-rpc",
    },
  });

  // Create policy for Parameter Store access
  const parameterStorePolicy = new aws.iam.Policy(
    "silvana-rpc-parameter-store-policy",
    {
      description:
        "Policy for accessing Silvana RPC environment variables in Parameter Store",
      policy: JSON.stringify({
        Version: "2012-10-17",
        Statement: [
          {
            Effect: "Allow",
            Action: ["ssm:GetParameter"],
            Resource: "arn:aws:ssm:*:*:parameter/silvana-rpc/*/env",
          },
          {
            Effect: "Allow",
            Action: "kms:Decrypt",
            Resource: "*",
          },
        ],
      }),
      tags: {
        Name: "silvana-rpc-parameter-store-policy",
        Project: "silvana-rpc",
      },
    }
  );

  // -------------------------
  // KMS Key for Secrets Encryption
  // -------------------------

  const secretsKmsKey = new aws.kms.Key("silvana-secrets-encryption-key", {
    description: "KMS key for encrypting Silvana secrets at rest",
    keyUsage: "ENCRYPT_DECRYPT",
    customerMasterKeySpec: "SYMMETRIC_DEFAULT",
    // tags: {
    //   Name: "silvana-secrets-encryption",
    //   Purpose: "Encrypt secrets at rest",
    //   Project: "silvana-rpc",
    // },
  });

  const secretsKmsKeyAlias = new aws.kms.Alias(
    "silvana-secrets-encryption-alias",
    {
      name: "alias/silvana-secrets-encryption",
      targetKeyId: secretsKmsKey.id,
    }
  );

  // -------------------------
  // DynamoDB Table for Secrets Storage
  // -------------------------

  const secretsTable = new aws.dynamodb.Table("silvana-secrets", {
    name: "silvana-secrets",
    billingMode: "PAY_PER_REQUEST",
    hashKey: "id", // Binary composite key (developer + agent + app + app_instance + name)
    attributes: [
      {
        name: "id",
        type: "B", // Binary
      },
    ],
    pointInTimeRecovery: {
      enabled: true,
    },
    tags: {
      Name: "silvana-secrets",
      Purpose: "Store encrypted secrets for developers and agents",
      BackupEnabled: "true",
      Project: "silvana-rpc",
    },
  });

  // -------------------------
  // DynamoDB Table for Configuration Storage
  // -------------------------

  const configTable = new aws.dynamodb.Table("silvana-config", {
    name: "silvana-config",
    billingMode: "PAY_PER_REQUEST",
    hashKey: "chain", // Partition key: chain identifier (testnet/devnet/mainnet)
    rangeKey: "config_key", // Sort key: configuration key
    attributes: [
      {
        name: "chain",
        type: "S", // String
      },
      {
        name: "config_key",
        type: "S", // String
      },
    ],
    pointInTimeRecovery: {
      enabled: true,
    },
    tags: {
      Name: "silvana-config",
      Purpose: "Store configuration key-value pairs per chain",
      BackupEnabled: "true",
      Project: "silvana-rpc",
    },
  });

  // Create policy for DynamoDB and KMS access (for secrets and config storage)
  const secretsPolicy = new aws.iam.Policy("silvana-rpc-secrets-policy", {
    description:
      "Policy for accessing DynamoDB and KMS for secrets and config storage",
    policy: pulumi.interpolate`{
      "Version": "2012-10-17",
      "Statement": [
        {
          "Effect": "Allow",
          "Action": [
            "dynamodb:GetItem",
            "dynamodb:PutItem",
            "dynamodb:DeleteItem",
            "dynamodb:UpdateItem"
          ],
          "Resource": "${secretsTable.arn}"
        },
        {
          "Effect": "Allow",
          "Action": [
            "dynamodb:Query",
            "dynamodb:BatchWriteItem",
            "dynamodb:PutItem",
            "dynamodb:DeleteItem"
          ],
          "Resource": "${configTable.arn}"
        },
        {
          "Effect": "Allow",
          "Action": [
            "kms:Decrypt",
            "kms:GenerateDataKey"
          ],
          "Resource": "${secretsKmsKey.arn}"
        }
      ]
    }`,
    tags: {
      Name: "silvana-rpc-secrets-policy",
      Project: "silvana-rpc",
    },
  });

  // -------------------------
  // S3 Bucket for Proofs Cache
  // -------------------------
  const proofsCacheBucket = new aws.s3.Bucket(`proofs-cache-${stack}`, {
    bucket: `proofs-cache-${stack}`,
    acl: "private",
    lifecycleRules: [
      {
        enabled: true,
        id: "expire-after-one-week",
        expiration: {
          days: 7,
        },
      },
    ],
    tags: {
      Name: `proofs-cache-${stack}`,
      Purpose: "Cache for proof data with automatic expiration",
      Project: "silvana-rpc",
    },
  });

  // -------------------------
  // S3 Bucket for Private Data Availability (DA) - Long-term Storage
  // -------------------------
  const privateDaBucket = new aws.s3.BucketV2(`silvana-private-da-${stack}`, {
    bucket: `silvana-private-da-${stack}`,
    tags: {
      Name: `silvana-private-da-${stack}`,
      Purpose: "Private data availability storage with permanent retention",
      Project: "silvana-rpc",
    },
  }, {
    protect: true, // Prevents Pulumi from deleting this bucket
    retainOnDelete: true, // Additional safety - bucket is retained even if removed from code
  });

  // Enable versioning to keep all object versions
  const privateDaVersioning = new aws.s3.BucketVersioningV2(`silvana-private-da-${stack}-versioning`, {
    bucket: privateDaBucket.bucket,
    versioningConfiguration: {
      status: "Enabled",
      mfaDelete: "Disabled", // Can be enabled later for extra protection
    },
  });

  // Block all public access
  const privateDaPublicAccessBlock = new aws.s3.BucketPublicAccessBlock(`silvana-private-da-${stack}-pab`, {
    bucket: privateDaBucket.bucket,
    blockPublicAcls: true,
    blockPublicPolicy: true,
    ignorePublicAcls: true,
    restrictPublicBuckets: true,
  });

  // Configure Object Lock for immutability (COMPLIANCE mode prevents deletion)
  const privateDaObjectLock = new aws.s3.BucketObjectLockConfigurationV2(`silvana-private-da-${stack}-lock`, {
    bucket: privateDaBucket.bucket,
    objectLockEnabled: "Enabled",
    rule: {
      defaultRetention: {
        mode: "COMPLIANCE", // Objects cannot be deleted or overwritten
        years: 10, // 10-year retention by default
      },
    },
  }, {
    dependsOn: [privateDaVersioning],
  });

  // Configure lifecycle transitions to cheaper storage
  const privateDaLifecycle = new aws.s3.BucketLifecycleConfigurationV2(`silvana-private-da-${stack}-lifecycle`, {
    bucket: privateDaBucket.bucket,
    rules: [{
      id: "transition-to-cheaper-storage",
      status: "Enabled",
      transitions: [
        {
          days: 30, // After 30 days (AWS minimum for STANDARD_IA)
          storageClass: "STANDARD_IA", // Infrequent Access (cheaper)
        },
        {
          days: 60, // After 2 months
          storageClass: "GLACIER_IR", // Glacier Instant Retrieval (cheaper, instant access)
        },
        {
          days: 365, // After 1 year
          storageClass: "DEEP_ARCHIVE", // Cheapest storage, 12-48 hour retrieval
        },
      ],
      // No expiration - data stored forever
    }],
  }, {
    dependsOn: [privateDaVersioning],
  });

  // Update S3 policy to include proofs-cache bucket and private DA bucket
  const s3PolicyWithProofsCache = new aws.iam.Policy(
    "silvana-rpc-s3-policy-with-proofs",
    {
      description:
        "Policy for accessing S3 buckets for SSL certificates, proofs cache, and private DA storage",
      policy: pulumi.interpolate`{
      "Version": "2012-10-17",
      "Statement": [
        {
          "Effect": "Allow",
          "Action": ["s3:GetObject", "s3:PutObject"],
          "Resource": "arn:aws:s3:::silvana-images-${stack}/*"
        },
        {
          "Effect": "Allow",
          "Action": [
            "s3:GetObject",
            "s3:PutObject",
            "s3:DeleteObject",
            "s3:GetObjectTagging",
            "s3:PutObjectTagging"
          ],
          "Resource": "${proofsCacheBucket.arn}/*"
        },
        {
          "Effect": "Allow",
          "Action": [
            "s3:ListBucket",
            "s3:GetBucketLocation"
          ],
          "Resource": "${proofsCacheBucket.arn}"
        },
        {
          "Effect": "Allow",
          "Action": [
            "s3:GetObject",
            "s3:GetObjectVersion",
            "s3:PutObject",
            "s3:PutObjectRetention",
            "s3:GetObjectRetention"
          ],
          "Resource": "${privateDaBucket.arn}/*"
        },
        {
          "Effect": "Allow",
          "Action": [
            "s3:ListBucket",
            "s3:GetBucketLocation",
            "s3:ListBucketVersions"
          ],
          "Resource": "${privateDaBucket.arn}"
        },
        {
          "Effect": "Allow",
          "Action": ["s3:GetObject", "s3:PutObject"],
          "Resource": "arn:aws:s3:::silvana-distribution/*"
        },
        {
          "Effect": "Allow",
          "Action": ["s3:ListBucket"],
          "Resource": "arn:aws:s3:::silvana-distribution"
        }
      ]
    }`,
      tags: {
        Name: `silvana-rpc-s3-policy-with-proofs-${stack}`,
        Project: "silvana-rpc",
      },
    }
  );

  // -------------------------
  // IAM User for S3 uploads (API keys)
  // -------------------------
  const s3UploaderUser = new aws.iam.User("silvana-rpc-s3-uploader", {
    tags: {
      Name: "silvana-rpc-s3-uploader",
      Project: "silvana-rpc",
    },
  });

  // Attach S3 policy to the user so the generated API keys have upload permissions
  new aws.iam.UserPolicyAttachment("silvana-rpc-s3-uploader-attachment", {
    user: s3UploaderUser.name,
    policyArn: s3PolicyWithProofsCache.arn,
  });

  // Create an access key (AccessKeyId / SecretAccessKey) for the user
  const s3UploaderAccessKey = new aws.iam.AccessKey(
    "silvana-rpc-s3-uploader-access-key",
    {
      user: s3UploaderUser.name,
    }
  );

  // Persist the new API keys locally for the build pipeline in standard AWS credentials format
  // NOTE: This writes the keys in plaintext; ensure .env.build is Git‑ignored.
  pulumi
    .all([s3UploaderAccessKey.id, s3UploaderAccessKey.secret])
    .apply(([accessKeyId, secretAccessKey]) => {
      const envFilePath = path.resolve(
        __dirname,
        `../../docker/rpc/.env.${stack}`
      );
      fs.writeFileSync(
        envFilePath,
        `[default]\naws_access_key_id     = ${accessKeyId}\naws_secret_access_key = ${secretAccessKey}\n`,
        { mode: 0o600 } // restrict permissions
      );
    });

  // Attach policies to the role
  new aws.iam.RolePolicyAttachment("silvana-rpc-parameter-store-attachment", {
    role: ec2Role.name,
    policyArn: parameterStorePolicy.arn,
  });

  new aws.iam.RolePolicyAttachment("silvana-rpc-s3-attachment", {
    role: ec2Role.name,
    policyArn: s3PolicyWithProofsCache.arn,
  });

  new aws.iam.RolePolicyAttachment("silvana-rpc-secrets-attachment", {
    role: ec2Role.name,
    policyArn: secretsPolicy.arn,
  });

  // Create instance profile
  const instanceProfile = new aws.iam.InstanceProfile(
    "silvana-rpc-instance-profile",
    {
      role: ec2Role.name,
      tags: {
        Name: "silvana-rpc-instance-profile",
        Project: "silvana-rpc",
      },
    }
  );

  // -------------------------
  // Store Environment Variables in Parameter Store
  // -------------------------

  // Read and store the .env.${stack} file in Parameter Store
  const envContent = fs.readFileSync(`./.env.${stack}`, "utf8");

  // Append the S3 bucket name and config table name to the environment variables
  const envContentWithResources = pulumi.interpolate`${envContent}\nPROOFS_CACHE_BUCKET=${proofsCacheBucket.bucket}\nDYNAMODB_CONFIG_TABLE=${configTable.name}`;

  const envParameter = new aws.ssm.Parameter(`silvana-rpc-env-${stack}`, {
    name: `/silvana-rpc/${stack}/env`,
    type: "SecureString",
    value: envContentWithResources,
    keyId: "alias/aws/ssm",
    description: "Silvana RPC environment variables for development",
    tags: {
      Name: `silvana-rpc-env-${stack}`,
      Project: "silvana-rpc",
      Environment: "dev",
    },
  });

  // Create Elastic IP for the RPC instance
  const rpcElasticIp = new aws.ec2.Eip("silvana-rpc-ip", {
    domain: "vpc",
    tags: {
      Name: "silvana-rpc-ip",
      Project: "silvana-rpc",
    },
  });

  // Create Security Group allowing SSH, HTTPS, gRPC, NATS, and monitoring ports
  const securityGroup = new aws.ec2.SecurityGroup("silvana-rpc-sg", {
    name: "silvana-rpc-sg",
    description:
      "Security group for Silvana RPC: SSH (22), HTTP (80), HTTPS (443), gRPC (50051), NATS (4222), NATS-WS (8080), NATS monitoring (8222), Prometheus metrics (9090)",
    ingress: [
      {
        description: "SSH",
        fromPort: 22,
        toPort: 22,
        protocol: "tcp",
        cidrBlocks: ["0.0.0.0/0"],
      },
      {
        description: "gRPC with TLS Port 443",
        fromPort: 443,
        toPort: 443,
        protocol: "tcp",
        cidrBlocks: ["0.0.0.0/0"],
      },
      {
        description: "gRPC Port 50051",
        fromPort: 50051,
        toPort: 50051,
        protocol: "tcp",
        cidrBlocks: ["0.0.0.0/0"],
      },
      {
        description: "Port 80",
        fromPort: 80,
        toPort: 80,
        protocol: "tcp",
        cidrBlocks: ["0.0.0.0/0"],
      },
      {
        description: "NATS Port 4222",
        fromPort: 4222,
        toPort: 4222,
        protocol: "tcp",
        cidrBlocks: ["0.0.0.0/0"],
      },
      {
        description: "NATS WebSocket Port 8080",
        fromPort: 8080,
        toPort: 8080,
        protocol: "tcp",
        cidrBlocks: ["0.0.0.0/0"],
      },
      {
        description: "NATS Monitoring Port 8222",
        fromPort: 8222,
        toPort: 8222,
        protocol: "tcp",
        cidrBlocks: ["0.0.0.0/0"],
      },
      {
        description: "Prometheus Metrics Port 9090",
        fromPort: 9090,
        toPort: 9090,
        protocol: "tcp",
        cidrBlocks: ["0.0.0.0/0"],
      },
    ],
    egress: [
      {
        description: "All outbound traffic",
        fromPort: 0,
        toPort: 0,
        protocol: "-1",
        cidrBlocks: ["0.0.0.0/0"],
      },
    ],
    tags: {
      Name: "silvana-rpc-sg",
      Project: "silvana-rpc",
    },
  });

  // Create EC2 Instance with Graviton c7g.4xlarge (without Nitro Enclaves)
  const rpcInstance = new aws.ec2.Instance(
    "silvana-rpc-instance",
    {
      ami: amiIdArm64,
      instanceType: "t4g.nano", //c7g.4xlarge", // Graviton processor t4g.micro - 1 GB RAM
      keyName: keyPairName,
      vpcSecurityGroupIds: [securityGroup.id],
      iamInstanceProfile: instanceProfile.name,

      // NO Nitro Enclaves enabled (removed enclaveOptions)

      rootBlockDevice: {
        volumeSize: 10,
        volumeType: "gp3",
        deleteOnTermination: true,
      },

      // User data script loaded from template and processed
      userData: (() => {
        const timestamp = new Date()
          .toISOString()
          .replace("T", " ")
          .substring(0, 19);
        const s3Bucket = `silvana-images-${stack}`;
        const template = fs.readFileSync("./user-data.sh", "utf8");

        return template
          .replace(/{{DEPLOY_TIMESTAMP}}/g, timestamp)
          .replace(/{{CHAIN}}/g, stack)
          .replace(/{{S3_BUCKET}}/g, s3Bucket);
      })(),
      userDataReplaceOnChange: true,

      tags: {
        Name: "silvana-rpc-instance",
        Project: "silvana-rpc",
        "instance-script": "true",
      },
    },
    {
      dependsOn: [instanceProfile, envParameter],
      //ignoreChanges: ["userData"],
    }
  );

  // Associate Elastic IP with the instance
  const eipAssociation = new aws.ec2.EipAssociation(
    "silvana-rpc-eip-association",
    {
      instanceId: rpcInstance.id,
      allocationId: rpcElasticIp.allocationId,
    }
  );

  // Return all outputs
  return {
    rpcElasticIpAddress: rpcElasticIp.publicIp,
    securityGroupId: securityGroup.id,
    amiIdArm64: amiIdArm64,
    rpcInstanceId: rpcInstance.id,
    rpcInstancePublicIp: rpcElasticIp.publicIp,
    rpcInstancePrivateIp: rpcInstance.privateIp,
    region: region,
    keyPairName: keyPairName,
    iamRoleArn: ec2Role.arn,
    parameterStoreArn: envParameter.arn,
    s3UploaderAccessKeyId: pulumi.secret(s3UploaderAccessKey.id),
    s3UploaderSecretAccessKey: pulumi.secret(s3UploaderAccessKey.secret),
    // Secrets storage resources
    secretsTableName: secretsTable.name,
    secretsTableArn: secretsTable.arn,
    secretsKmsKeyId: secretsKmsKey.id,
    secretsKmsKeyAlias: secretsKmsKeyAlias.name,
    // Config storage resources
    configTableName: configTable.name,
    configTableArn: configTable.arn,
    // Proofs cache bucket
    proofsCacheBucketName: proofsCacheBucket.bucket,
    proofsCacheBucketArn: proofsCacheBucket.arn,
    // Private DA bucket
    privateDaBucketName: privateDaBucket.bucket,
    privateDaBucketArn: privateDaBucket.arn,
  };
};
