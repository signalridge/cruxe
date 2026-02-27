/**
 * Cruxe TypeScript sample - main entry point.
 *
 * Re-exports public API surface for the application.
 */

export { AuthHandler, validateToken } from "./auth/handler";
export { User, UserRole, CreateUserDto } from "./models/user";
export { DatabaseService, QueryResult } from "./services/database";
export { Logger, LogLevel, createLogger } from "./utils/logger";
export { AppConfig, loadConfig, DEFAULT_PORT } from "./config";

/** Application version string. */
export const VERSION = "0.1.0";

/**
 * Bootstrap the application with the given configuration path.
 *
 * Loads config, initializes the database, and returns a ready-to-use
 * application context.
 */
export async function bootstrap(configPath?: string): Promise<{
  config: import("./config").AppConfig;
  db: DatabaseService;
  logger: Logger;
}> {
  const { loadConfig } = await import("./config");
  const { DatabaseService } = await import("./services/database");
  const { createLogger, LogLevel } = await import("./utils/logger");

  const config = loadConfig(configPath);
  const logger = createLogger(
    "app",
    config.debug ? LogLevel.Debug : LogLevel.Info,
  );
  const db = new DatabaseService(config.databaseUrl, config.poolSize);

  await db.connect();
  logger.info("Application bootstrapped successfully");

  return { config, db, logger };
}
