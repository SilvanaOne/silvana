export interface Config {
  grpc: {
    host: string;
    port: number;
    url: string;
    useSsl: boolean;
  };
  grpcWeb: {
    host: string;
    port: number;
    url: string;
  };
  nats: {
    servers: string[];
    subject: string;
  };
}

export const config: Config = {
  grpc: {
    host: process.env.GRPC_HOST || "rpc.silvana.dev",
    port: parseInt(process.env.GRPC_PORT || "443"),
    get url() {
      if (process.env.TEST_SERVER) {
        return process.env.TEST_SERVER.replace(/^https?:\/\//, "");
      }
      return `${this.host}:${this.port}`;
    },
    get useSsl() {
      if (process.env.TEST_SERVER) {
        return process.env.TEST_SERVER.startsWith("https://");
      }
      // Default to SSL for port 443, insecure for other ports
      return this.port === 443;
    },
  },
  grpcWeb: {
    host: process.env.GRPC_WEB_HOST || "https://rpc.silvana.dev",
    port: parseInt(process.env.GRPC_WEB_PORT || "443"),
    get url() {
      if (process.env.TEST_SERVER) {
        return process.env.TEST_SERVER.replace(/^https?:\/\//, "");
      }
      return `${this.host}:${this.port}`;
    },
  },
  nats: {
    servers: process.env.NATS_URL?.split(",") || [
      "nats://rpc.silvana.dev:4222",
    ],
    subject: process.env.NATS_SUBJECT || "silvana.events",
  },
};
