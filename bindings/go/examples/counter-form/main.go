package main

import (
	"encoding/json"
	"github.com/grafikr/grafikr-go"
	"log"
	"math"
	"os"
	"os/signal"
	"strconv"
)

func main() {
	app, err := grafikr.StartWithRuntime(os.Getenv("GRAFIKR_BUNDLE"), os.Getenv("GRAFIKR_CACHE"))
	if err != nil {
		log.Fatal(err)
	}
	defer app.Close()
	w, err := app.CreateWindow("Counter and form", 480, 320)
	if err != nil {
		log.Fatal(err)
	}
	label, err := app.CreateNode(w, "Text", map[string]any{"text": "Count: 0"})
	if err != nil {
		log.Fatal(err)
	}
	_ = label.Attach(w.Root)
	button, err := app.CreateNode(w, "Button", map[string]any{"label": "Add"})
	if err != nil {
		log.Fatal(err)
	}
	_ = button.Attach(w.Root)
	count, err := app.CreateState(map[string]any{"type": "i64", "value": "0"})
	if err != nil {
		log.Fatal(err)
	}
	_, err = app.Subscribe(button.Handle, "click", func(grafikr.Event) {
		current, err := count.Get()
		if err != nil {
			return
		}
		var scalar struct {
			Value string `json:"value"`
		}
		if json.Unmarshal(current, &scalar) != nil {
			return
		}
		value, err := strconv.ParseInt(scalar.Value, 10, 64)
		if err != nil || value == math.MaxInt64 {
			return
		}
		value++
		if count.Set(map[string]any{"type": "i64", "value": strconv.FormatInt(value, 10)}) == nil {
			_ = label.Update(map[string]any{"text": "Count: " + strconv.FormatInt(value, 10)})
		}
	})
	if err != nil {
		log.Fatal(err)
	}
	textField, err := app.CreateNode(w, "TextField", map[string]any{"text": "Grafikr 雪", "width": 320})
	if err != nil {
		log.Fatal(err)
	}
	_ = textField.Attach(w.Root)
	preview, err := app.CreateNode(w, "Text", map[string]any{"text": "Grafikr 雪"})
	if err != nil {
		log.Fatal(err)
	}
	_ = preview.Attach(w.Root)
	_, err = app.Subscribe(textField.Handle, "change", func(event grafikr.Event) {
		var payload struct {
			Text string `json:"text"`
		}
		if json.Unmarshal(event.Payload, &payload) == nil {
			_ = preview.Update(map[string]any{"text": payload.Text})
		}
	})
	if err != nil {
		log.Fatal(err)
	}
	signals := make(chan os.Signal, 1)
	signal.Notify(signals, os.Interrupt)
	<-signals
	_ = count.Close()
}
