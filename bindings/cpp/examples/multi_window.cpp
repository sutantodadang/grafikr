#include "common.hpp"
#include <array>
#include <iostream>
#include <optional>

int main() {
    grafikr::Application app;
    std::array<std::optional<grafikr::Subscription>, 2> subscriptions;
    std::array<unsigned, 2> clicks{};
    try {
        app.run([&](grafikr::Client& client) {
            for (std::size_t index = 0; index < subscriptions.size(); ++index) {
                auto [window, root] = example::window(client, "Grafikr window " + std::to_string(index + 1));
                auto label_props = grafikr::Value::object();
                example::put(label_props, "text", grafikr::Value::string("Window clicks: 0"));
                auto label = example::node(client, window, "Text", std::move(label_props));
                auto button_props = grafikr::Value::object();
                example::put(button_props, "label", grafikr::Value::string("Update this window"));
                auto button = example::node(client, window, "Button", std::move(button_props));
                example::attach(client, label, root);
                example::attach(client, button, root);
                subscriptions[index].emplace(client.on(example::handle(button), "click", [&client, label, &clicks, index](const grafikr::Value&) {
                    example::set_text(client, label, "Window clicks: " + std::to_string(++clicks[index]));
                }));
            }
        });
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
