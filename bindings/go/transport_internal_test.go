package grafikr

import (
	"fmt"
	"strings"
	"sync"
	"testing"
	"time"
)

func TestEventQueueIsBoundedByBytes(t *testing.T) {
	app := &App{pending: map[string]chan message{}, done: make(chan struct{}), events: make(chan queuedEvent, maxCallbackQueueMessages)}
	payload := strings.Repeat("x", 1<<20)
	frame := fmt.Sprintf("{\"kind\":\"event\",\"sequence\":\"1\",\"event\":\"change\",\"source\":\"1\",\"payload\":{\"text\":%q}}\n", payload)
	var input strings.Builder
	for range 17 {
		input.WriteString(frame)
	}
	app.read(strings.NewReader(input.String()))
	app.mu.Lock()
	failure := app.failure
	app.mu.Unlock()
	bridge, ok := failure.(*BridgeError)
	if !ok || bridge.Code != "output_overflow" {
		t.Fatalf("expected bounded queue failure, got %v", failure)
	}
}

func TestCommandQueueRejectsCountAndByteOverflow(t *testing.T) {
	countLimited := &App{pending: map[string]chan message{}, pendingSizes: map[string]int{}}
	for i := range maxPendingRequests {
		countLimited.pending[fmt.Sprint(i)] = make(chan message, 1)
	}
	if err := countLimited.Request("state.get", nil, nil); err == nil || err.(*BridgeError).Code != "queue_full" {
		t.Fatalf("expected count quota error, got %v", err)
	}
	byteLimited := &App{pending: map[string]chan message{}, pendingSizes: map[string]int{}, pendingBytes: maxPendingBytes}
	if err := byteLimited.Request("state.get", nil, nil); err == nil || err.(*BridgeError).Code != "queue_full" {
		t.Fatalf("expected byte quota error, got %v", err)
	}
}

func TestBlockedCallbackCannotGrowEventQueueWithoutBound(t *testing.T) {
	started := make(chan struct{})
	release := make(chan struct{})
	app := &App{
		pending: map[string]chan message{},
		done:    make(chan struct{}),
		events:  make(chan queuedEvent, maxCallbackQueueMessages),
		callbacks: map[string]map[string]func(Event){
			"1\x00click": {"sub-1": func(Event) { close(started); <-release }},
		},
	}
	var dispatch sync.WaitGroup
	dispatch.Add(1)
	go func() { defer dispatch.Done(); app.dispatch() }()
	app.events <- queuedEvent{event: Event{Name: "click", Source: "1"}, wireBytes: 64}
	select {
	case <-started:
	case <-time.After(time.Second):
		t.Fatal("blocking callback did not start")
	}
	var input strings.Builder
	for index := 0; index <= maxCallbackQueueMessages; index++ {
		fmt.Fprintf(&input, `{"kind":"event","sequence":"%d","event":"click","source":"2","payload":{}}`+"\n", index+1)
	}
	app.read(strings.NewReader(input.String()))
	app.mu.Lock()
	failure := app.failure
	app.mu.Unlock()
	bridge, ok := failure.(*BridgeError)
	if !ok || bridge.Code != "output_overflow" {
		t.Fatalf("expected event count quota failure, got %v", failure)
	}
	close(release)
	dispatch.Wait()
}
