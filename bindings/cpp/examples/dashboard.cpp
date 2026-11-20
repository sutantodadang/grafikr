#include "common.hpp"
#include <iostream>
#include <optional>

int main() {
    grafikr::Application app;
    std::optional<grafikr::Subscription> subscription;
    unsigned refreshes = 0;
    try {
        app.run([&](grafikr::Client& client) {
            auto [window, root] = example::window(client, "Grafikr dashboard");
            auto layout = grafikr::Value::object();
            example::put(layout, "gap", grafikr::Value::i64(10));
            auto column = example::node(client, window, "Column", std::move(layout));
            auto heading_props = grafikr::Value::object();
            example::put(heading_props, "text", grafikr::Value::string("Operations dashboard"));
            example::put(heading_props, "size", grafikr::Value::i64(24));
            auto heading = example::node(client, window, "Text", std::move(heading_props));
            auto status_props = grafikr::Value::object();
            example::put(status_props, "text", grafikr::Value::string("Ready"));
            auto status = example::node(client, window, "Text", std::move(status_props));
            auto button_props = grafikr::Value::object();
            example::put(button_props, "label", grafikr::Value::string("Refresh status"));
            auto button = example::node(client, window, "Button", std::move(button_props));
            example::attach(client, column, root);
            example::attach(client, heading, column);
            example::attach(client, status, column);
            example::attach(client, button, column);
            subscription.emplace(client.on(example::handle(button), "click", [&client, status, &refreshes](const grafikr::Value&) {
                example::set_text(client, status, "Updated " + std::to_string(++refreshes) + " times");
            }));
        });
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
