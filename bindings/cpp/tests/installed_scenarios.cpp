#include "grafikr.hpp"

#include <cstdlib>
#include <cstdio>
#include <memory>
#include <optional>
#include <string>
#include <thread>

namespace {
using grafikr::Application;
using grafikr::Client;
using grafikr::CollectionMetadata;
using grafikr::CollectionProvider;
using grafikr::Value;

struct Scenario {
    Application* app{};
    Client* client{};
    std::string mode;
    std::string window, second_window, label, button, field, state, count_state, binding;
    std::optional<grafikr::Subscription> click, change, selection, modal_change;
    std::unique_ptr<CollectionProvider> provider;
    std::thread data_thread;
    bool clicked{}, text_changed{};
};

void report(const char* step) {
    const char* path = std::getenv("GRAFIKR_SCENARIO_REPORT");
    if (auto* file = path ? std::fopen(path, "ab") : nullptr) {
        std::fprintf(file, "%s\n", step);
        std::fclose(file);
    }
}

Value object() { return Value::object(); }
Value array() { return Value::array(); }

void put(Value& target, std::string_view key, Value value) {
    target.set(key, std::move(value));
}

Value request(Client& client, std::string_view op, Value args) {
    return client.request(op, args);
}

std::string create_window(Scenario& state, std::string_view title, std::string* root = nullptr) {
    auto args = object();
    put(args, "title", Value::string(title));
    put(args, "width", Value::number(480));
    put(args, "height", Value::number(320));
    auto result = request(*state.client, "window.create", std::move(args));
    if (root) *root = result.get("root").as_string();
    return result.get("window").as_string();
}

std::string create_node(Scenario& state, std::string_view window, std::string_view widget, Value properties) {
    auto args = object();
    put(args, "window", Value::string(window));
    put(args, "widget", Value::string(widget));
    put(args, "properties", std::move(properties));
    return request(*state.client, "node.create", std::move(args)).get("node").as_string();
}

void attach(Scenario& state, std::string_view node, std::string_view parent) {
    auto args = object(); put(args, "node", Value::string(node)); put(args, "parent", Value::string(parent));
    (void)request(*state.client, "node.attach", std::move(args));
}

void destroy(Scenario& state, std::string_view op, std::string_view field, std::string_view token) {
    auto args = object(); put(args, field, Value::string(token));
    (void)request(*state.client, op, std::move(args));
}

std::string create_state(Scenario& state, std::string_view type, std::string_view value) {
    auto wire = object(); put(wire, "type", Value::string(type)); put(wire, "value", Value::string(value));
    auto args = object(); put(args, "value", std::move(wire));
    return request(*state.client, "state.create", std::move(args)).get("state").as_string();
}

void set_state(Scenario& state, std::string_view value) {
    auto wire = object(); put(wire, "type", Value::string("string")); put(wire, "value", Value::string(value));
    auto args = object(); put(args, "state", Value::string(state.state)); put(args, "value", std::move(wire));
    (void)request(*state.client, "state.set", std::move(args));
}

std::string get_state(Scenario& state) {
    auto args = object(); put(args, "state", Value::string(state.state));
    return request(*state.client, "state.get", std::move(args)).get("value").as_string();
}

void bind_text(Scenario& state, std::string_view direction = "two_way") {
    auto args = object();
    put(args, "state", Value::string(state.state)); put(args, "node", Value::string(state.field));
    put(args, "property", Value::string("text")); put(args, "direction", Value::string(direction));
    state.binding = request(*state.client, "binding.bind", std::move(args)).get("binding").as_string();
}

void finish_counter(Scenario& state) {
    if (!state.clicked || !state.text_changed) return;
    if (state.click) { state.click->close(); state.click.reset(); }
    if (state.change) { state.change->close(); state.change.reset(); }
    destroy(state, "binding.unbind", "binding", state.binding);
    destroy(state, "state.destroy", "state", state.state);
    destroy(state, "state.destroy", "state", state.count_state);
    destroy(state, "window.close", "window", state.window);
    state.app->stop();
}

void on_click(Scenario& state) {
    auto args = object(); put(args, "state", Value::string(state.count_state));
    const auto count = request(*state.client, "state.get", std::move(args)).get("value").as_string();
    const auto next = std::to_string(std::stoll(count) + 1);
    auto wire = object(); put(wire, "type", Value::string("i64")); put(wire, "value", Value::string(next));
    args = object(); put(args, "state", Value::string(state.count_state)); put(args, "value", std::move(wire));
    (void)request(*state.client, "state.set", std::move(args));
    auto properties = object(); put(properties, "text", Value::string("Count: " + next));
    args = object(); put(args, "node", Value::string(state.label)); put(args, "properties", std::move(properties));
    (void)request(*state.client, "property.update", std::move(args));
    state.clicked = true;
    report("counter.click-state-update");
    finish_counter(state);
}

void on_text(Scenario& state, const Value& event) {
    const auto payload = event.get("payload");
    if (payload.get("value").as_string() != (state.mode == "counter" ? "Input 雪" : "Confirm 雪")) return;
    if (get_state(state) != payload.get("value").as_string()) throw std::runtime_error("two-way binding did not update state");
    state.text_changed = true;
    report(state.mode == "counter" ? "counter.unicode-binding" : "multi-window.keyboard-binding");
    if (state.mode == "counter") finish_counter(state);
}

void on_modal_change(Scenario& state, const Value& event) {
    const auto payload = event.get("payload");
    const auto value = payload.get("value");
    if (value.as_bool()) return;
    report("multi-window.modal-escape");
    if (state.change) { state.change->close(); state.change.reset(); }
    if (state.modal_change) { state.modal_change->close(); state.modal_change.reset(); }
    destroy(state, "binding.unbind", "binding", state.binding);
    destroy(state, "state.destroy", "state", state.state);
    destroy(state, "window.close", "window", state.second_window);
    report("multi-window-modal-close");
    state.app->stop();
}

void counter_setup(Scenario& state) {
    std::string root;
    state.window = create_window(state, "Grafikr installed counter", &root);
    state.state = create_state(state, "string", "Grafikr 雪");
    state.count_state = create_state(state, "i64", "0");
    auto label = object(); put(label, "text", Value::string("Count: 0"));
    state.label = create_node(state, state.window, "Text", std::move(label));
    auto button = object(); put(button, "label", Value::string("Increment"));
    state.button = create_node(state, state.window, "Button", std::move(button));
    auto field = object(); put(field, "text", Value::string("Grafikr 雪")); put(field, "width", Value::number(240));
    state.field = create_node(state, state.window, "TextField", std::move(field));
    attach(state, state.label, root); attach(state, state.button, root); attach(state, state.field, root);
    bind_text(state);
    state.click.emplace(state.client->on(std::stoull(state.button), "click", [&state](const Value&) { on_click(state); }));
    state.change.emplace(state.client->on(std::stoull(state.field), "change", [&state](const Value& event) { on_text(state, event); }));
    report("counter.ready");
}

Value canvas_commands() {
    auto rect = object(); put(rect, "x", Value::number(0)); put(rect, "y", Value::number(0));
    put(rect, "width", Value::number(32)); put(rect, "height", Value::number(24));
    auto command = object(); put(command, "kind", Value::string("rect")); put(command, "rect", std::move(rect));
    put(command, "color", Value::string("#336699")); put(command, "radius", Value::number(0));
    auto commands = array(); commands.push(std::move(command)); return commands;
}

void dashboard_worker(Scenario& state) {
    try {
        auto range = state.provider->next_request(5000);
        if (!range || range->start != 0 || range->count == 0 || range->count > 512) throw std::runtime_error("visible collection range missing");
        auto rows = array();
        for (std::uint32_t index = 0; index < range->count; ++index) {
            auto row = array(); row.push(Value::string("Row " + std::to_string(index))); rows.push(std::move(row));
        }
        state.provider->provide(*range, std::move(rows));
        report("dashboard.initial-range");
        auto scrolled = state.provider->next_request(10000);
        if (!scrolled || scrolled->start < 49000 || scrolled->start + scrolled->count != 50000 || scrolled->count == 0 || scrolled->count > 512)
            throw std::runtime_error("scrolled collection range missing");
        auto final_rows = array();
        for (std::uint32_t index = 0; index < scrolled->count; ++index) {
            auto row = array(); row.push(Value::string("Row " + std::to_string(scrolled->start + index)));
            final_rows.push(std::move(row));
        }
        state.provider->provide(*scrolled, std::move(final_rows));
        report("dashboard.scroll-range");
        if (state.selection) { state.selection->close(); state.selection.reset(); }
        destroy(state, "window.close", "window", state.window);
        report("dashboard.multi-resource-cleanup");
    } catch (const std::exception& error) {
        report("dashboard.setup-failed");
        std::fprintf(stderr, "installed C++ dashboard failed: %s\n", error.what());
    }
    state.app->stop();
}

void dashboard_setup(Scenario& state) {
    std::string root;
    state.window = create_window(state, "Grafikr installed dashboard", &root);
    auto list = object(); put(list, "row_count", Value::i64(50000)); put(list, "row_height", Value::number(24));
    put(list, "width", Value::number(600)); put(list, "height", Value::number(360));
    state.field = create_node(state, state.window, "VirtualList", std::move(list)); attach(state, state.field, root);
    grafikr::CollectionMetadata metadata{50000, 24, {"label"}};
    state.provider = std::make_unique<CollectionProvider>(CollectionProvider::create(*state.client, std::stoull(state.field), metadata));
    state.selection.emplace(state.client->on(std::stoull(state.field), "select", [](const Value& event) {
        if (event.get("payload").get("index").as_string() == "49999") report("dashboard.selection");
    }));

    auto canvas = object(); put(canvas, "commands", array()); put(canvas, "label", Value::string("Dashboard canvas"));
    put(canvas, "width", Value::number(320)); put(canvas, "height", Value::number(120));
    state.label = create_node(state, state.window, "Canvas", std::move(canvas)); attach(state, state.label, root);
    auto args = object(); put(args, "node", Value::string(state.label)); put(args, "commands", canvas_commands());
    (void)request(*state.client, "canvas.update", std::move(args)); report("dashboard.canvas");

    args = object(); put(args, "width", Value::i64(1)); put(args, "height", Value::i64(1)); put(args, "byte_length", Value::i64(4));
    auto upload = request(*state.client, "image.begin", std::move(args)).get("upload").as_string();
    args = object(); put(args, "upload", Value::string(upload)); put(args, "offset", Value::i64(0)); put(args, "data", Value::string("AAAA/w=="));
    (void)request(*state.client, "image.chunk", std::move(args));
    auto image = request(*state.client, "image.finalize", [&] { auto value = object(); put(value, "upload", Value::string(upload)); return value; }()).get("image").as_string();
    auto image_properties = object(); put(image_properties, "image", Value::string(image)); put(image_properties, "width", Value::i64(1));
    put(image_properties, "height", Value::i64(1)); put(image_properties, "label", Value::string("Pixel"));
    state.button = create_node(state, state.window, "Image", std::move(image_properties)); attach(state, state.button, root);
    args = object(); put(args, "image", Value::string(image)); (void)request(*state.client, "image.destroy", std::move(args));
    destroy(state, "node.destroy", "node", state.button); report("dashboard.image-resource-cleanup"); report("dashboard.ready");
    state.data_thread = std::thread([&state] { dashboard_worker(state); });
}

void multi_setup(Scenario& state) {
    std::string root, second_root;
    state.window = create_window(state, "Grafikr first window", &root);
    state.second_window = create_window(state, "Grafikr second window", &second_root);
    auto modal = object(); put(modal, "open", Value::boolean(true)); put(modal, "width", Value::number(300));
    state.label = create_node(state, state.second_window, "Modal", std::move(modal)); attach(state, state.label, second_root);
    state.modal_change.emplace(state.client->on(std::stoull(state.label), "change", [&state](const Value& event) { on_modal_change(state, event); }));
    state.state = create_state(state, "string", "Type here");
    auto field = object(); put(field, "text", Value::string("Type here")); put(field, "width", Value::number(240));
    state.field = create_node(state, state.second_window, "TextField", std::move(field)); attach(state, state.field, state.label);
    bind_text(state);
    destroy(state, "window.close", "window", state.window);
    set_state(state, "state-survives-first-close");
    if (get_state(state) != "state-survives-first-close") throw std::runtime_error("remaining window lost shared state");
    set_state(state, "Type here");
    report("multi-window-state-survives-first-close");
    state.change.emplace(state.client->on(std::stoull(state.field), "change", [&state](const Value& event) { on_text(state, event); }));
    report("multi-window.ready");
}
}

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    Scenario state; state.mode = argv[1];
    Application app; state.app = &app;
    app.run([&](Client& client) {
        state.client = &client;
        if (state.mode == "counter") counter_setup(state);
        else if (state.mode == "dashboard") dashboard_setup(state);
        else if (state.mode == "multi") multi_setup(state);
        else throw std::runtime_error("unknown installed scenario");
    });
    if (state.data_thread.joinable()) state.data_thread.join();
    return 0;
}
