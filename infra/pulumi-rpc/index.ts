import * as pulumi from "@pulumi/pulumi";
import * as aws from "@pulumi/aws";
import * as awsx from "@pulumi/awsx";
import * as fs from "fs";
import * as path from "path";

export = async () => {
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

  // Create policy for S3 access (for certificates)
  const s3Policy = new aws.iam.Policy("silvana-rpc-s3-policy", {
    description: "Policy for accessing S3 bucket for SSL certificates",
    policy: JSON.stringify({
      Version: "2012-10-17",
      Statement: [
        {
          Effect: "Allow",
          Action: ["s3:GetObject", "s3:PutObject"],
          Resource: "arn:aws:s3:::silvana-tee-images/*",
        },
      ],
    }),
    tags: {
      Name: "silvana-rpc-s3-policy",
      Project: "silvana-rpc",
    },
  });

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
    policyArn: s3Policy.arn,
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
      const envFilePath = path.resolve(__dirname, "../.env.build");
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
    policyArn: s3Policy.arn,
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

  // Read and store the .env.rpc file in Parameter Store
  const envContent = fs.readFileSync("./.env.rpc", "utf8");

  const envParameter = new aws.ssm.Parameter("silvana-rpc-env-dev", {
    name: "/silvana-rpc/dev/env",
    type: "SecureString",
    value: envContent,
    keyId: "alias/aws/ssm",
    description: "Silvana RPC environment variables for development",
    tags: {
      Name: "silvana-rpc-env-dev",
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

      // User data script loaded from user-data.sh file
      userData: fs.readFileSync("./user-data.sh", "utf8"),
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
  };
};
