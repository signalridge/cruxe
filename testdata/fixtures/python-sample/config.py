"""Application configuration with environment variable loading."""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from typing import Optional

# Default port the server listens on.
DEFAULT_PORT = 8080

# Default database connection pool size.
DEFAULT_POOL_SIZE = 5


class ConfigError(Exception):
    """Raised when configuration loading or validation fails."""

    def __init__(self, message: str, variable: Optional[str] = None) -> None:
        super().__init__(message)
        self.variable = variable


@dataclass
class Config:
    """Application configuration container.

    All fields have sensible defaults for local development.
    Production deployments should set the corresponding environment
    variables.
    """

    bind_address: str = "127.0.0.1"
    port: int = DEFAULT_PORT
    database_url: str = "postgres://localhost/cruxe_dev"
    jwt_secret: str = "development-secret-do-not-use-in-prod"
    pool_size: int = DEFAULT_POOL_SIZE
    debug: bool = True
    allowed_origins: list[str] = field(
        default_factory=lambda: ["http://localhost:3000"]
    )

    def validate(self) -> list[str]:
        """Return a list of validation error messages (empty if valid)."""
        errors: list[str] = []

        if not self.database_url:
            errors.append("database_url is required")
        if not self.jwt_secret:
            errors.append("jwt_secret is required")
        if not 1 <= self.port <= 65535:
            errors.append(f"port {self.port} is out of range (1-65535)")
        if self.pool_size < 1:
            errors.append(f"pool_size must be >= 1, got {self.pool_size}")

        return errors

    @property
    def server_address(self) -> str:
        """Return the full bind address with port."""
        return f"{self.bind_address}:{self.port}"


def load_config(env_file: Optional[str] = None, strict: bool = False) -> Config:
    """Load configuration from environment variables.

    Args:
        env_file: Optional path to a .env file (not implemented in fixture).
        strict: If True, raise on missing required variables.

    Returns:
        A populated Config instance.

    Raises:
        ConfigError: If strict mode is enabled and a required variable is missing,
            or if a variable value is invalid.
    """
    _ = env_file  # would load .env in real implementation

    config = Config()

    if bind := os.environ.get("BIND_ADDRESS"):
        config.bind_address = bind

    if port_str := os.environ.get("PORT"):
        try:
            config.port = int(port_str)
        except ValueError:
            raise ConfigError(f"Invalid port: {port_str!r}", "PORT")

    if db_url := os.environ.get("DATABASE_URL"):
        config.database_url = db_url
    elif strict:
        raise ConfigError("DATABASE_URL is required", "DATABASE_URL")

    if secret := os.environ.get("JWT_SECRET"):
        config.jwt_secret = secret
    elif strict:
        raise ConfigError("JWT_SECRET is required", "JWT_SECRET")

    debug_val = os.environ.get("DEBUG", "").lower()
    if debug_val in ("1", "true"):
        config.debug = True
    elif debug_val in ("0", "false"):
        config.debug = False

    if origins := os.environ.get("ALLOWED_ORIGINS"):
        config.allowed_origins = [o.strip() for o in origins.split(",") if o.strip()]

    return config
