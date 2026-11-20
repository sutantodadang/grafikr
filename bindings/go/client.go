package grafikr

import (
	"bufio"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os/exec"
	"path/filepath"
	"strconv"
	"sync"
	"sync/atomic"
	"time"
)

const MaxFrame = 8 << 20
const maxCallbackQueueMessages = 1024
const maxCallbackQueueBytes = 16 << 20
const maxPendingRequests = 256
const maxPendingBytes = 16 << 20
const maxImageBytes = 64 << 20
const maxImageChunkBytes = 1 << 20

type BridgeError struct {
	Code    string `json:"code"`
	Message string `json:"message"`
	Field   string `json:"field,omitempty"`
}

func (e *BridgeError) Error() string { return e.Code + ": " + e.Message }

type message struct {
	Kind     string          `json:"kind"`
	ID       string          `json:"id,omitempty"`
	OK       bool            `json:"ok,omitempty"`
	Result   json.RawMessage `json:"result,omitempty"`
	Error    *BridgeError    `json:"error,omitempty"`
	Sequence string          `json:"sequence,omitempty"`
	Event    string          `json:"event,omitempty"`
	Source   string          `json:"source,omitempty"`
	Payload  json.RawMessage `json:"payload,omitempty"`
}
type Event struct {
	Sequence, Name, Source string
	Payload                json.RawMessage
}
type queuedEvent struct {
	event     Event
	wireBytes int
}

type App struct {
	cmd              *exec.Cmd
	in               io.WriteCloser
	mu               sync.Mutex
	writeMu          sync.Mutex
	pending          map[string]chan message
	pendingSizes     map[string]int
	pendingBytes     int
	callbacks        map[string]map[string]func(Event)
	errorHandler     func(error, Event)
	subscriptionKeys map[string]string
	ids              atomic.Uint64
	closed           bool
	disposed         bool
	failure          error
	done             chan struct{}
	events           chan queuedEvent
	eventBytes       int
}

func Start(host string) (*App, error) {
	if !filepath.IsAbs(host) {
		return nil, errors.New("absolute host path is required")
	}
	ctx := context.Background()
	cmd := exec.CommandContext(ctx, host)
	configureProcess(cmd)
	in, err := cmd.StdinPipe()
	if err != nil {
		return nil, err
	}
	out, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	errout, err := cmd.StderrPipe()
	if err != nil {
		return nil, err
	}
	if err = cmd.Start(); err != nil {
		return nil, err
	}
	a := &App{cmd: cmd, in: in, pending: map[string]chan message{}, pendingSizes: map[string]int{}, callbacks: map[string]map[string]func(Event){}, subscriptionKeys: map[string]string{}, done: make(chan struct{}), events: make(chan queuedEvent, maxCallbackQueueMessages)}
	go io.Copy(io.Discard, errout)
	go a.read(out)
	go a.dispatch()
	var hello struct {
		Protocol struct {
			Major int `json:"major"`
		} `json:"protocol"`
	}
	if err = a.Request("hello", map[string]any{"protocol_major": 1, "protocol_minor": 0, "sdk_version": "1.1.0"}, &hello); err != nil {
		_ = a.Close()
		return nil, err
	}
	if hello.Protocol.Major != 1 {
		_ = a.Close()
		return nil, &BridgeError{Code: "version_mismatch", Message: "host protocol major must be 1"}
	}
	return a, nil
}

func (a *App) Request(op string, args any, result any) error {
	return a.requestTimeout(op, args, result, 30*time.Second)
}

func (a *App) requestTimeout(op string, args any, result any, timeout time.Duration) error {
	a.mu.Lock()
	if a.closed {
		err := a.failure
		a.mu.Unlock()
		if err == nil {
			err = errors.New("app is closed")
		}
		return err
	}
	if len(a.pending) >= maxPendingRequests {
		a.mu.Unlock()
		return &BridgeError{Code: "queue_full", Message: "request count quota reached"}
	}
	id := strconv.FormatUint(a.ids.Add(1), 10)
	ch := make(chan message, 1)
	a.pending[id] = ch
	a.mu.Unlock()
	frame, err := json.Marshal(map[string]any{"id": id, "op": op, "args": args})
	if err != nil {
		a.remove(id)
		return &BridgeError{Code: "invalid_value", Message: "request arguments are not valid JSON"}
	}
	frame = append(frame, '\n')
	if len(frame) > MaxFrame {
		a.remove(id)
		return &BridgeError{Code: "frame_too_large", Message: "request exceeds 8 MiB"}
	}
	a.mu.Lock()
	if _, pending := a.pending[id]; !pending {
		err := a.failure
		a.mu.Unlock()
		if err != nil {
			return err
		}
		return &BridgeError{Code: "connection_closed", Message: "app is closed"}
	}
	if a.pendingBytes+len(frame) > maxPendingBytes {
		delete(a.pending, id)
		a.mu.Unlock()
		return &BridgeError{Code: "queue_full", Message: "request byte quota reached"}
	}
	a.pendingSizes[id] = len(frame)
	a.pendingBytes += len(frame)
	a.mu.Unlock()
	a.writeMu.Lock()
	for len(frame) > 0 {
		var n int
		n, err = a.in.Write(frame)
		if err != nil || n == 0 {
			break
		}
		frame = frame[n:]
	}
	a.writeMu.Unlock()
	if err != nil || len(frame) > 0 {
		a.remove(id)
		return &BridgeError{Code: "connection_closed", Message: "failed to write host request"}
	}
	select {
	case reply := <-ch:
		if !reply.OK {
			if reply.Error != nil {
				return reply.Error
			}
			return &BridgeError{Code: "internal_failure", Message: "request failed"}
		}
		if result != nil {
			return json.Unmarshal(reply.Result, result)
		}
		return nil
	case <-time.After(timeout):
		a.remove(id)
		return &BridgeError{Code: "timeout", Message: "request timed out"}
	case <-a.done:
		a.mu.Lock()
		failure := a.failure
		a.mu.Unlock()
		if failure != nil {
			return failure
		}
		return &BridgeError{Code: "connection_closed", Message: "host exited"}
	}
}

func (a *App) Subscribe(source, event string, fn func(Event)) (string, error) {
	key := source + "\x00" + event
	provisional := "pending:" + strconv.FormatUint(a.ids.Add(1), 10)
	a.mu.Lock()
	if a.closed {
		a.mu.Unlock()
		return "", &BridgeError{Code: "connection_closed", Message: "app is closed"}
	}
	if a.callbacks[key] == nil {
		a.callbacks[key] = map[string]func(Event){}
	}
	a.callbacks[key][provisional] = fn
	a.mu.Unlock()
	var out struct {
		Subscription string `json:"subscription"`
	}
	if err := a.Request("event.subscribe", map[string]any{"node": source, "event": event}, &out); err != nil {
		a.removeSubscription(key, provisional)
		return "", err
	}
	a.mu.Lock()
	delete(a.callbacks[key], provisional)
	a.callbacks[key][out.Subscription] = fn
	a.subscriptionKeys[out.Subscription] = key
	a.mu.Unlock()
	return out.Subscription, nil
}

func (a *App) SetErrorHandler(fn func(error, Event)) {
	a.mu.Lock()
	a.errorHandler = fn
	a.mu.Unlock()
}

func (a *App) Unsubscribe(handle string) error {
	a.mu.Lock()
	key := a.subscriptionKeys[handle]
	a.mu.Unlock()
	a.removeSubscription(key, handle)
	return a.Request("event.unsubscribe", map[string]any{"subscription": handle}, nil)
}

func (a *App) Close() error {
	a.mu.Lock()
	if a.disposed {
		a.mu.Unlock()
		return nil
	}
	wasClosed := a.closed
	a.disposed = true
	a.mu.Unlock()
	if !wasClosed {
		_ = a.requestTimeout("shutdown", map[string]any{}, nil, 5*time.Second)
	}
	_ = a.in.Close()
	finished := make(chan error, 1)
	go func() { finished <- a.cmd.Wait() }()
	select {
	case err := <-finished:
		a.setFailure(&BridgeError{Code: "connection_closed", Message: "app closed"})
		return err
	case <-time.After(5 * time.Second):
		_ = a.cmd.Process.Kill()
		err := <-finished
		a.setFailure(&BridgeError{Code: "timeout", Message: "host failed to stop within five seconds"})
		return err
	}
}

func (a *App) read(r io.Reader) {
	br := bufio.NewReaderSize(r, 4096)
	for {
		var frame []byte
		for {
			part, err := br.ReadSlice('\n')
			frame = append(frame, part...)
			if len(frame) > MaxFrame {
				a.setFailure(&BridgeError{Code: "frame_too_large", Message: "host response exceeds 8 MiB"})
				return
			}
			if err == bufio.ErrBufferFull {
				continue
			}
			if err != nil {
				if err != io.EOF {
					a.setFailure(err)
				} else {
					a.setFailure(&BridgeError{Code: "connection_closed", Message: "host output closed"})
				}
				return
			}
			break
		}
		var m message
		if err := json.Unmarshal(frame, &m); err != nil {
			a.setFailure(&BridgeError{Code: "invalid_frame", Message: "host emitted invalid JSON"})
			return
		}
		if m.Kind == "response" {
			a.mu.Lock()
			ch := a.pending[m.ID]
			delete(a.pending, m.ID)
			a.pendingBytes -= a.pendingSizes[m.ID]
			delete(a.pendingSizes, m.ID)
			a.mu.Unlock()
			if ch != nil {
				ch <- m
			}
		} else if m.Kind == "event" {
			event := Event{m.Sequence, m.Event, m.Source, m.Payload}
			queued := queuedEvent{event: event, wireBytes: len(frame)}
			a.mu.Lock()
			if a.eventBytes+queued.wireBytes > maxCallbackQueueBytes {
				a.mu.Unlock()
				a.setFailure(&BridgeError{Code: "output_overflow", Message: "event byte quota exceeded"})
				return
			}
			a.eventBytes += queued.wireBytes
			a.mu.Unlock()
			select {
			case a.events <- queued:
			case <-a.done:
				return
			default:
				a.mu.Lock()
				a.eventBytes -= queued.wireBytes
				a.mu.Unlock()
				a.setFailure(&BridgeError{Code: "output_overflow", Message: "event queue is full"})
				return
			}
		} else {
			a.setFailure(&BridgeError{Code: "invalid_frame", Message: "unknown message kind"})
			return
		}
	}
}
func (a *App) dispatch() {
	for {
		select {
		case e := <-a.events:
			a.mu.Lock()
			a.eventBytes -= e.wireBytes
			key := e.event.Source + "\x00" + e.event.Name
			fns := make([]func(Event), 0, len(a.callbacks[key]))
			for _, fn := range a.callbacks[key] {
				fns = append(fns, fn)
			}
			a.mu.Unlock()
			for _, fn := range fns {
				func() {
					defer func() {
						if recovered := recover(); recovered != nil {
							a.mu.Lock()
							handler := a.errorHandler
							a.mu.Unlock()
							if handler != nil {
								func() {
									defer func() { _ = recover() }()
									handler(fmt.Errorf("event callback panic: %v", recovered), e.event)
								}()
							}
						}
					}()
					fn(e.event)
				}()
			}
		case <-a.done:
			return
		}
	}
}
func (a *App) remove(id string) {
	a.mu.Lock()
	delete(a.pending, id)
	a.pendingBytes -= a.pendingSizes[id]
	delete(a.pendingSizes, id)
	a.mu.Unlock()
}
func (a *App) removeSubscription(key, handle string) {
	a.mu.Lock()
	delete(a.callbacks[key], handle)
	if len(a.callbacks[key]) == 0 {
		delete(a.callbacks, key)
	}
	delete(a.subscriptionKeys, handle)
	a.mu.Unlock()
}
func (a *App) setFailure(err error) {
	a.mu.Lock()
	if !a.closed {
		a.closed = true
		a.failure = err
		close(a.done)
		for id, ch := range a.pending {
			ch <- message{OK: false, Error: &BridgeError{Code: "connection_closed", Message: err.Error()}}
			delete(a.pending, id)
			a.pendingBytes -= a.pendingSizes[id]
			delete(a.pendingSizes, id)
		}
	}
	a.mu.Unlock()
}

type Window struct {
	App          *App
	Handle, Root string
}

func (a *App) CreateWindow(title string, width, height float64) (Window, error) {
	var r struct{ Window, Root string }
	e := a.Request("window.create", map[string]any{"title": title, "width": width, "height": height}, &r)
	return Window{a, r.Window, r.Root}, e
}

type Node struct {
	App    *App
	Handle string
}

func (a *App) CreateNode(w Window, widget string, properties map[string]any) (Node, error) {
	var r struct{ Node string }
	e := a.Request("node.create", map[string]any{"window": w.Handle, "widget": widget, "properties": properties}, &r)
	return Node{a, r.Node}, e
}
func (n Node) Attach(parent string) error {
	return n.App.Request("node.attach", map[string]any{"node": n.Handle, "parent": parent}, nil)
}
func (n Node) Update(properties map[string]any) error {
	return n.App.Request("property.update", map[string]any{"node": n.Handle, "properties": properties}, nil)
}
func (n Node) Destroy() error {
	return n.App.Request("node.destroy", map[string]any{"node": n.Handle}, nil)
}

type Canvas struct{ Node }

func (a *App) CreateCanvas(w Window, properties map[string]any) (Canvas, error) {
	node, err := a.CreateNode(w, "Canvas", properties)
	return Canvas{Node: node}, err
}

func (c Canvas) Update(commands []map[string]any) error {
	return c.App.Request("canvas.update", map[string]any{"node": c.Handle, "commands": commands}, nil)
}

type Image struct {
	App    *App
	Handle string
}

type CollectionMetadata struct {
	RowCount  uint64   `json:"row_count"`
	RowHeight float64  `json:"row_height"`
	Columns   []string `json:"columns"`
}

type CollectionRequest struct {
	Revision string `json:"revision"`
	Request  string `json:"request"`
	Start    uint64 `json:"start"`
	Count    uint64 `json:"count"`
}

type CollectionProvider struct {
	Node
	requests     chan CollectionRequest
	subscription string
	requestMu    sync.Mutex
	closed       bool
	closeOnce    sync.Once
	closeErr     error
}

func (a *App) CreateCollectionProvider(node Node, metadata CollectionMetadata) (*CollectionProvider, error) {
	provider := &CollectionProvider{Node: node, requests: make(chan CollectionRequest, 1)}
	subscription, err := a.Subscribe(node.Handle, "data.request", func(event Event) {
		var request CollectionRequest
		if err := json.Unmarshal(event.Payload, &request); err != nil {
			a.setFailure(&BridgeError{Code: "invalid_frame", Message: "host emitted an invalid collection request"})
			return
		}
		if !provider.enqueue(request) {
			a.setFailure(&BridgeError{Code: "output_overflow", Message: "collection request queue is full"})
		}
	})
	if err != nil {
		return nil, err
	}
	provider.subscription = subscription
	if err := a.Request("collection.create", map[string]any{
		"node": node.Handle, "row_count": metadata.RowCount,
		"row_height": metadata.RowHeight, "columns": metadata.Columns,
	}, nil); err != nil {
		_ = provider.Close()
		return nil, err
	}
	return provider, nil
}

func (p *CollectionProvider) Requests() <-chan CollectionRequest { return p.requests }

func (p *CollectionProvider) enqueue(request CollectionRequest) bool {
	p.requestMu.Lock()
	defer p.requestMu.Unlock()
	if p.closed {
		return true
	}
	select {
	case p.requests <- request:
		return true
	default:
		return false
	}
}

func (p *CollectionProvider) Update(metadata CollectionMetadata) error {
	return p.App.Request("collection.update", map[string]any{"node": p.Handle, "metadata": metadata}, nil)
}

func (p *CollectionProvider) Provide(request CollectionRequest, rows [][]any) error {
	return p.App.Request("data.provide", map[string]any{
		"node": p.Handle, "revision": request.Revision, "request": request.Request,
		"start": request.Start, "rows": rows,
	}, nil)
}

func (p *CollectionProvider) Close() error {
	p.closeOnce.Do(func() {
		p.closeErr = p.App.Unsubscribe(p.subscription)
		p.requestMu.Lock()
		p.closed = true
		close(p.requests)
		p.requestMu.Unlock()
	})
	return p.closeErr
}

// UploadImage uploads one RGBA image using the protocol's bounded chunks.
// shortcut: protocol 1 cannot abort a partial upload; add image.abort when upload cancellation is standardized.
func (a *App) UploadImage(width, height uint32, rgba []byte) (Image, error) {
	if width == 0 || height == 0 || width > 8192 || height > 8192 {
		return Image{}, &BridgeError{Code: "invalid_value", Message: "image dimensions must be between 1 and 8192"}
	}
	expected := uint64(width) * uint64(height) * 4
	if expected > maxImageBytes || expected != uint64(len(rgba)) {
		return Image{}, &BridgeError{Code: "invalid_value", Message: "RGBA byte length must match dimensions and stay within 64 MiB"}
	}
	var upload struct{ Upload string }
	if err := a.Request("image.begin", map[string]any{"width": width, "height": height, "byte_length": expected}, &upload); err != nil {
		return Image{}, err
	}
	for offset := 0; offset < len(rgba); offset += maxImageChunkBytes {
		end := offset + maxImageChunkBytes
		if end > len(rgba) {
			end = len(rgba)
		}
		if err := a.Request("image.chunk", map[string]any{
			"upload": upload.Upload,
			"offset": offset,
			"data":   base64.StdEncoding.EncodeToString(rgba[offset:end]),
		}, nil); err != nil {
			return Image{}, err
		}
	}
	var result struct{ Image string }
	if err := a.Request("image.finalize", map[string]any{"upload": upload.Upload}, &result); err != nil {
		return Image{}, err
	}
	return Image{App: a, Handle: result.Image}, nil
}

func (i Image) Destroy() error {
	return i.App.Request("image.destroy", map[string]any{"image": i.Handle}, nil)
}

type State struct {
	App    *App
	Handle string
}

func (a *App) CreateState(value any) (State, error) {
	var r struct{ State string }
	e := a.Request("state.create", map[string]any{"value": value}, &r)
	return State{a, r.State}, e
}
func (s State) Set(value any) error {
	return s.App.Request("state.set", map[string]any{"state": s.Handle, "value": value}, nil)
}
func (s State) Get() (json.RawMessage, error) {
	var r json.RawMessage
	e := s.App.Request("state.get", map[string]any{"state": s.Handle}, &r)
	return r, e
}
func (s State) Close() error {
	return s.App.Request("state.destroy", map[string]any{"state": s.Handle}, nil)
}
