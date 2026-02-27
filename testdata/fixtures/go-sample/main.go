// Package main is the entry point for the Cruxe Go sample application.
package main

import (
	"fmt"
	"log"
	"os"
	"os/signal"
	"syscall"

	"cruxe/config"
	"cruxe/database"
	"cruxe/handlers"
)

// version is set at build time via ldflags.
var version = "dev"

func main() {
	log.SetFlags(log.LstdFlags | log.Lshortfile)
	log.Printf("starting cruxe %s", version)

	cfg, err := config.LoadConfig("")
	if err != nil {
		log.Fatalf("failed to load config: %v", err)
	}

	db, err := database.NewConnection(cfg.DatabaseURL, cfg.PoolSize)
	if err != nil {
		log.Fatalf("failed to connect to database: %v", err)
	}
	defer db.Close()

	handler := handlers.NewRequestHandler(cfg, db)

	// Set up graceful shutdown on SIGINT/SIGTERM.
	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)

	go func() {
		addr := fmt.Sprintf("%s:%d", cfg.BindAddress, cfg.Port)
		log.Printf("listening on %s", addr)
		if err := serve(addr, handler); err != nil {
			log.Fatalf("server error: %v", err)
		}
	}()

	sig := <-quit
	log.Printf("received signal %v, shutting down", sig)
}

// serve starts the HTTP server. In a real application this would use
// net/http.ListenAndServe; here it blocks until the context is cancelled.
func serve(addr string, handler *handlers.RequestHandler) error {
	_ = addr
	_ = handler
	// Block forever (real server would listen here).
	select {}
}
