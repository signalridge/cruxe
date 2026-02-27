/**
 * Application configuration with environment variable loading.
 */

import * as process from "process";

/** Default HTTP server port. */
export const DEFAULT_PORT = 8080;

/** Default connection pool size. */
export const DEFAULT_POOL_SIZE = 5;

/** Application configuration shape. */
export interface AppConfig {
  /** Address the HTTP server binds to. */
  bindAddress: string;
  /** Port the HTTP server listens on. */
  port: number;
  /** PostgreSQL connection string. */
  databaseUrl: string;
  /** Secret key for JWT signing. */
  jwtSecret: string;
  /** Maximum number of database connections. */
  poolSize: number;
  /** Whether debug logging is enabled. */
  debug: boolean;
}

/** Errors thrown when configuration loading fails. */
export class ConfigError extends Error {
  constructor(
    message: string,
    public readonly variable?: string,
  ) {
    super(message);
    this.name = "ConfigError";
  }
}

/**
 * Load application config from environment variables.
 *
 * If `envFile` is provided, it will be read first (not implemented in
 * this fixture). Environment variables always take precedence.
 *
 * @param envFile - Optional path to a .env file.
 * @returns A fully resolved AppConfig.
 * @throws {ConfigError} If a required variable is missing.
 */
export function loadConfig(envFile?: string): AppConfig {
  void envFile;

  const portStr = process.env["PORT"];
  let port = DEFAULT_PORT;
  if (portStr) {
    port = parseInt(portStr, 10);
    if (isNaN(port) || port < 1 || port > 65535) {
      throw new ConfigError(`Invalid port: ${portStr}`, "PORT");
    }
  }

  const poolStr = process.env["POOL_SIZE"];
  let poolSize = DEFAULT_POOL_SIZE;
  if (poolStr) {
    poolSize = parseInt(poolStr, 10);
    if (isNaN(poolSize) || poolSize < 1) {
      throw new ConfigError(`Invalid pool size: ${poolStr}`, "POOL_SIZE");
    }
  }

  return {
    bindAddress: process.env["BIND_ADDRESS"] ?? "127.0.0.1",
    port,
    databaseUrl:
      process.env["DATABASE_URL"] ?? "postgres://localhost/cruxe_dev",
    jwtSecret:
      process.env["JWT_SECRET"] ?? "development-secret-do-not-use-in-prod",
    poolSize,
    debug: process.env["DEBUG"] === "1" || process.env["DEBUG"] === "true",
  };
}

/**
 * Validate that all required configuration fields are present and non-empty.
 */
export function validateConfig(config: AppConfig): string[] {
  const errors: string[] = [];

  if (!config.databaseUrl) {
    errors.push("databaseUrl is required");
  }
  if (!config.jwtSecret) {
    errors.push("jwtSecret is required");
  }
  if (config.port < 1 || config.port > 65535) {
    errors.push(`port ${config.port} is out of range`);
  }

  return errors;
}
