// Package handlers provides HTTP request handling with authentication.
package handlers

import (
	"encoding/json"
	"fmt"
	"log"

	"cruxe/auth"
	"cruxe/config"
	"cruxe/database"
)

// Request represents a simplified HTTP request.
type Request struct {
	Method  string            `json:"method"`
	Path    string            `json:"path"`
	Headers map[string]string `json:"headers"`
	Body    string            `json:"body,omitempty"`
}

// Response represents a simplified HTTP response.
type Response struct {
	Status  int               `json:"status"`
	Body    string            `json:"body"`
	Headers map[string]string `json:"headers,omitempty"`
}

// Handler defines the interface for request handlers.
type Handler interface {
	// HandleRequest processes a single HTTP request and returns a response.
	HandleRequest(req *Request) *Response
}

// newResponse creates a response with the given status and body.
func newResponse(status int, body string) *Response {
	return &Response{
		Status:  status,
		Body:    body,
		Headers: map[string]string{"Content-Type": "application/json"},
	}
}

// okResponse creates a 200 OK response.
func okResponse(body string) *Response {
	return newResponse(200, body)
}

// errorResponse creates an error response with the given status code.
func errorResponse(status int, message string) *Response {
	body, _ := json.Marshal(map[string]string{"error": message})
	return newResponse(status, string(body))
}

// RequestHandler dispatches authenticated requests to the appropriate handler.
// It implements the Handler interface.
type RequestHandler struct {
	config *config.Config
	db     *database.Connection
	auth   *auth.AuthHandler
}

// NewRequestHandler creates a new handler with the given dependencies.
func NewRequestHandler(cfg *config.Config, db *database.Connection) *RequestHandler {
	return &RequestHandler{
		config: cfg,
		db:     db,
		auth:   auth.NewAuthHandler(cfg.JWTSecret),
	}
}

// HandleRequest processes an incoming HTTP request.
// It validates the auth token, then routes to the appropriate handler method.
func (h *RequestHandler) HandleRequest(req *Request) *Response {
	claims, err := h.authenticate(req)
	if err != nil {
		log.Printf("auth failed: %v", err)
		return errorResponse(401, err.Error())
	}

	switch {
	case req.Method == "GET" && req.Path == "/api/health":
		return h.handleHealth()
	case req.Method == "GET" && req.Path == "/api/user":
		return h.handleGetUser(claims.Sub)
	case req.Method == "POST" && req.Path == "/api/user":
		return h.handleCreateUser(req)
	default:
		return errorResponse(404, "not found")
	}
}

// authenticate extracts and validates the bearer token from request headers.
func (h *RequestHandler) authenticate(req *Request) (*auth.Claims, error) {
	header, ok := req.Headers["authorization"]
	if !ok || header == "" {
		return nil, fmt.Errorf("missing Authorization header")
	}
	return h.auth.ValidateToken(header)
}

// handleHealth returns a health check response.
func (h *RequestHandler) handleHealth() *Response {
	if !h.db.IsConnected() {
		return errorResponse(503, "database unavailable")
	}
	return okResponse(`{"status": "healthy"}`)
}

// handleGetUser fetches a user by ID from the database.
func (h *RequestHandler) handleGetUser(userID string) *Response {
	rows, err := h.db.Query(fmt.Sprintf("SELECT * FROM users WHERE id = '%s'", userID))
	if err != nil {
		log.Printf("database error: %v", err)
		return errorResponse(500, "database error")
	}
	if len(rows) == 0 {
		return errorResponse(404, "user not found")
	}

	body, _ := json.Marshal(map[string]string{"user": rows[0]})
	return okResponse(string(body))
}

// handleCreateUser creates a new user from the request body.
func (h *RequestHandler) handleCreateUser(req *Request) *Response {
	if req.Body == "" {
		return errorResponse(400, "missing request body")
	}

	var payload map[string]string
	if err := json.Unmarshal([]byte(req.Body), &payload); err != nil {
		return errorResponse(400, fmt.Sprintf("invalid JSON: %v", err))
	}

	affected, err := h.db.Execute(
		"INSERT INTO users (username, email) VALUES ($1, $2)",
		payload["username"],
		payload["email"],
	)
	if err != nil {
		return errorResponse(500, fmt.Sprintf("database error: %v", err))
	}

	body, _ := json.Marshal(map[string]int64{"created": affected})
	return okResponse(string(body))
}
