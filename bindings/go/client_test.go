package grafikr_test

import (
	"bufio"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/grafikr/grafikr-go"
)

func TestCollectionProviderServesVisibleRows(t *testing.T) {
	app, err := grafikr.Start(fixtureHost(t))
	if err != nil {
		t.Fatal(err)
	}
	defer app.Close()
	window, err := app.CreateWindow("collection", 320, 240)
	if err != nil {
		t.Fatal(err)
	}
	node, err := app.CreateNode(window, "VirtualList", map[string]any{"row_count": 100, "row_height": 24, "height": 120})
	if err != nil {
		t.Fatal(err)
	}
	if err := node.Attach(window.Root); err != nil {
		t.Fatal(err)
	}
	provider, err := app.CreateCollectionProvider(node, grafikr.CollectionMetadata{
		RowCount: 100, RowHeight: 24, Columns: []string{"label"},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer provider.Close()
	select {
	case request := <-provider.Requests():
		rows := make([][]any, request.Count)
		for i := range rows {
			rows[i] = []any{fmt.Sprintf("row-%d", request.Start+uint64(i))}
		}
		if err := provider.Provide(request, rows); err != nil {
			t.Fatal(err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("collection did not request visible rows")
	}
	if err := provider.Update(grafikr.CollectionMetadata{RowCount: 2, RowHeight: 30, Columns: []string{"label"}}); err != nil {
		t.Fatal(err)
	}
	if err := provider.Close(); err != nil {
		t.Fatal(err)
	}
	for range provider.Requests() {
	}
}

func TestConcurrentRequestsCorrelateOutOfOrderResponses(t *testing.T) {
	t.Setenv("GRAFIKR_FIXTURE_REVERSE_RESPONSES", "1")
	app, err := grafikr.Start(fixtureHost(t))
	if err != nil {
		t.Fatal(err)
	}
	defer app.Close()
	first, err := app.CreateState(map[string]any{"type": "i64", "value": "7"})
	if err != nil {
		t.Fatal(err)
	}
	second, err := app.CreateState(map[string]any{"type": "i64", "value": "8"})
	if err != nil {
		t.Fatal(err)
	}
	type result struct {
		value json.RawMessage
		err   error
	}
	results := make(chan result, 2)
	go func() { value, err := first.Get(); results <- result{value, err} }()
	go func() { value, err := second.Get(); results <- result{value, err} }()
	values := map[string]bool{}
	for range 2 {
		select {
		case response := <-results:
			if response.err != nil {
				t.Fatal(response.err)
			}
			var state struct {
				Value string `json:"value"`
			}
			if err := json.Unmarshal(response.value, &state); err != nil {
				t.Fatal(err)
			}
			values[state.Value] = true
		case <-time.After(2 * time.Second):
			t.Fatal("out-of-order state responses were not correlated")
		}
	}
	if !values["7"] || !values["8"] {
		t.Fatalf("responses were mismatched: %#v", values)
	}
}

func TestNestedRequestFromEventCallbackDoesNotBlockReader(t *testing.T) {
	t.Setenv("GRAFIKR_FIXTURE_EARLY_SUBSCRIBE", "1")
	app, err := grafikr.Start(fixtureHost(t))
	if err != nil {
		t.Fatal(err)
	}
	defer app.Close()
	state, err := app.CreateState(map[string]any{"type": "i64", "value": "7"})
	if err != nil {
		t.Fatal(err)
	}
	window, err := app.CreateWindow("nested callback", 320, 240)
	if err != nil {
		t.Fatal(err)
	}
	button, err := app.CreateNode(window, "Button", map[string]any{"label": "click"})
	if err != nil {
		t.Fatal(err)
	}
	if err := button.Attach(window.Root); err != nil {
		t.Fatal(err)
	}
	result := make(chan json.RawMessage, 1)
	if _, err := app.Subscribe(button.Handle, "click", func(grafikr.Event) {
		value, err := state.Get()
		if err != nil {
			result <- json.RawMessage(`{"error":true}`)
			return
		}
		result <- value
	}); err != nil {
		t.Fatal(err)
	}
	select {
	case value := <-result:
		var decoded struct {
			Value string `json:"value"`
		}
		if err := json.Unmarshal(value, &decoded); err != nil || decoded.Value != "7" {
			t.Fatalf("nested request returned %s: %v", value, err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("nested request deadlocked the event callback")
	}
}

func TestLargeUTF8StateFrameAndI64BoundariesRoundTrip(t *testing.T) {
	app, err := grafikr.Start(fixtureHost(t))
	if err != nil {
		t.Fatal(err)
	}
	defer app.Close()
	text := "line one\nline two 雪 😀" + strings.Repeat("x", 102_400)
	stringState, err := app.CreateState(map[string]any{"type": "string", "value": text})
	if err != nil {
		t.Fatal(err)
	}
	got, err := stringState.Get()
	if err != nil {
		t.Fatal(err)
	}
	var decoded struct {
		Type  string `json:"type"`
		Value string `json:"value"`
	}
	if err := json.Unmarshal(got, &decoded); err != nil || decoded.Type != "string" || decoded.Value != text {
		t.Fatalf("large UTF-8 state mismatch: type=%q value-len=%d err=%v", decoded.Type, len(decoded.Value), err)
	}

	integer, err := app.CreateState(map[string]any{"type": "i64", "value": "-9223372036854775808"})
	if err != nil {
		t.Fatal(err)
	}
	got, err = integer.Get()
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(got, &decoded); err != nil || decoded.Value != "-9223372036854775808" {
		t.Fatalf("i64 minimum mismatch: %s err=%v", got, err)
	}
	if err := integer.Set(map[string]any{"type": "i64", "value": "9223372036854775807"}); err != nil {
		t.Fatal(err)
	}
	got, err = integer.Get()
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(got, &decoded); err != nil || decoded.Value != "9223372036854775807" {
		t.Fatalf("i64 maximum mismatch: %s err=%v", got, err)
	}
}

func TestEOFRejects256PendingRequestsExactlyOnce(t *testing.T) {
	t.Setenv("GRAFIKR_FIXTURE_HOLD_RESPONSES", "1")
	t.Setenv("GRAFIKR_FIXTURE_EXIT_AFTER_HELLO_MS", "500")
	app, err := grafikr.Start(fixtureHost(t))
	if err != nil {
		t.Fatal(err)
	}
	defer app.Close()
	const count = 256
	errors := make(chan error, count)
	var started sync.WaitGroup
	started.Add(count)
	for range count {
		go func() {
			defer started.Done()
			var result json.RawMessage
			errors <- app.Request("state.get", map[string]any{"state": "0"}, &result)
		}()
	}
	started.Wait()
	for range count {
		select {
		case err := <-errors:
			bridgeErr, ok := err.(*grafikr.BridgeError)
			if !ok || bridgeErr.Code != "connection_closed" {
				t.Fatalf("pending request was not rejected once on EOF: %v", err)
			}
		case <-time.After(3 * time.Second):
			t.Fatal("pending requests were not released after host EOF")
		}
	}
}

func TestCloseKeepsUnrelatedSentinelAlive(t *testing.T) {
	marker := filepath.Join(t.TempDir(), "sentinel-alive")
	sentinel := exec.Command(os.Args[0], "-test.run=^TestSentinelProcess$")
	sentinel.Env = append(os.Environ(), "GRAFIKR_SENTINEL_MARKER="+marker)
	if err := sentinel.Start(); err != nil {
		t.Fatal(err)
	}
	app, err := grafikr.Start(fixtureHost(t))
	if err != nil {
		_ = sentinel.Process.Kill()
		_ = sentinel.Wait()
		t.Fatal(err)
	}
	if err := app.Close(); err != nil {
		t.Fatal(err)
	}
	if err := sentinel.Wait(); err != nil {
		t.Fatalf("sentinel process was affected by app close: %v", err)
	}
	if data, err := os.ReadFile(marker); err != nil || string(data) != "alive" {
		t.Fatalf("sentinel did not finish independently: data=%q err=%v", data, err)
	}
}

func TestSentinelProcess(t *testing.T) {
	marker := os.Getenv("GRAFIKR_SENTINEL_MARKER")
	if marker == "" {
		return
	}
	time.Sleep(300 * time.Millisecond)
	if err := os.WriteFile(marker, []byte("alive"), 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestBoundedUTF8FrameRead(t *testing.T) {
	input := "{\"kind\":\"event\",\"payload\":\"π\"}\n"
	reader := bufio.NewReaderSize(strings.NewReader(input), 4)
	frame := []byte{}
	for {
		part, err := reader.ReadSlice('\n')
		frame = append(frame, part...)
		if err == bufio.ErrBufferFull {
			continue
		}
		if err != nil {
			t.Fatal(err)
		}
		break
	}
	if len(frame) > grafikr.MaxFrame {
		t.Fatal("valid frame exceeded bound")
	}
}

func TestSubscribeDeliversEventBeforeResponse(t *testing.T) {
	host := fixtureHost(t)
	t.Setenv("GRAFIKR_FIXTURE_EARLY_SUBSCRIBE", "1")
	app, err := grafikr.Start(host)
	if err != nil {
		t.Fatal(err)
	}
	defer app.Close()
	window, err := app.CreateWindow("fixture", 320, 240)
	if err != nil {
		t.Fatal(err)
	}
	button, err := app.CreateNode(window, "Button", map[string]any{"label": "click"})
	if err != nil {
		t.Fatal(err)
	}
	if err := button.Attach(window.Root); err != nil {
		t.Fatal(err)
	}
	received := make(chan grafikr.Event, 1)
	subscription, err := app.Subscribe(button.Handle, "click", func(event grafikr.Event) { received <- event })
	if err != nil {
		t.Fatal(err)
	}
	defer app.Unsubscribe(subscription)
	select {
	case event := <-received:
		if event.Source != button.Handle || event.Name != "click" {
			t.Fatalf("unexpected event: %#v", event)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("early subscription event was dropped")
	}
}

func TestCallbackPanicReachesErrorHandlerAndReaderContinues(t *testing.T) {
	t.Setenv("GRAFIKR_FIXTURE_EARLY_SUBSCRIBE", "1")
	app, err := grafikr.Start(fixtureHost(t))
	if err != nil {
		t.Fatal(err)
	}
	defer app.Close()
	window, err := app.CreateWindow("callback error", 320, 240)
	if err != nil {
		t.Fatal(err)
	}
	button, err := app.CreateNode(window, "Button", map[string]any{"label": "click"})
	if err != nil {
		t.Fatal(err)
	}
	if err := button.Attach(window.Root); err != nil {
		t.Fatal(err)
	}
	errors := make(chan error, 1)
	app.SetErrorHandler(func(err error, event grafikr.Event) {
		if event.Name == "click" {
			errors <- err
		}
	})
	_, err = app.Subscribe(button.Handle, "click", func(grafikr.Event) { panic("fixture callback panic") })
	if err != nil {
		t.Fatal(err)
	}
	select {
	case callbackErr := <-errors:
		if callbackErr == nil {
			t.Fatal("panic was reported without an error")
		}
	case <-time.After(2 * time.Second):
		t.Fatal("callback panic did not reach the error handler")
	}
	state, err := app.CreateState(map[string]any{"type": "i64", "value": "7"})
	if err != nil {
		t.Fatalf("response reader stopped after callback panic: %v", err)
	}
	if value, err := state.Get(); err != nil || string(value) != `{"type":"i64","value":"7"}` {
		t.Fatalf("state request after callback panic: value=%s err=%v", value, err)
	}
}

func TestRejectsNonFiniteValuesBeforeWritingARequest(t *testing.T) {
	app, err := grafikr.Start(fixtureHost(t))
	if err != nil {
		t.Fatal(err)
	}
	defer app.Close()
	err = app.Request("state.create", map[string]any{"value": map[string]any{"type": "f64", "value": math.NaN()}}, nil)
	bridgeErr, ok := err.(*grafikr.BridgeError)
	if !ok || bridgeErr.Code != "invalid_value" {
		t.Fatalf("expected invalid_value without writing a frame, got %v", err)
	}
	if _, err := app.CreateState(map[string]any{"type": "i64", "value": "7"}); err != nil {
		t.Fatalf("request pipeline did not recover after invalid input: %v", err)
	}
}

func TestUnsubscribeSuppressesEventBeforeResponseCompletes(t *testing.T) {
	t.Setenv("GRAFIKR_FIXTURE_EARLY_SUBSCRIBE", "1")
	t.Setenv("GRAFIKR_FIXTURE_UNSUBSCRIBE_EVENT", "1")
	t.Setenv("GRAFIKR_FIXTURE_DELAY_UNSUBSCRIBE_RESPONSE_MS", "200")
	app, err := grafikr.Start(fixtureHost(t))
	if err != nil {
		t.Fatal(err)
	}
	defer app.Close()
	window, err := app.CreateWindow("unsubscribe", 320, 240)
	if err != nil {
		t.Fatal(err)
	}
	button, err := app.CreateNode(window, "Button", map[string]any{"label": "click"})
	if err != nil {
		t.Fatal(err)
	}
	if err := button.Attach(window.Root); err != nil {
		t.Fatal(err)
	}
	events := make(chan grafikr.Event, 4)
	subscription, err := app.Subscribe(button.Handle, "click", func(event grafikr.Event) { events <- event })
	if err != nil {
		t.Fatal(err)
	}
	select {
	case <-events:
	case <-time.After(2 * time.Second):
		t.Fatal("early subscription event was not delivered")
	}
	if err := app.Unsubscribe(subscription); err != nil {
		t.Fatal(err)
	}
	select {
	case event := <-events:
		t.Fatalf("event dispatched while unsubscribe was pending: %#v", event)
	case <-time.After(50 * time.Millisecond):
	}
}

func TestCreatesEverySharedWidgetAndReceivesDeclaredEvents(t *testing.T) {
	if os.Getenv("GRAFIKR_FIXTURE_WIDGET_INVENTORY") == "" {
		t.Skip("requires isolated widget inventory fixture mode")
	}
	t.Setenv("GRAFIKR_FIXTURE_EARLY_SUBSCRIBE", "1")
	data, err := os.ReadFile(filepath.Join("..", "protocol", "v1", "widgets.json"))
	if err != nil {
		t.Fatal(err)
	}
	var inventory struct {
		Widgets map[string]struct {
			Properties map[string]string `json:"properties"`
			Events     []string          `json:"events"`
		} `json:"widgets"`
	}
	if err := json.Unmarshal(data, &inventory); err != nil {
		t.Fatal(err)
	}
	app, err := grafikr.Start(fixtureHost(t))
	if err != nil {
		t.Fatal(err)
	}
	defer app.Close()
	window, err := app.CreateWindow("widget inventory", 640, 480)
	if err != nil {
		t.Fatal(err)
	}
	state, err := app.CreateState(map[string]any{"type": "i64", "value": "1"})
	if err != nil {
		t.Fatal(err)
	}
	image, err := app.UploadImage(1, 1, []byte{0, 0, 0, 255})
	if err != nil {
		t.Fatal(err)
	}
	for widget, spec := range inventory.Widgets {
		properties := make(map[string]any, len(spec.Properties))
		for name, kind := range spec.Properties {
			properties[name] = sampleWidgetValue(widget, name, kind, state.Handle, image.Handle)
		}
		node, err := app.CreateNode(window, widget, properties)
		if err != nil {
			t.Fatalf("%s create failed: %v", widget, err)
		}
		if err := node.Attach(window.Root); err != nil {
			t.Fatalf("%s attach failed: %v", widget, err)
		}
		for _, eventName := range spec.Events {
			received := make(chan grafikr.Event, 1)
			subscription, err := app.Subscribe(node.Handle, eventName, func(event grafikr.Event) { received <- event })
			if err != nil {
				t.Fatalf("%s@%s subscribe failed: %v", widget, eventName, err)
			}
			select {
			case event := <-received:
				if event.Source != node.Handle || event.Name != eventName {
					t.Fatalf("%s@%s returned unexpected event: %#v", widget, eventName, event)
				}
			case <-time.After(2 * time.Second):
				t.Fatalf("%s@%s was not delivered", widget, eventName)
			}
			if err := app.Unsubscribe(subscription); err != nil {
				t.Fatalf("%s@%s unsubscribe failed: %v", widget, eventName, err)
			}
		}
		if err := node.Destroy(); err != nil {
			t.Fatalf("%s destroy failed: %v", widget, err)
		}
	}
	if err := image.Destroy(); err != nil {
		t.Fatal(err)
	}
	if err := state.Close(); err != nil {
		t.Fatal(err)
	}
	if err := app.Request("window.close", map[string]any{"window": window.Handle}, nil); err != nil {
		t.Fatal(err)
	}
}

func TestCanvasFacadeUpdatesCommandsOverIPC(t *testing.T) {
	app, err := grafikr.Start(fixtureHost(t))
	if err != nil {
		t.Fatal(err)
	}
	defer app.Close()
	window, err := app.CreateWindow("canvas", 320, 240)
	if err != nil {
		t.Fatal(err)
	}
	canvas, err := app.CreateCanvas(window, map[string]any{"width": 100, "height": 100})
	if err != nil {
		t.Fatal(err)
	}
	if err := canvas.Attach(window.Root); err != nil {
		t.Fatal(err)
	}
	commands := []map[string]any{{
		"kind": "rect", "rect": map[string]any{"x": 0, "y": 0, "width": 20, "height": 20},
		"color": []int{255, 0, 0, 255}, "radius": 0,
	}}
	if err := canvas.Update(commands); err != nil {
		t.Fatal(err)
	}
}

func TestImageFacadeUploadsChunksAndReleasesResource(t *testing.T) {
	app, err := grafikr.Start(fixtureHost(t))
	if err != nil {
		t.Fatal(err)
	}
	defer app.Close()
	window, err := app.CreateWindow("image", 320, 240)
	if err != nil {
		t.Fatal(err)
	}
	pixels := make([]byte, 1024*512*4)
	image, err := app.UploadImage(1024, 512, pixels)
	if err != nil {
		t.Fatal(err)
	}
	node, err := app.CreateNode(window, "Image", map[string]any{
		"image": image.Handle, "width": 160, "height": 80, "label": "pixel image",
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := node.Attach(window.Root); err != nil {
		t.Fatal(err)
	}
	if err := node.Destroy(); err != nil {
		t.Fatal(err)
	}
	if err := image.Destroy(); err != nil {
		t.Fatal(err)
	}
}

func TestImageFacadeRejectsMismatchedRGBABytesBeforeRequest(t *testing.T) {
	app := &grafikr.App{}
	if _, err := app.UploadImage(2, 2, []byte{255, 0, 0, 255}); err == nil {
		t.Fatal("accepted an RGBA buffer with the wrong size")
	} else if bridgeErr, ok := err.(*grafikr.BridgeError); !ok || bridgeErr.Code != "invalid_value" {
		t.Fatalf("unexpected validation error: %v", err)
	}
}

func fixtureHost(t *testing.T) string {
	t.Helper()
	if host := os.Getenv("GRAFIKR_FIXTURE_HOST"); host != "" {
		return host
	}
	_, source, _, ok := runtime.Caller(0)
	if !ok {
		t.Skip("fixture host path is unavailable")
	}
	root, err := filepath.Abs(filepath.Join(filepath.Dir(source), "..", ".."))
	if err != nil {
		t.Fatal(err)
	}
	name := "fixture-host"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	host := filepath.Join(root, "target", "debug", name)
	if _, err := os.Stat(host); err != nil {
		t.Skip("fixture host has not been built")
	}
	return host
}

func sampleWidgetValue(widget, name, kind, state, image string) any {
	if widget == "Radio" && name == "group" {
		return state
	}
	if widget == "TreeView" && name == "selected" {
		return "root"
	}
	if widget == "Slider" {
		switch name {
		case "min":
			return 0.0
		case "max":
			return 10.0
		case "width":
			return 120.0
		default:
			return 1.0
		}
	}
	if widget == "Spinner" && name == "size" {
		return 16.0
	}
	if name == "row_count" {
		return 1
	}
	if name == "row_height" {
		return 24.0
	}
	samples := map[string]any{
		"number": 1.0, "f32": 1.0, "u16": 1, "u64": 0, "string": "sample",
		"color": "#336699", "transform": map[string]any{"x": 0.0, "y": 0.0, "scale": 1.0},
		"boolean": false, "primary|secondary|icon": "primary", "state": state,
		"GridColumn[]":    []map[string]any{{"title": "Column", "width": 120.0}},
		"string[]":        []string{"One", "Two"},
		"TreeNode[]":      []map[string]any{{"key": "root", "label": "Root", "children": []any{}}},
		"CanvasCommand[]": []map[string]any{{"kind": "rect", "rect": map[string]any{"x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0}, "color": "#336699", "radius": 0.0}},
		"ImageHandle":     image,
	}
	return samples[kind]
}

func TestInstallRuntimeVerifiesAndCachesUnderUnicodePath(t *testing.T) {
	target := map[string]string{
		"windows/amd64": "x86_64-pc-windows-msvc", "windows/arm64": "aarch64-pc-windows-msvc",
		"linux/amd64": "x86_64-unknown-linux-gnu", "linux/arm64": "aarch64-unknown-linux-gnu",
		"darwin/amd64": "x86_64-apple-darwin", "darwin/arm64": "aarch64-apple-darwin",
	}[runtime.GOOS+"/"+runtime.GOARCH]
	if target == "" {
		t.Skip("runtime target is unsupported")
	}
	base := t.TempDir()
	bundle := filepath.Join(base, "bündle with spaces")
	if err := os.MkdirAll(filepath.Join(bundle, "bin"), 0o755); err != nil {
		t.Fatal(err)
	}
	host := []byte("runtime fixture")
	if err := os.WriteFile(filepath.Join(bundle, "bin", "grafikr-host"), host, 0o644); err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(host)
	manifest := map[string]any{
		"format_version":      1,
		"engine_version":      "1.1.0",
		"target":              target,
		"development_fixture": false,
		"files": []any{map[string]any{
			"path": "bin/grafikr-host", "size": len(host), "sha256": hex.EncodeToString(digest[:]), "role": "host",
		}},
	}
	manifestBytes, err := json.Marshal(manifest)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(bundle, "manifest.json"), manifestBytes, 0o644); err != nil {
		t.Fatal(err)
	}
	cache := filepath.Join(base, "cäché")
	path, err := grafikr.InstallRuntime(bundle, cache)
	if err != nil {
		t.Fatal(err)
	}
	if !filepath.IsAbs(path) {
		t.Fatal("installer returned a non-absolute host path")
	}
	if data, err := os.ReadFile(path); err != nil || string(data) != string(host) {
		t.Fatalf("installed host mismatch: data=%q err=%v", data, err)
	}
	if runtime.GOOS != "windows" {
		info, err := os.Stat(path)
		if err != nil {
			t.Fatal(err)
		}
		if info.Mode().Perm()&0o111 == 0 {
			t.Fatalf("installed host is not executable: mode=%v", info.Mode().Perm())
		}
	}
	if err := os.WriteFile(path, []byte("tampered"), 0o755); err != nil {
		t.Fatal(err)
	}
	if _, err := grafikr.InstallRuntime(bundle, cache); err == nil {
		t.Fatal("installer accepted a tampered cached runtime")
	}
}
