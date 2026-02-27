"""
Cruxe Python sample package for testing symbol extraction.

Provides authentication, database access, request handling, and
configuration management.
"""

from .auth import validate_token, require_auth, AuthError
from .config import Config, load_config
from .database import DatabaseConnection
from .handlers import RequestHandler, handle_request
from .models import User, Role, UserProfile

__version__ = "0.1.0"

__all__ = [
    "AuthError",
    "Config",
    "DatabaseConnection",
    "RequestHandler",
    "Role",
    "User",
    "UserProfile",
    "handle_request",
    "load_config",
    "require_auth",
    "validate_token",
]
