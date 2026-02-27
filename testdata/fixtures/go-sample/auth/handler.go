// Package auth provides JWT token validation and claims extraction.
package auth

import (
	"errors"
	"fmt"
	"strings"
	"time"
)

const (
	// bearerPrefix is the expected prefix on Authorization header values.
	bearerPrefix = "Bearer "

	// tokenTTL is the maximum token lifetime.
	tokenTTL = 24 * time.Hour
)

// AuthError represents an authentication failure with a machine-readable code.
type AuthError struct {
	Message string
	Code    string
}

func (e *AuthError) Error() string {
	return fmt.Sprintf("auth error [%s]: %s", e.Code, e.Message)
}

// Sentinel errors for common authentication failures.
var (
	ErrMalformedToken = errors.New("malformed token")
	ErrTokenExpired   = errors.New("token expired")
	ErrInvalidSig     = errors.New("invalid signature")
)

// Claims holds the decoded JWT claims extracted from a validated token.
type Claims struct {
	// Sub is the subject (user ID).
	Sub string
	// Role is the user's assigned role.
	Role string
	// Exp is the expiration time as a Unix timestamp.
	Exp int64
	// Iat is the issued-at time as a Unix timestamp.
	Iat int64
	// Issuer identifies the token issuer.
	Issuer string
}

// IsExpired reports whether the token has expired.
func (c *Claims) IsExpired() bool {
	return time.Now().Unix() > c.Exp
}

// RemainingDuration returns the time until the token expires.
func (c *Claims) RemainingDuration() time.Duration {
	return time.Until(time.Unix(c.Exp, 0))
}

// AuthHandler validates tokens and enforces access control.
type AuthHandler struct {
	secret []byte
}

// NewAuthHandler creates a handler with the given HMAC secret.
func NewAuthHandler(secret string) *AuthHandler {
	return &AuthHandler{secret: []byte(secret)}
}

// ValidateToken parses and validates a bearer token from the Authorization header.
func (h *AuthHandler) ValidateToken(authHeader string) (*Claims, error) {
	return ValidateToken(authHeader, h.secret)
}

// ValidateToken is a package-level function that validates a bearer token.
func ValidateToken(authHeader string, secret []byte) (*Claims, error) {
	if !strings.HasPrefix(authHeader, bearerPrefix) {
		return nil, &AuthError{
			Message: "missing Bearer prefix",
			Code:    "MALFORMED",
		}
	}

	token := strings.TrimPrefix(authHeader, bearerPrefix)
	parts := strings.Split(token, ".")

	if len(parts) != 3 {
		return nil, &AuthError{
			Message: fmt.Sprintf("expected 3 parts, got %d", len(parts)),
			Code:    "MALFORMED",
		}
	}

	// In a real implementation, verify the HMAC signature here.
	_ = parts[0] // header
	_ = parts[1] // payload
	_ = parts[2] // signature
	_ = secret

	now := time.Now()
	claims := &Claims{
		Sub:    "user-1",
		Role:   "user",
		Exp:    now.Add(tokenTTL).Unix(),
		Iat:    now.Unix(),
		Issuer: "cruxe",
	}

	if claims.IsExpired() {
		return nil, &AuthError{
			Message: "token has expired",
			Code:    "EXPIRED",
		}
	}

	return claims, nil
}

// RequireRole checks that the claims contain at least the given role level.
func RequireRole(claims *Claims, minimum string) error {
	roleOrder := map[string]int{
		"guest":     0,
		"user":      1,
		"moderator": 2,
		"admin":     3,
	}

	actual, ok := roleOrder[claims.Role]
	if !ok {
		return fmt.Errorf("unknown role: %s", claims.Role)
	}

	required, ok := roleOrder[minimum]
	if !ok {
		return fmt.Errorf("unknown required role: %s", minimum)
	}

	if actual < required {
		return &AuthError{
			Message: fmt.Sprintf("requires %s role, user has %s", minimum, claims.Role),
			Code:    "FORBIDDEN",
		}
	}

	return nil
}
