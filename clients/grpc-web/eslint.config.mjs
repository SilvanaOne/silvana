import { dirname } from "path";
import { fileURLToPath } from "url";
import { FlatCompat } from "@eslint/eslintrc";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const compat = new FlatCompat({
  baseDirectory: __dirname,
});

const eslintConfig = [
  ...compat.extends("next/core-web-vitals", "next/typescript"),
  {
    // Global configuration to disable unused disable directive warnings
    linterOptions: {
      reportUnusedDisableDirectives: false,
    },
  },
  {
    // Specific configuration for proto files
    files: ["src/proto/**/*.ts"],
    rules: {
      // Disable all rules for generated protobuf files
      "@typescript-eslint/no-unused-vars": "off",
      "@typescript-eslint/no-explicit-any": "off",
      "@typescript-eslint/ban-ts-comment": "off",
      "prefer-const": "off",
    },
  },
];

export default eslintConfig;
