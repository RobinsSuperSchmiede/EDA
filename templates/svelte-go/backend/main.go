// Example backend for OpenShell – Go standard library only.
package main

import (
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"os"
	"strconv"
	"strings"
	"sync"
)

type Todo struct {
	ID    int    `json:"id"`
	Title string `json:"title"`
	Done  bool   `json:"done"`
}

var (
	mu    sync.Mutex
	todos = []Todo{
		{1, "Start the app with `openshell dev`", true},
		{2, "Connect an AI agent with `openshell mcp`", false},
	}
)

func send(w http.ResponseWriter, status int, body any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(body)
}

func main() {
	port := os.Getenv("PORT")
	if port == "" {
		port = "8000"
	}
	http.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		send(w, 200, map[string]string{"status": "ok"})
	})
	http.HandleFunc("/api/todos", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		switch r.Method {
		case http.MethodGet:
			send(w, 200, todos)
		case http.MethodPost:
			var in struct{ Title string `json:"title"` }
			json.NewDecoder(r.Body).Decode(&in)
			if strings.TrimSpace(in.Title) == "" {
				send(w, 400, map[string]string{"error": "title is required"})
				return
			}
			id := 1
			for _, t := range todos {
				if t.ID >= id {
					id = t.ID + 1
				}
			}
			t := Todo{id, strings.TrimSpace(in.Title), false}
			todos = append(todos, t)
			send(w, 201, t)
		default:
			send(w, 405, map[string]string{"error": "method not allowed"})
		}
	})
	http.HandleFunc("/api/todos/", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		parts := strings.Split(strings.TrimPrefix(r.URL.Path, "/api/todos/"), "/")
		id, err := strconv.Atoi(parts[0])
		if err != nil {
			send(w, 404, map[string]string{"error": "not found"})
			return
		}
		for i, t := range todos {
			if t.ID != id {
				continue
			}
			if r.Method == http.MethodPost && len(parts) == 2 && parts[1] == "toggle" {
				todos[i].Done = !todos[i].Done
				send(w, 200, todos[i])
				return
			}
			if r.Method == http.MethodDelete {
				todos = append(todos[:i], todos[i+1:]...)
				send(w, 200, map[string]int{"deleted": 1})
				return
			}
		}
		send(w, 404, map[string]string{"error": "not found"})
	})
	fmt.Printf("backend listening on http://127.0.0.1:%s\n", port)
	log.Fatal(http.ListenAndServe("127.0.0.1:"+port, nil))
}
