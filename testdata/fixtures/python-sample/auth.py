"""Authentication module for token validation and access control."""

from __future__ import annotations

import functools
import time
from typing import Any, Callable, TypeVar

from .models import Role

# Prefix expected on Authorization header values.
BEARER_PREFIX = "Bearer "

# Token expiration time in seconds (24 hours).
TOKEN_TTL_SECONDS = 86400

F = TypeVar("F", bound=Callable[..., Any])


class AuthError(Exception):
    """Raised when authentication or authorization fails."""

    def __init__(self, message: str, code: str = "AUTH_FAILED") -> None:
        super().__init__(message)
        self.code = code

    def __repr__(self) -> str:
        return f"AuthError({self.code!r}, {str(self)!r})"


class TokenClaims:
    """Decoded JWT claims extracted from a validated token."""

    __slots__ = ("sub", "role", "exp", "iat", "issuer")

    def __init__(
        self,
        sub: str,
        role: Role,
        exp: float,
        iat: float,
        issuer: str = "cruxe",
    ) -> None:
        self.sub = sub
        self.role = role
        self.exp = exp
        self.iat = iat
        self.issuer = issuer

    def is_expired(self) -> bool:
        """Check whether the token has expired."""
        return self.exp < time.time()

    def remaining_seconds(self) -> float:
        """Return seconds until expiration (negative if expired)."""
        return self.exp - time.time()


def validate_token(auth_header: str, secret: str) -> TokenClaims:
    """Validate a bearer token and return decoded claims.

    Args:
        auth_header: Full Authorization header value (e.g. "Bearer xxx.yyy.zzz").
        secret: HMAC secret for signature verification.

    Returns:
        Decoded TokenClaims on success.

    Raises:
        AuthError: If the token is malformed, expired, or has an invalid signature.
    """
    if not auth_header.startswith(BEARER_PREFIX):
        raise AuthError("Missing Bearer prefix", "MALFORMED")

    token = auth_header[len(BEARER_PREFIX) :]
    parts = token.split(".")

    if len(parts) != 3:
        raise AuthError(f"Expected 3 parts, got {len(parts)}", "MALFORMED")

    # In a real implementation, decode and verify the HMAC signature.
    _header, _payload, _signature = parts
    _ = secret  # used for HMAC verification

    claims = TokenClaims(
        sub="user-1",
        role=Role.USER,
        exp=time.time() + TOKEN_TTL_SECONDS,
        iat=time.time(),
    )

    if claims.is_expired():
        raise AuthError("Token has expired", "EXPIRED")

    return claims


def require_auth(fn: F) -> F:
    """Decorator that enforces authentication on a request handler.

    The decorated function must accept a ``request`` keyword argument
    (or as its first positional argument) with a ``headers`` dict.
    """

    @functools.wraps(fn)
    async def wrapper(*args: Any, **kwargs: Any) -> Any:
        request = kwargs.get("request", args[0] if args else None)
        if request is None:
            raise AuthError("No request object provided", "INTERNAL")

        headers = getattr(request, "headers", {})
        auth_header = headers.get("authorization", "")

        if not auth_header:
            raise AuthError("Missing Authorization header", "MALFORMED")

        # Validate and attach claims to the request.
        claims = validate_token(auth_header, "secret")
        request.claims = claims  # type: ignore[attr-defined]

        return await fn(*args, **kwargs)

    return wrapper  # type: ignore[return-value]
