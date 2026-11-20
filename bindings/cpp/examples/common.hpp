#ifndef GRAFIKR_CPP_EXAMPLE_COMMON_HPP
#define GRAFIKR_CPP_EXAMPLE_COMMON_HPP
#include "grafikr.hpp"
#include <string>
#include <utility>

namespace example {
using grafikr::Client;
using grafikr::Value;

inline void put(Value& object, std::string_view key, Value value) {
    object.set(key, std::move(value));
}

inline Value request(Client& client, std::string_view op, Value args) {
    return client.request(op, args);
}

inline std::pair<std::string, std::string> window(Client& client, std::string_view title) {
    auto args = Value::object();
    put(args, "title", Value::string(title));
    put(args, "width", Value::i64(560));
    put(args, "height", Value::i64(400));
    auto result = request(client, "window.create", std::move(args));
    return {result.get("window").as_string(), result.get("root").as_string()};
}

inline std::string node(Client& client, std::string_view window, std::string_view widget, Value properties) {
    auto args = Value::object();
    put(args, "window", Value::string(window));
    put(args, "widget", Value::string(widget));
    put(args, "properties", std::move(properties));
    return request(client, "node.create", std::move(args)).get("node").as_string();
}

inline void attach(Client& client, std::string_view node, std::string_view parent) {
    auto args = Value::object();
    put(args, "node", Value::string(node));
    put(args, "parent", Value::string(parent));
    (void)request(client, "node.attach", std::move(args));
}

inline void set_text(Client& client, std::string_view node, std::string text) {
    auto properties = Value::object();
    put(properties, "text", Value::string(text));
    auto args = Value::object();
    put(args, "node", Value::string(node));
    put(args, "properties", std::move(properties));
    (void)request(client, "property.update", std::move(args));
}

inline std::uint64_t handle(std::string_view value) { return std::stoull(std::string(value)); }
}
#endif
