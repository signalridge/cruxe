// Package config handles application configuration from environment variables.
package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

const (
	// DefaultPort is the default HTTP server port.
	DefaultPort = 8080

	// DefaultPoolSize is the default database connection pool size.
	DefaultPoolSize = 5
)

// ConfigError represents a configuration loading failure.
type ConfigError struct {
	Variable string
	Message  string
}

func (e *ConfigError) Error() string {
	return fmt.Sprintf("config error for %s: %s", e.Variable, e.Message)
}

// Config holds all application configuration values.
type Config struct {
	// BindAddress is the address the HTTP server binds to.
	BindAddress string `json:"bind_address"`
	// Port is the port the HTTP server listens on.
	Port int `json:"port"`
	// DatabaseURL is the PostgreSQL connection string.
	DatabaseURL string `json:"database_url"`
	// JWTSecret is the secret key for token signing and verification.
	JWTSecret string `json:"jwt_secret"`
	// PoolSize is the maximum database connection pool size.
	PoolSize int `json:"pool_size"`
	// Debug enables verbose logging when true.
	Debug bool `json:"debug"`
	// AllowedOrigins is the list of permitted CORS origins.
	AllowedOrigins []string `json:"allowed_origins"`
}

// ServerAddress returns the full bind address with port.
func (c *Config) ServerAddress() string {
	return fmt.Sprintf("%s:%d", c.BindAddress, c.Port)
}

// Validate checks that all required fields are present and valid.
// Returns a slice of error messages (empty if valid).
func (c *Config) Validate() []string {
	var errs []string

	if c.DatabaseURL == "" {
		errs = append(errs, "database_url is required")
	}
	if c.JWTSecret == "" {
		errs = append(errs, "jwt_secret is required")
	}
	if c.Port < 1 || c.Port > 65535 {
		errs = append(errs, fmt.Sprintf("port %d is out of range (1-65535)", c.Port))
	}
	if c.PoolSize < 1 {
		errs = append(errs, fmt.Sprintf("pool_size must be >= 1, got %d", c.PoolSize))
	}

	return errs
}

// LoadConfig reads configuration from environment variables, falling back
// to defaults for any unset variable. The envFile parameter is accepted
// for compatibility but not implemented in this fixture.
func LoadConfig(envFile string) (*Config, error) {
	_ = envFile

	cfg := &Config{
		BindAddress:    "127.0.0.1",
		Port:           DefaultPort,
		DatabaseURL:    "postgres://localhost/cruxe_dev",
		JWTSecret:      "development-secret-do-not-use-in-prod",
		PoolSize:       DefaultPoolSize,
		Debug:          true,
		AllowedOrigins: []string{"http://localhost:3000"},
	}

	if addr := os.Getenv("BIND_ADDRESS"); addr != "" {
		cfg.BindAddress = addr
	}

	if portStr := os.Getenv("PORT"); portStr != "" {
		port, err := strconv.Atoi(portStr)
		if err != nil {
			return nil, &ConfigError{Variable: "PORT", Message: fmt.Sprintf("invalid port: %s", portStr)}
		}
		cfg.Port = port
	}

	if dbURL := os.Getenv("DATABASE_URL"); dbURL != "" {
		cfg.DatabaseURL = dbURL
	}

	if secret := os.Getenv("JWT_SECRET"); secret != "" {
		cfg.JWTSecret = secret
	}

	if debug := os.Getenv("DEBUG"); debug != "" {
		cfg.Debug = debug == "1" || strings.EqualFold(debug, "true")
	}

	if origins := os.Getenv("ALLOWED_ORIGINS"); origins != "" {
		cfg.AllowedOrigins = strings.Split(origins, ",")
		for i, o := range cfg.AllowedOrigins {
			cfg.AllowedOrigins[i] = strings.TrimSpace(o)
		}
	}

	return cfg, nil
}
