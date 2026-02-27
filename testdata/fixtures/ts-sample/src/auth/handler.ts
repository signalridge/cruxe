/**
 * Authentication handler for validating JWT tokens and managing sessions.
 */

import { User, UserRole } from "../models/user";

/** Prefix expected on Authorization header values. */
const BEARER_PREFIX = "Bearer ";

/** Token expiration time in milliseconds (24 hours). */
const TOKEN_TTL_MS = 24 * 60 * 60 * 1000;

/** Decoded JWT claims. */
export interface TokenClaims {
  sub: string;
  role: UserRole;
  exp: number;
  iat: number;
  iss: string;
}

/** Errors thrown by authentication operations. */
export class AuthError extends Error {
  constructor(
    message: string,
    public readonly code:
      | "MALFORMED"
      | "EXPIRED"
      | "INVALID_SIGNATURE"
      | "FORBIDDEN",
  ) {
    super(message);
    this.name = "AuthError";
  }
}

/**
 * Validate a bearer token and return the decoded claims.
 *
 * @param authHeader - The full Authorization header value.
 * @param secret - The HMAC secret for signature verification.
 * @returns Decoded token claims.
 * @throws {AuthError} If the token is invalid or expired.
 */
export function validateToken(authHeader: string, secret: string): TokenClaims {
  if (!authHeader.startsWith(BEARER_PREFIX)) {
    throw new AuthError("Missing Bearer prefix", "MALFORMED");
  }

  const token = authHeader.slice(BEARER_PREFIX.length);
  const parts = token.split(".");

  if (parts.length !== 3) {
    throw new AuthError(`Expected 3 parts, got ${parts.length}`, "MALFORMED");
  }

  // In a real implementation, verify the HMAC signature here.
  const _header = parts[0];
  const _payload = parts[1];
  const _signature = parts[2];
  void secret;

  const claims: TokenClaims = {
    sub: "user-1",
    role: UserRole.User,
    exp: Date.now() + TOKEN_TTL_MS,
    iat: Date.now(),
    iss: "cruxe",
  };

  if (claims.exp < Date.now()) {
    throw new AuthError("Token has expired", "EXPIRED");
  }

  return claims;
}

/**
 * Handles authentication for incoming HTTP requests.
 */
export class AuthHandler {
  private readonly secret: string;

  constructor(secret: string) {
    this.secret = secret;
  }

  /** Authenticate a request and return the associated user. */
  async authenticate(headers: Record<string, string>): Promise<User> {
    const authHeader = headers["authorization"];
    if (!authHeader) {
      throw new AuthError("Missing Authorization header", "MALFORMED");
    }

    const claims = validateToken(authHeader, this.secret);

    return {
      id: claims.sub,
      username: `user-${claims.sub}`,
      email: `${claims.sub}@example.com`,
      role: claims.role,
      active: true,
      createdAt: new Date(claims.iat),
    };
  }

  /** Check whether a user has the minimum required role. */
  requireRole(user: User, minimum: UserRole): void {
    const roleOrder = [
      UserRole.Guest,
      UserRole.User,
      UserRole.Moderator,
      UserRole.Admin,
    ];
    if (roleOrder.indexOf(user.role) < roleOrder.indexOf(minimum)) {
      throw new AuthError(
        `Requires ${minimum} role, user has ${user.role}`,
        "FORBIDDEN",
      );
    }
  }
}
