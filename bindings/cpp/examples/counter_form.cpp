#include "common.hpp"
#include <iostream>
#include <optional>

int main() {
    grafikr::Application app;
    std::optional<grafikr::Subscription> subscription;
    int count = 0;
    try {
        app.run([&](grafikr::Client& client) {
            auto [window, root] = example::window(client, "Grafikr counter form");
            auto layout = grafikr::Value::object();
            example::put(layout, "gap", grafikr::Value::i64(12));
            auto column = example::node(client, window, "Column", std::move(layout));
            auto field_props = grafikr::Value::object();
            example::put(field_props, "text", grafikr::Value::string("Type here"));
            example::put(field_props, "width", grafikr::Value::i64(240));
            auto field = example::node(client, window, "TextField", std::move(field_props));
            auto label_props = grafikr::Value::object();
            example::put(label_props, "text", grafikr::Value::string("Count: 0"));
            auto label = example::node(client, window, "Text", std::move(label_props));
            auto button_props = grafikr::Value::object();
            example::put(button_props, "label", grafikr::Value::string("Add one"));
            auto button = example::node(client, window, "Button", std::move(button_props));
            example::attach(client, column, root);
            example::attach(client, field, column);
            example::attach(client, label, column);
            example::attach(client, button, column);
            subscription.emplace(client.on(example::handle(button), "click", [&client, label, &count](const grafikr::Value&) {
                example::set_text(client, label, "Count: " + std::to_string(++count));
            }));
        });
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
